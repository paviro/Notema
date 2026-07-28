use super::*;
use crate::tui::features::feelings::FeelingRow;
use crate::tui::state::ListNav;
use crate::tui::state::MetadataKind;
use crate::tui::test_support::{app_with_bodies, app_with_journals, new_app, new_app_with_state};
use notema_domain::FEELING_GROUPS;
use std::fs;
use tempfile::tempdir;

/// Drive a requested reload to completion the way the event loop does — poll
/// until the worker's result has been drained and installed.
fn settle_library_reload(app: &mut AppModel) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.library_reload.has_pending() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
        app.apply_library_reload_results();
    }
    app.apply_library_reload_results();
}

fn write_entry(dir: &std::path::Path, name: &str, created: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(
        &path,
        format!("+++\nschema_version = 1\n[time]\ncreated_at = \"{created}\"\n+++\n\n{body}\n"),
    )
    .unwrap();
    path
}

mod cache_cases;
mod metadata_cases;
mod reload_cases;
mod search_cases;
mod selection_cases;
mod theme_cases;
mod toast_cases;
