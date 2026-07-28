use super::*;
use crate::{
    config::Config,
    tui::{
        app::{AppModel, Focus, ScrollbarDrag},
        features::{
            feelings::FeelingRow,
            insights::{InsightsTab, InsightsTimeframe},
            location::LocationPreset,
            metadata::EditMetadataFocus,
        },
        render, scroll,
        state::{HoverTarget, ListNav},
        test_support::{app_reading, app_with_entries, app_with_entry, app_with_journals, new_app},
    },
};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use notema_storage::JournalStore;
use ratatui::layout::Rect;
use std::fs;
use tempfile::tempdir;

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::empty(),
    }
}

fn down() -> MouseEventKind {
    MouseEventKind::Down(MouseButton::Left)
}

fn drag() -> MouseEventKind {
    MouseEventKind::Drag(MouseButton::Left)
}

fn up() -> MouseEventKind {
    MouseEventKind::Up(MouseButton::Left)
}

/// Render a frame into a fresh `ViewState`, returning it with the terminal so
/// callers can drive the production translation/dispatch paths against the
/// regions that render registered.
fn render_view(
    app: &mut AppModel,
    w: u16,
    h: u16,
) -> (
    ratatui::Terminal<ratatui::backend::TestBackend>,
    crate::tui::ui::ViewState,
) {
    let backend = ratatui::backend::TestBackend::new(w, h);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    let mut view = crate::tui::ui::ViewState::default();
    let theme = app.appearance.theme.clone();
    terminal
        .draw(|frame| {
            let mut context = crate::tui::ui::RenderContext::new(&theme, &mut view);
            crate::tui::render::draw(frame, app, &mut context);
        })
        .unwrap();
    (terminal, view)
}

/// Render a frame and dispatch the `ViewRendered` the run loop would, so tests
/// exercise the real render-to-model handshake that link-hint labels ride on.
fn render_and_sync(
    app: &mut AppModel,
    w: u16,
    h: u16,
) -> ratatui::Terminal<ratatui::backend::TestBackend> {
    let (mut terminal, mut view) = render_view(app, w, h);
    dispatch_action(
        &mut terminal,
        app,
        Action::ViewRendered {
            reader_scroll: (view.reader.line_count > 0).then_some(view.reader.scroll),
            insights_scroll: (view.insights.total > 0).then_some(view.insights.scroll),
            journal_offset: view.journal_offset,
            entry_offset: view.entry_offset,
            reader_hints: std::mem::take(&mut view.reader.hints),
            reader_openable: view.reader.openable,
        },
    )
    .unwrap();
    terminal
}

fn mouse_in_area(app: &mut AppModel, event: MouseEvent, w: u16, h: u16) {
    let (mut terminal, view) = render_view(app, w, h);
    if let Some(action) = mouse::mouse_to_action(app, event, Rect::new(0, 0, w, h), &view, false) {
        dispatch_action(&mut terminal, app, action).unwrap();
    }
}

/// The first cell whose topmost registered interaction region satisfies
/// `predicate`, scanning the frame row-major.
fn find_interaction(
    view: &crate::tui::ui::ViewState,
    w: u16,
    h: u16,
    predicate: impl Fn(&crate::tui::ui::InteractionKind) -> bool,
) -> Option<(u16, u16)> {
    (0..h)
        .flat_map(|row| (0..w).map(move |col| (col, row)))
        .find(|(col, row)| view.interactions.hit(*col, *row).is_some_and(&predicate))
}

/// Render a frame, then drive the production hover path at `(col, row)`.
/// Returns whether the hover target changed — the run loop's repaint signal.
fn apply_hover(app: &mut AppModel, col: u16, row: u16, area: Rect) -> bool {
    let (mut terminal, view) = render_view(app, area.width, area.height);
    mouse::update_hover(&mut terminal, app, col, row, area, &view).unwrap()
}

/// A search box offering `count` tag values, typed into as the event loop would.
/// Past the popup's eight rows the list scrolls, which is what the wheel, the
/// thumb and the arrow keys are all measured against.
fn app_offering_suggestions(count: usize) -> AppModel {
    let mut app = crate::tui::test_support::app_in_temp(|root| {
        let dir = root.join("work").join("2026-07-01");
        fs::create_dir_all(&dir).unwrap();
        for index in 0..count {
            fs::write(
                dir.join(format!("{index}.md")),
                format!(
                    "+++\nschema_version = 1\n\n[entry]\ntags = [\"tag-{index:02}\"]\n\n[time]\ncreated_at = \"2026-07-01T10:{index:02}:00+02:00\"\n+++\n\nbody\n"
                ),
            )
            .unwrap();
        }
    });
    app.begin_search();
    for ch in "tags:tag".chars() {
        app.search_input_key(key(KeyCode::Char(ch)));
    }
    assert_eq!(app.search.suggestions.rows.len(), count);
    app
}

fn set_tag_dialog_items(app: &mut AppModel, count: usize) {
    let state = app.edit_metadata_state_mut().unwrap();
    state.all_values = (0..count)
        .map(|index| (format!("tag-{index:02}"), index + 1))
        .collect();
    state.filtered = (0..count).collect();
    state.normalize_list_state();
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::empty())
}

mod keyboard_cases;
mod mouse_cases;
mod overlay_cases;
mod paste_cases;
