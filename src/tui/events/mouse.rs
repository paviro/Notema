use crate::AppResult;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::CrosstermBackend, layout::Rect};
use std::io;

use crate::tui::{
    app::{App, Focus, Mode, entry_view_is_available, inline_entry_view_is_visible},
    events::actions::view_selected,
    render,
};

use super::action::Action;

pub(crate) fn handle_mouse(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    mouse: MouseEvent,
) -> AppResult<bool> {
    let size = terminal.size()?;
    let area = Rect::new(0, 0, size.width, size.height);

    if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
        if app.has_overlay() {
            handle_dialog_hint_click(terminal, app, mouse, area)?;
            return Ok(false);
        }
        let layout = render::tui_layout(area, app);
        if render::point_in_rect(layout.footer, mouse.column, mouse.row) {
            if let Some(action) = footer_click_to_action(app, mouse, layout) {
                return super::dispatch_action(terminal, app, action);
            }
            return Ok(false);
        }
    }

    handle_mouse_in_area(app, mouse, area)?;
    Ok(false)
}

pub(super) fn handle_mouse_in_area(app: &mut App, mouse: MouseEvent, area: Rect) -> AppResult<()> {
    if app.has_overlay() {
        return Ok(());
    }

    app.normalize_focus(entry_view_is_available(area.width));
    let layout = render::tui_layout(area, app);

    if app.entry_view_expanded {
        match mouse.kind {
            MouseEventKind::ScrollUp => app.scroll_entry_view(-1),
            MouseEventKind::ScrollDown => app.scroll_entry_view(1),
            _ => {}
        }
        return Ok(());
    }

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => handle_left_click(app, mouse, layout)?,
        MouseEventKind::ScrollUp => handle_wheel(app, mouse, layout, -1),
        MouseEventKind::ScrollDown => handle_wheel(app, mouse, layout, 1),
        _ => {}
    }

    Ok(())
}

fn handle_left_click(app: &mut App, mouse: MouseEvent, layout: render::TuiLayout) -> AppResult<()> {
    if app.mode == Mode::Browse
        && let Some(area) = layout.journals
        && render::point_in_rect(area.area, mouse.column, mouse.row)
    {
        app.focus = if layout.single_panel {
            Focus::Entries
        } else {
            Focus::Journals
        };
        if let Some(index) = render::journal_index_at(
            area,
            mouse.column,
            mouse.row,
            app.scroll.journal,
            app.journals.len(),
        ) {
            app.select_journal(index);
        }
        return Ok(());
    }

    if let Some(area) = layout.entries
        && render::point_in_rect(area.panel.area, mouse.column, mouse.row)
    {
        app.focus = Focus::Entries;
        let rows = render::entry_row_metadata(app, area.text_width);
        if let Some(index) =
            render::entry_index_at(area, mouse.column, mouse.row, app.scroll.entry, &rows)
        {
            app.select_entry_index(index);
            if !inline_entry_view_is_visible(layout.content.width) {
                view_selected(app)?;
            }
        }
        return Ok(());
    }

    if let Some(area) = layout.entry_view
        && render::point_in_rect(area.area, mouse.column, mouse.row)
        && app.has_selected_entry_target()
    {
        let tags = app.selected_entry_tags();
        let feelings = app.selected_entry_feelings();
        let mood = app.selected_entry_mood();
        if let Some(feeling) =
            render::feeling_at_point(area.area, mouse.column, mouse.row, &tags, &feelings, mood)
        {
            app.begin_feeling_search(&feeling);
            return Ok(());
        }
        if let Some(tag) =
            render::tag_at_point(area.area, mouse.column, mouse.row, &tags, &feelings, mood)
        {
            app.begin_tag_search(&tag);
            return Ok(());
        }
        app.focus = Focus::EntryView;
    }

    Ok(())
}

fn handle_wheel(app: &mut App, mouse: MouseEvent, layout: render::TuiLayout, delta: i16) {
    if let Some(area) = layout.entry_view
        && render::point_in_rect(area.area, mouse.column, mouse.row)
    {
        app.focus = Focus::EntryView;
        app.scroll_entry_view(delta);
        return;
    }

    if let Some(area) = layout.entries
        && render::point_in_rect(area.panel.area, mouse.column, mouse.row)
    {
        let rows = render::entry_row_metadata(app, area.text_width);
        app.scroll.entry = render::scroll_offset(
            app.scroll.entry,
            delta,
            render::total_entry_row_height(&rows),
            area.viewport_height,
        );
        return;
    }

    if app.mode == Mode::Browse
        && let Some(area) = layout.journals
        && render::point_in_rect(area.area, mouse.column, mouse.row)
    {
        app.scroll.journal = render::scroll_offset(
            app.scroll.journal,
            delta,
            app.journals.len(),
            area.content.height,
        );
    }
}

