use crate::{
    AppResult,
    cli::prompts,
    config::{self, Config},
    platform::ish,
};
use anyhow::bail;
use notema_storage::{CachePolicy, JournalStore, LibraryDiscovery, LibraryLoadProgress};
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

/// What `load_or_setup_with_path` resolved: a store ready to open in the TUI. An
/// encrypted store this device can't yet read is opened too — the TUI shows the
/// enroll/awaiting notice rather than the CLI printing it.
pub(crate) struct Startup {
    pub config_path: PathBuf,
    pub config: Config,
    pub store: JournalStore,
    pub discovery: Option<LibraryDiscovery>,
}

pub(crate) fn load_or_setup_with_path(path_override: Option<&Path>) -> AppResult<Startup> {
    let config_path = config_path(path_override)?;

    // An encrypted store this device can't yet read (no key, awaiting approval, or
    // revoked) is still opened: the TUI shows the enroll/awaiting notice instead
    // of the CLI printing a hint, so every unreadable-store case looks the same.
    // Reconciling a remote encryption *disable* is likewise deferred to the TUI,
    // which must run it before probing for a lock.
    let (config, store, discovery) = if config_path.exists() {
        let config = config::load_config(&config_path)?;
        let prepared = ish::prepare_store(&config_path, &config.journal.path, true)?;
        (config, prepared.store, prepared.discovery)
    } else {
        let (config, store) = interactive_setup(&config_path)?;
        (config, store, None)
    };

    Ok(Startup {
        config_path,
        config,
        store,
        discovery,
    })
}

pub(crate) fn load_existing(path_override: Option<&Path>) -> AppResult<Startup> {
    let config_path = config_path(path_override)?;
    if !config_path.exists() {
        bail!(
            "config file not found at {}; run `journal` once to set it up or pass --config <DIR>",
            config_path.display()
        );
    }

    let config = config::load_config(&config_path)?;
    let store = ish::prepare_store(&config_path, &config.journal.path, false)?.store;
    if store.reconcile_disabled_encryption()? {
        eprintln!(
            "Note: encryption was disabled on another device; retired this device's key and trust pins."
        );
    }
    Ok(Startup {
        config_path,
        config,
        store,
        discovery: None,
    })
}

/// Resolve the config *file* from an optional config-directory override. The
/// override names the directory that holds `config.toml` alongside this device's
/// encryption key; without one we fall back to the XDG default.
fn config_path(path_override: Option<&Path>) -> AppResult<PathBuf> {
    match path_override {
        Some(dir) => {
            // `--config` names the directory, not the file. Passing a file (a
            // stale `.../config.toml`) would silently nest into
            // `.../config.toml/config.toml` and trigger a bogus first-run setup.
            if dir.is_file() || dir.file_name() == Some(std::ffi::OsStr::new("config.toml")) {
                bail!(
                    "--config takes a directory, not a file; pass {} instead",
                    dir.parent().unwrap_or(dir).display()
                );
            }
            Ok(dir.join("config.toml"))
        }
        None => config::default_config_path(),
    }
}

