#![forbid(unsafe_code)]

mod cli;
mod config;
mod licenses;
mod platform;
mod startup;
mod tui;

pub(crate) type AppResult<T> = anyhow::Result<T>;

pub fn run() -> anyhow::Result<()> {
    notema_timing::init();
    notema_timing::mark("main");
    let result = cli::run();
    notema_timing::mark("total");
    result
}

/// Bench-only handles onto the otherwise-private TUI hot paths (search, full-frame
/// render, filter browser). Gated behind the `bench` feature so it never widens
/// the shipped API.
#[cfg(feature = "bench")]
pub mod bench {
    pub use crate::tui::bench_support::{
        BenchApp, BenchCorpus, BenchSnapshot, BenchTerminal, FILTER_TAB_COUNT, METADATA_KIND_COUNT,
        app_with_corpus, app_with_entries, bench_terminal, close_picker, draw_frame,
        editor_highlight, editor_input, filter_metadata_picker, filter_rows_cold, filter_tab_rows,
        first_entry_path, insights_analytics, insights_drivers, install_snapshot, library_snapshot,
        open_editor_with_body, open_filter, open_location_picker, open_metadata_picker,
        refresh_path, reload_journal_list, rename_journal, search, search_query,
    };
}

/// The command a device runs to request access to an already-encrypted store.
/// Referenced from CLI errors and the TUI enroll notice so the wording lives in
/// one place.
pub(crate) const ENROLL_CMD: &str = "notema encryption device enroll";

/// The command an approving device runs to admit a pending join request. A
/// device name is appended when one is known. Shared by the CLI prompts and the
/// TUI awaiting-approval notice so the wording lives in one place.
pub(crate) const APPROVE_CMD: &str = "notema encryption device approve";

/// The command that writes a standalone copy of this device's key. Named
/// wherever a backup is advised, since the identity file is only the backup
/// while the key is actually in it.
pub(crate) const EXPORT_KEY_CMD: &str = "notema encryption key export";

/// The command that moves this device's key into the OS keychain, offered when
/// an automatic move could not complete.
pub(crate) const KEY_STORE_KEYRING_CMD: &str = "notema encryption key store keyring";

/// The command that turns encryption on, pointed at from every surface that
/// notices a journal doesn't have it yet.
pub(crate) const ENABLE_CMD: &str = "notema encryption enable";

/// The command that shows the roster and any pending join requests.
pub(crate) const DEVICE_LIST_CMD: &str = "notema encryption device list";
