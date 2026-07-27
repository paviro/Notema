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
    /// The user asked. `rebuild` re-reads every entry from source instead of
    /// trusting the cache's stamps.
    Manual {
        rebuild: bool,
    },
}

impl ReloadReason {
    /// The louder of two reasons, so folding a request into one already running
    /// never turns a manual refresh into a silent one, or a rebuild into a
    /// stamp check that would leave the cache exactly as it was.
    pub(crate) fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Manual { rebuild: a }, Self::Manual { rebuild: b }) => {
                Self::Manual { rebuild: a || b }
            }
            (manual @ Self::Manual { .. }, Self::Automatic)
            | (Self::Automatic, manual @ Self::Manual { .. }) => manual,
            (Self::Automatic, Self::Automatic) => Self::Automatic,
        }
    }

    pub(crate) fn is_manual(self) -> bool {
        matches!(self, Self::Manual { .. })
    }

    pub(crate) fn rebuilds(self) -> bool {
        matches!(self, Self::Manual { rebuild: true })
    }

    fn policy(self) -> CachePolicy {
        if self.rebuilds() {
            CachePolicy::Rebuild
        } else {
            CachePolicy::Normal
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
        .load_library(request.reason.policy())
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

    const REFRESH: ReloadReason = ReloadReason::Manual { rebuild: false };
    const REBUILD: ReloadReason = ReloadReason::Manual { rebuild: true };

    #[test]
    fn merging_never_silences_a_manual_refresh() {
        use ReloadReason::Automatic;

        assert_eq!(Automatic.merge(Automatic), Automatic);
        assert_eq!(Automatic.merge(REFRESH), REFRESH);
        assert_eq!(REFRESH.merge(Automatic), REFRESH);
        assert_eq!(REFRESH.merge(REFRESH), REFRESH);
    }

    #[test]
    fn merging_never_downgrades_a_rebuild() {
        use ReloadReason::Automatic;

        // A stamp check would leave the cache exactly as the rebuild found it,
        // so the rebuild has to win however the two arrive.
        assert_eq!(REBUILD.merge(REFRESH), REBUILD);
        assert_eq!(REFRESH.merge(REBUILD), REBUILD);
        assert_eq!(REBUILD.merge(Automatic), REBUILD);
        assert_eq!(Automatic.merge(REBUILD), REBUILD);
    }

    #[test]
    fn only_a_rebuild_ignores_the_cache() {
        use ReloadReason::Automatic;

        assert_eq!(REBUILD.policy(), CachePolicy::Rebuild);
        assert_eq!(REFRESH.policy(), CachePolicy::Normal);
        assert_eq!(Automatic.policy(), CachePolicy::Normal);
    }
}