fn interactive_setup(config_path: &Path) -> AppResult<(Config, JournalStore)> {
    let mut stdout = io::stdout();
    let default_root = dirs::home_dir()
        .map(|home| home.join("Journals"))
        .unwrap_or_else(|| PathBuf::from("Journals"));

    writeln!(stdout, "Notema first-run setup")?;

    // On iSH the journal lives on a mounted iOS folder, so there's no path to
    // type: use a fixed mountpoint and let the iOS picker choose the folder.
    let journal_root = if ish::is_ish() {
        let mountpoint = PathBuf::from(ish::DEFAULT_MOUNTPOINT);
        ish::ensure_journal_mounted(&mountpoint)?;
        mountpoint
    } else {
        write!(
            stdout,
            "Journal root [{}]: ",
            default_root.to_string_lossy()
        )?;
        stdout.flush()?;

        let mut root_input = String::new();
        io::stdin().read_line(&mut root_input)?;
        if root_input.trim().is_empty() {
            default_root
        } else {
            PathBuf::from(root_input.trim())
        }
    };

    let mut config = Config::new(journal_root);

    // E-ink and other limited-palette displays get `classic`, which renders on
    // terminals without true-color; everything else starts on the default theme.
    // Such devices also ignore per-journal themes, which mostly don't render here.
    write!(stdout, "Is this an e-ink / monochrome display? [y/N]: ")?;
    stdout.flush()?;
    let mut eink_input = String::new();
    io::stdin().read_line(&mut eink_input)?;
    if crate::cli::prompts::is_yes(&eink_input) {
        config.ui.theme = "classic".to_string();
        config.ui.ignore_journal_themes = true;
    }
    let prepared = ish::prepare_store(config_path, &config.journal.path, true)?;
    let store = prepared.store;

    if should_offer_encryption(&store)? {
        offer_encryption(&mut stdout, &store)?;
    } else if !store.encryption_enabled() {
        // An existing plaintext journal is registered as-is; encryption stays a
        // deliberate later step rather than a first-run prompt.
        writeln!(
            stdout,
            "Using existing journal at {}. Encryption is off; run `notema encryption enable` to turn it on.",
            config.journal.path.display()
        )?;
    }

    if ish::is_ish() {
        writeln!(
            stdout,
            "Note: iSH does not support live file watching. Notema refreshes automatically on each startup; new entries may take a little time to appear while that refresh runs. Press r to refresh immediately."
        )?;
    }

    config::save_config(config_path, &config)?;
    if !store.encryption_enabled() {
        let progress = Mutex::new(SetupCacheProgress::new(&mut stdout, ish::is_ish()));
        let update = |update| {
            progress
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .update(update);
        };
        let snapshot = match prepared.discovery {
            Some(discovery) => store.load_discovered_library_with_progress(
                CachePolicy::Rebuild,
                discovery,
                &update,
            ),
            None => store.load_library_with_progress(CachePolicy::Rebuild, &update),
        };
        let progress_result = progress
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .finish();
        progress_result?;
        let snapshot = snapshot?;
        if let Some(warning) = snapshot.report.cache_warning {
            writeln!(stdout, "Warning: {warning}")?;
        }
    }
    Ok((config, store))
}

struct SetupCacheProgress<'a, W: Write> {
    writer: &'a mut W,
    ish: bool,
    discovery_started: bool,
    reading_started: bool,
    discovered: usize,
    last_discovered: usize,
    last_read: usize,
    error: Option<io::Error>,
}

impl<'a, W: Write> SetupCacheProgress<'a, W> {
    const WIDTH: usize = 20;

    fn new(writer: &'a mut W, ish: bool) -> Self {
        Self {
            writer,
            ish,
            discovery_started: false,
            reading_started: false,
            discovered: 0,
            last_discovered: 0,
            last_read: 0,
            error: None,
        }
    }

    fn update(&mut self, update: LibraryLoadProgress) {
        if self.error.is_some() {
            return;
        }

        let result = match update {
            LibraryLoadProgress::Discovering { entries_found } => {
                self.update_discovery(entries_found)
            }
            LibraryLoadProgress::Reading { current, total } => self.update_reading(current, total),
        };
        if let Err(error) = result {
            self.error = Some(error);
        }
    }

    fn update_discovery(&mut self, entries_found: usize) -> io::Result<()> {
        self.discovered = entries_found;
        if !self.discovery_started {
            if self.ish {
                writeln!(self.writer, "First setup: scanning journal files.")?;
                writeln!(
                    self.writer,
                    "This can take a long time on iSH; later starts are fast."
                )?;
            }
            self.discovery_started = true;
        }
        if entries_found != 0 && entries_found.saturating_sub(self.last_discovered) < 25 {
            return Ok(());
        }
        self.last_discovered = entries_found;
        write!(
            self.writer,
            "\rScanning journal files… {entries_found} entries found"
        )?;
        self.writer.flush()
    }

    fn update_reading(&mut self, current: usize, total: usize) -> io::Result<()> {
        if !self.reading_started {
            if self.discovery_started {
                if self.discovered != self.last_discovered {
                    self.last_discovered = self.discovered;
                    write!(
                        self.writer,
                        "\rScanning journal files… {} entries found",
                        self.discovered
                    )?;
                }
                writeln!(self.writer)?;
            }
            self.reading_started = true;
        }
        if current < self.last_read {
            return Ok(());
        }
        let step = (total / 100).max(1);
        if current != 0 && current != total && current.saturating_sub(self.last_read) < step {
            return Ok(());
        }
        self.last_read = current;
        let filled = current
            .min(total)
            .saturating_mul(Self::WIDTH)
            .checked_div(total)
            .unwrap_or(Self::WIDTH);
        let bar = format!("{}{}", "#".repeat(filled), "-".repeat(Self::WIDTH - filled));
        write!(
            self.writer,
            "\rIndexing journal entries [{bar}] {current}/{total}"
        )?;
        self.writer.flush()
    }

