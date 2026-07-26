use notema_timing as timing;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
};

/// What one poll of a [`PendingWatcher`] found.
pub(crate) struct WatcherPoll {
    pub(crate) paths: Vec<PathBuf>,
    /// A registration that will never produce events. Reported on the single
    /// poll that observes it, then never again.
    pub(crate) failure: Option<String>,
}

/// A watcher whose registration runs on a background thread.
///
/// Registering recursively costs one OS watch per directory on inotify, which
/// on FUSE-backed storage runs to seconds — too much to spend before the first
/// frame.
pub(crate) struct PendingWatcher {
    state: State,
}

enum State {
    Registering(mpsc::Receiver<Result<FileWatcher, String>>),
    Watching(FileWatcher),
    Off,
}

impl PendingWatcher {
    /// Never watches anything: for platforms where `notify` doesn't work.
    pub(crate) fn off() -> Self {
        Self { state: State::Off }
    }

    pub(crate) fn start(root: &Path, label: &'static str) -> Self {
        Self::start_then(root, label, || {})
    }

    /// Registers `root`, then runs `after` on the same thread.
    ///
    /// The sequencing is the point: `Watcher::watch` acks only once the watch is
    /// armed, so anything `after` reads from the tree is either already covered
    /// by the watch or was seen by `after` itself. Running the two concurrently
    /// would leave changes in the registration window observed by neither.
    pub(crate) fn start_then(
        root: &Path,
        label: &'static str,
        after: impl FnOnce() + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let root = root.to_path_buf();
        std::thread::spawn(move || {
            let registered = FileWatcher::start(&root).map_err(|error| error.to_string());
            timing::event(label);
            // Hand the watcher over before `after` runs, so a caller polling both
            // sees the watcher no later than `after`'s result.
            let _ = tx.send(registered);
            after();
        });
        Self {
            state: State::Registering(rx),
        }
    }

    /// Adopts a finished registration, then drains whatever it has seen.
    pub(crate) fn poll(&mut self) -> WatcherPoll {
        if let State::Registering(rx) = &self.state {
            match rx.try_recv() {
                Ok(Ok(watcher)) => self.state = State::Watching(watcher),
                Ok(Err(error)) => {
                    self.state = State::Off;
                    return WatcherPoll {
                        paths: Vec::new(),
                        failure: Some(error),
                    };
                }
                Err(mpsc::TryRecvError::Empty) => {
                    return WatcherPoll {
                        paths: Vec::new(),
                        failure: None,
                    };
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.state = State::Off;
                    return WatcherPoll {
                        paths: Vec::new(),
                        failure: Some("registration stopped unexpectedly".to_string()),
                    };
                }
            }
        }
        WatcherPoll {
            paths: match &self.state {
                State::Watching(watcher) => watcher.poll_changes(),
                _ => Vec::new(),
            },
            failure: None,
        }
    }
}

pub(crate) struct FileWatcher {
    rx: mpsc::Receiver<PathBuf>,
    _watcher: RecommendedWatcher,
}

impl FileWatcher {
    pub(crate) fn start(root: &Path) -> notify::Result<Self> {
        let (tx, rx) = mpsc::channel();

        let filter_root = root.to_path_buf();
        let mut watcher = RecommendedWatcher::new(
            move |res: notify::Result<Event>| {
                if let Ok(event) = res {
                    // Forward each changed path so the app can reload just the
                    // affected entries instead of re-reading the whole corpus.
                    for path in event
                        .paths
                        .into_iter()
                        .filter(|p| is_relevant(&filter_root, p))
                    {
                        let _ = tx.send(path);
                    }
                }
            },
            Config::default(),
        )?;

        watcher.watch(root, RecursiveMode::Recursive)?;

        Ok(Self {
            rx,
            _watcher: watcher,
        })
    }

    /// Drain and return every changed path seen since the last poll (empty when
    /// nothing changed).
    pub(crate) fn poll_changes(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        while let Ok(path) = self.rx.try_recv() {
            paths.push(path);
        }
        paths
    }
}

/// Hidden-file filter, applied only to the path *below* the watch root — a
/// root that itself lives under a dot directory (`~/.config/notema/themes`)
/// must still report its children. A path outside the root (e.g. notify
/// reporting a canonicalized form) falls back to the whole-path check rather
/// than being dropped.
fn is_relevant(root: &Path, path: &Path) -> bool {
    let below = path.strip_prefix(root).unwrap_or(path);
    !below
        .components()
        .any(|c| c.as_os_str().to_str().is_some_and(|s| s.starts_with('.')))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registering() -> (mpsc::Sender<Result<FileWatcher, String>>, PendingWatcher) {
        let (tx, rx) = mpsc::channel();
        (
            tx,
            PendingWatcher {
                state: State::Registering(rx),
            },
        )
    }

    #[test]
    fn a_failed_registration_is_reported_once() {
        let (tx, mut watcher) = registering();
        tx.send(Err("no watches left".to_string())).unwrap();

        let first = watcher.poll();
        assert_eq!(first.failure.as_deref(), Some("no watches left"));
        assert!(first.paths.is_empty());
        assert!(watcher.poll().failure.is_none());
    }

    #[test]
    fn a_registration_thread_that_dies_is_reported_once() {
        let (tx, mut watcher) = registering();
        drop(tx);

        assert!(watcher.poll().failure.is_some());
        assert!(watcher.poll().failure.is_none());
    }

    #[test]
    fn a_pending_registration_reports_nothing_yet() {
        let (_tx, mut watcher) = registering();

        let poll = watcher.poll();
        assert!(poll.paths.is_empty());
        assert!(poll.failure.is_none());
    }

    #[test]
    fn a_disabled_watcher_stays_quiet() {
        let mut watcher = PendingWatcher::off();

        for _ in 0..3 {
            let poll = watcher.poll();
            assert!(poll.paths.is_empty());
            assert!(poll.failure.is_none());
        }
    }

    #[test]
    fn the_follow_up_runs_even_when_registration_fails() {
        let (done_tx, done_rx) = mpsc::channel();
        let mut watcher = PendingWatcher::start_then(
            Path::new("/notema-no-such-directory/watch-target"),
            "watch:test",
            move || {
                let _ = done_tx.send(());
            },
        );

        done_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("follow-up must run whether or not the watch registered");
        // The registration result is already queued: the follow-up only runs
        // after it is sent.
        assert!(watcher.poll().failure.is_some());
    }

    #[test]
    fn dot_filter_applies_below_the_watch_root_only() {
        let root = Path::new("/home/u/.config/notema/themes");
        assert!(is_relevant(root, &root.join("journal.toml")));
        assert!(!is_relevant(root, &root.join(".journal.toml.swp")));
        assert!(!is_relevant(
            root,
            &root.join(".hidden").join("journal.toml")
        ));
        // A path that doesn't strip against the root keeps the whole-path check.
        assert!(!is_relevant(root, Path::new("/elsewhere/.hidden/x.toml")));
        assert!(is_relevant(root, Path::new("/elsewhere/visible/x.toml")));
    }
}