// ── Footer click ──────────────────────────────────────────────────────────────

fn footer_click_to_action(
    app: &App,
    mouse: MouseEvent,
    layout: render::TuiLayout,
) -> Option<Action> {
    let hint_id = if app.entry_view_expanded {
        render::expanded_footer_hint_id_at(layout.footer.x, mouse.column)
    } else {
        render::footer_hint_id_at(app, layout.footer.x, mouse.column)
    };

    hint_id.and_then(|id| hint_id_to_action(app, id))
}

// ── Dialog hint click routing ─────────────────────────────────────────────────

fn handle_dialog_hint_click(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    mouse: MouseEvent,
    area: Rect,
) -> AppResult<()> {
    let col = mouse.column;
    let row = mouse.row;

    if let Some(focus) = app.edit_tag_state().map(|s| s.focus) {
        let filtered_len = app.edit_tag_state().map_or(0, |s| s.filtered.len());
        let dialog = render::tags_dialog_area(area, filtered_len);
        let inner = render::panel_inner(dialog);
        if row == inner.y + inner.height.saturating_sub(1)
            && let Some(id) = render::hint_id_at(render::tags_dialog_hints(focus), inner.x + 1, col)
            && let Some(action) = hint_id_to_action(app, id)
        {
            super::dispatch_action(terminal, app, action)?;
        }
        return Ok(());
    }

    if app.edit_feeling_state().is_some() {
        let all_len = app.edit_feeling_state().map_or(0, |s| s.all_feelings.len());
        let dialog = render::feelings_dialog_area(area, all_len);
        let inner = render::panel_inner(dialog);
        if row == inner.y + inner.height.saturating_sub(1)
            && let Some(id) = render::hint_id_at(render::feelings_dialog_hints(), inner.x + 1, col)
            && let Some(action) = hint_id_to_action(app, id)
        {
            super::dispatch_action(terminal, app, action)?;
        }
        return Ok(());
    }

    if app.edit_mood_state().is_some() {
        let dialog = render::mood_dialog_area(area);
        let inner = render::panel_inner(dialog);
        if row == inner.y + inner.height.saturating_sub(1)
            && let Some(id) = render::hint_id_at(render::mood_dialog_hints(), inner.x + 1, col)
            && let Some(action) = hint_id_to_action(app, id)
        {
            super::dispatch_action(terminal, app, action)?;
        }
    }

    Ok(())
}

/// Pure: maps a typed hint id to an Action.
pub(super) fn hint_id_to_action(app: &App, id: render::HintId) -> Option<Action> {
    match id {
        render::HintId::NewJournal => Some(Action::NewJournal),
        render::HintId::NewEntry => Some(Action::NewEntry),
        render::HintId::Refresh => Some(Action::Refresh),
        render::HintId::BeginSearch => Some(Action::BeginSearch),
        render::HintId::Quit => Some(Action::Quit),
        render::HintId::EditSelected if app.can_act_on_selected_entry() => {
            Some(Action::EditSelected)
        }
        render::HintId::ViewSelected if app.has_selected_entry_target() => {
            Some(Action::ViewSelected)
        }
        render::HintId::BeginDelete if app.has_selected_entry_target() => Some(Action::BeginDelete),
        render::HintId::BeginEditTags if app.has_selected_entry_target() => {
            Some(Action::BeginEditTags)
        }
        render::HintId::BeginEditFeelings if app.has_selected_entry_target() => {
            Some(Action::BeginEditFeelings)
        }
        render::HintId::BeginEditMood if app.has_selected_entry_target() => {
            Some(Action::BeginEditMood)
        }
        render::HintId::ExitSearch => Some(Action::ExitSearch),
        render::HintId::CancelOverlay => Some(Action::CancelOverlay),
        render::HintId::TagsToggle
            if app
                .edit_tag_state()
                .is_some_and(|state| !state.filtered.is_empty()) =>
        {
            Some(Action::TagsToggle)
        }
        render::HintId::TagsSwitchFocus => Some(Action::TagsSwitchFocus),
        render::HintId::TagsAddFromInput => Some(Action::TagsAddFromInput),
        render::HintId::TagsSave => Some(Action::TagsSave),
        render::HintId::FeelingsToggle => Some(Action::FeelingsToggle),
        render::HintId::FeelingsSave => Some(Action::FeelingsSave),
        render::HintId::MoodDecrease => Some(Action::MoodDecrease),
        render::HintId::MoodIncrease => Some(Action::MoodIncrease),
        render::HintId::MoodSave => Some(Action::MoodSave),
        render::HintId::MoodClear => Some(Action::MoodClear),
        _ => None,
    }
}
