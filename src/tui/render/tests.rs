use super::*;
use crate::tui::features::insights::InsightsTab;
use crate::{
    config::Config,
    tui::{
        app::{Focus, INLINE_READER_MIN_WIDTH, Mode},
        features::metadata::{EditMetadataFocus, EditMetadataState},
        state::MetadataKind,
        test_support::{app_reading, app_with_entries, app_with_entry, app_with_journals, new_app},
        theme,
    },
};
use notema_domain::{Entry, EntryEncryptionState, SearchHit};
use ratatui::{Frame, Terminal, backend::TestBackend, layout::Rect, style::Modifier, text::Line};
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

/// Draw `draw` onto a fresh `width`×`height` test terminal and return the
/// backend, the shared plumbing behind the typed render helpers below.
fn render_backend(width: u16, height: u16, draw: impl FnOnce(&mut Frame)) -> TestBackend {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(draw).unwrap();
    terminal.backend().clone()
}

/// The rendered buffer as one flat string (every cell symbol, row by row).
fn render_to_text(width: u16, height: u16, draw: impl FnOnce(&mut Frame)) -> String {
    render_backend(width, height, draw)
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

/// The rendered buffer split into one string per row.
fn render_to_rows(width: u16, height: u16, draw: impl FnOnce(&mut Frame)) -> Vec<String> {
    render_backend(width, height, draw)
        .buffer()
        .content()
        .chunks(width as usize)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect())
        .collect()
}

fn draw_app(frame: &mut Frame<'_>, app: &mut AppModel, view: &mut crate::tui::ui::ViewState) {
    let active_theme = app.appearance.theme.clone();
    let mut context = crate::tui::ui::RenderContext::new(&active_theme, view);
    draw(frame, app, &mut context);
}

fn render_text(mut app: AppModel, width: u16, height: u16) -> String {
    let mut view = crate::tui::ui::ViewState::default();
    render_to_text(width, height, |frame| draw_app(frame, &mut app, &mut view))
}

fn render_app(mut app: AppModel, width: u16, height: u16) -> TestBackend {
    let mut view = crate::tui::ui::ViewState::default();
    render_backend(width, height, |frame| draw_app(frame, &mut app, &mut view))
}

fn render_edit_tags_dialog_text_with_theme(
    theme: &theme::Theme,
    mut state: EditMetadataState,
    width: u16,
    height: u16,
) -> String {
    render_to_text(width, height, |frame| {
        dialogs::draw_edit_metadata_dialog(
            theme,
            frame,
            &mut state,
            crate::tui::state::HoverTarget::None,
        )
    })
}

/// An `AppModel` with a `work` journal holding one entry carrying mood, feelings, and
/// a person, with `work` selected and the Journals column focused (so the tabbed
/// insights panel is the visible right pane).
fn app_with_metadata_entry() -> AppModel {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n\n[entry]\ntags = [\"work\"]\nfeelings = [\"calm\"]\npeople = [\"alex\"]\nactivities = [\"running\"]\nmood = 3\n\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n+++\n\n# A\nBody\n",
    )
    .unwrap();
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    std::mem::forget(dir);
    app
}

/// Put `app` into the state where the insights panel is the visible, focused
/// right pane: browsing with the panel focused and no entry selected.
fn focus_insights(app: &mut AppModel, tab: InsightsTab) {
    app.nav.selected_entry_index = None;
    app.nav.focus = Focus::Insights;
    app.nav.insights_tab = tab;
}

fn rendered_lines(lines: &[Line<'static>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

fn render_pending_notice_text_with_theme(
    theme: &theme::Theme,
    device_name: &str,
    notice: &AccessNotice,
) -> String {
    render_to_text(72, 20, |frame| {
        draw_pending_notice(theme, frame, device_name, notice)
    })
}

mod chip_cases;
mod dialog_cases;
mod editor_cases;
mod entries_cases;
mod flat_chrome_cases;
mod footer_cases;
mod insights_cases;
mod layout_cases;
mod menus_cases;
mod panel_cases;
mod reader_cases;
mod scrollbar_cases;
mod search_cases;
mod unlock_cases;
