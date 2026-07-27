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
        BenchApp, BenchCorpus, BenchSnapshot, FILTER_TAB_COUNT, METADATA_KIND_COUNT,
        app_with_corpus, app_with_entries, close_picker, draw_frame, filter_metadata_picker,
        filter_tab_rows, first_entry_path, install_snapshot, library_snapshot, open_filter,
        open_location_picker, open_metadata_picker, refresh_path, reload_journal_list,
        rename_journal, search,
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
