//! Whole-library reloads over the shared [`Worker`], so a corpus walk never
//! runs on the event loop.

use crate::tui::runtime::worker::Worker;
use notema_storage::{CachePolicy, JournalStore, LibrarySnapshot};
use notema_timing as timing;

pub(crate) type LibraryReloadWorker = Worker<ReloadRequest, ReloadResult>;

/// Why a reload was asked for. A manual one owns a progress toast and reports
/// when it lands; an automatic one is quiet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReloadReason {
    Automatic,
    Manual,
}

impl ReloadReason {
    /// The louder of two reasons, so folding a request into one already running
    /// never turns a manual refresh into a silent one.
    pub(crate) fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Manual, _) | (_, Self::Manual) => Self::Manual,
            _ => Self::Automatic,
        }
    }
}

pub(crate) struct ReloadRequest {
    pub(crate) store: JournalStore,
    pub(crate) reason: ReloadReason,
    /// The library this was asked for. A result carrying a generation the app
    /// has moved past describes a corpus that no longer exists.
    pub(crate) generation: u64,
}

pub(crate) struct ReloadResult {
    pub(crate) reason: ReloadReason,
    pub(crate) generation: u64,
    pub(crate) snapshot: Result<LibrarySnapshot, String>,
}

/// Run one reload. On the worker thread.
pub(crate) fn reload(request: ReloadRequest) -> ReloadResult {
    let snapshot = request
        .store
        .load_library(CachePolicy::Normal)
        .map_err(|error| format!("{error:#}"));
    // Timestamped where the walk finished, not where the loop gets around to
    // reading it, and emitted even for a result later discarded as stale.
    if let Ok(snapshot) = &snapshot {
        timing::event_with(|| snapshot.report.timing_summary());
    }
    ReloadResult {
        reason: request.reason,
        generation: request.generation,
        snapshot,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merging_never_silences_a_manual_refresh() {
        use ReloadReason::{Automatic, Manual};

        assert_eq!(Automatic.merge(Automatic), Automatic);
        assert_eq!(Automatic.merge(Manual), Manual);
        assert_eq!(Manual.merge(Automatic), Manual);
        assert_eq!(Manual.merge(Manual), Manual);
    }
}