    fn finish(mut self) -> io::Result<()> {
        if let Some(error) = self.error.take() {
            return Err(error);
        }
        writeln!(self.writer)
    }
}

/// First-run offers to enable encryption only for a brand-new, empty root — never
/// for a journal that already has entries or is already encrypted. Those are just
/// registered; encryption is managed with the `notema encryption …` commands.
fn should_offer_encryption(store: &JournalStore) -> AppResult<bool> {
    Ok(!store.encryption_enabled() && store.list_journals()?.is_empty())
}

/// Prompt to enable encryption on a fresh store and, if accepted, generate this
/// device's identity. Holds for a keypress afterward because the TUI's alternate
/// screen would otherwise wipe the identity-backup warning.
fn offer_encryption(stdout: &mut impl Write, store: &JournalStore) -> AppResult<()> {
    write!(stdout, "Enable encryption? [y/N]: ")?;
    stdout.flush()?;
    let mut encryption_input = String::new();
    io::stdin().read_line(&mut encryption_input)?;
    if !crate::cli::prompts::is_yes(&encryption_input) {
        return Ok(());
    }

    let (device_name, passphrase) = prompts::resolve_new_identity_options(None, false)?;
    store.initialize_encryption(&device_name, passphrase.as_ref())?;
    writeln!(
        stdout,
        "Identity file: {}. Back it up; without it encrypted journal files cannot be decrypted.",
        store.identity_path().display()
    )?;
    if passphrase.is_none() {
        writeln!(
            stdout,
            "This key has no passphrase, so anyone with this file can read the journal — keep the device and its backups secure."
        )?;
    }

    write!(stdout, "\nPress Enter to open your journal…")?;
    stdout.flush()?;
    let mut ack = String::new();
    io::stdin().read_line(&mut ack)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn store_in(dir: &Path) -> JournalStore {
        let store =
            JournalStore::for_config(&dir.join("config.toml"), &dir.join("journals")).unwrap();
        store.ensure().unwrap();
        store
    }

    #[test]
    fn offers_encryption_only_for_an_empty_new_root() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        assert!(should_offer_encryption(&store).unwrap());
    }

    #[test]
    fn skips_encryption_prompt_for_an_existing_plaintext_journal() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        store.create_journal("work").unwrap();
        assert!(!should_offer_encryption(&store).unwrap());
    }

    #[test]
    fn skips_encryption_prompt_for_an_already_encrypted_store() {
        let dir = tempdir().unwrap();
        let store = store_in(dir.path());
        store.initialize_encryption("laptop", None).unwrap();
        assert!(!should_offer_encryption(&store).unwrap());
    }

    #[test]
    fn config_path_rejects_a_file_argument() {
        let err = config_path(Some(Path::new("/some/dir/config.toml"))).unwrap_err();
        assert!(err.to_string().contains("takes a directory"));
    }

    #[test]
    fn first_start_cache_progress_reaches_a_full_bar() {
        let mut output = Vec::new();
        let mut progress = SetupCacheProgress::new(&mut output, true);

        progress.update(LibraryLoadProgress::Discovering { entries_found: 0 });
        progress.update(LibraryLoadProgress::Discovering { entries_found: 4 });
        progress.update(LibraryLoadProgress::Reading {
            current: 0,
            total: 4,
        });
        progress.update(LibraryLoadProgress::Reading {
            current: 2,
            total: 4,
        });
        progress.update(LibraryLoadProgress::Reading {
            current: 4,
            total: 4,
        });
        progress.finish().unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("First setup: scanning journal files."));
        assert!(output.contains("later starts are fast"));
        assert!(output.contains("Scanning journal files… 0 entries found"));
        assert!(output.contains("[##########----------] 2/4"));
        assert!(output.contains("[####################] 4/4"));
        assert!(output.ends_with('\n'));
    }
}
