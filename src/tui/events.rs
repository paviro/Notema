use crate::{
    AppResult,
    markdown::split_front_matter,
    storage::{create_entry, create_journal, move_entry_to_trash, open_editor, set_updated_at_now},
};
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEvent, MouseButton, MouseEvent,
        MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend, layout::Rect};
use std::{fs, io};

use super::{
    app::{App, Focus, MarkdownView, Mode, inline_entry_view_is_visible},
    render,
};

pub(crate) fn handle_key(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    key: KeyEvent,
) -> AppResult<bool> {
    let width = terminal.size()?.width;
    let inline_entry_view_visible = inline_entry_view_is_visible(width);
    app.normalize_focus(inline_entry_view_visible);

    if app.viewer.is_some() {
        handle_viewer_key(terminal, app, key, inline_entry_view_visible)?;
        return Ok(false);
    }

    if app.new_journal_input.is_some() {
        handle_new_journal_input(app, key)?;
        return Ok(false);
    }

    if app.confirm_delete {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                delete_selected(app)?;
                app.confirm_delete = false;
                app.refresh()?;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => app.confirm_delete = false,
            _ => {}
        }
        return Ok(false);
    }

    if app.mode == Mode::Search {
        handle_search_key(terminal, app, key, inline_entry_view_visible)?;
        return Ok(false);
    }

    match key.code {
        KeyCode::Char('q') => return Ok(true),
        KeyCode::Char('r') => app.refresh()?,
        KeyCode::Char('/') => app.begin_search(),
        KeyCode::Left => move_focus_left(app),
        KeyCode::Right => handle_right(app, inline_entry_view_visible)?,
        KeyCode::Enter => handle_enter(app, inline_entry_view_visible)?,
        KeyCode::Up if app.focus == Focus::EntryView => app.scroll_entry_view(-1),
        KeyCode::Down if app.focus == Focus::EntryView => app.scroll_entry_view(1),
        KeyCode::Char('k') if app.focus == Focus::EntryView => app.scroll_entry_view(-1),
        KeyCode::Char('j') if app.focus == Focus::EntryView => app.scroll_entry_view(1),
        KeyCode::PageUp if app.focus == Focus::EntryView => app.page_entry_view(-1),
        KeyCode::PageDown if app.focus == Focus::EntryView => app.page_entry_view(1),
        KeyCode::Home if app.focus == Focus::EntryView => app.entry_view_scroll = 0,
        KeyCode::End if app.focus == Focus::EntryView => app.entry_view_scroll = u16::MAX,
        KeyCode::Up => {
            app.move_selection(-1);
            keep_selection_visible(terminal, app)?;
        }
        KeyCode::Down => {
            app.move_selection(1);
            keep_selection_visible(terminal, app)?;
        }
        KeyCode::Char('e') if app.can_act_on_selected_entry() => edit_selected(terminal, app)?,
        KeyCode::Char('v') if app.can_act_on_selected_entry() => view_selected(app)?,
        KeyCode::Char('n') => create_entry_in_selected_journal(terminal, app)?,
        KeyCode::Char('j') | KeyCode::Char('J') => app.begin_new_journal_input(),
        KeyCode::Char('d') if app.can_act_on_selected_entry() => app.confirm_delete = true,
        _ => {}
    }

    Ok(false)
}

fn handle_search_key(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    key: KeyEvent,
    inline_entry_view_visible: bool,
) -> AppResult<()> {
    match key.code {
        KeyCode::Esc => app.exit_search(),
        KeyCode::Left if app.focus == Focus::EntryView => app.focus = Focus::Entries,
        KeyCode::Right
            if app.focus == Focus::Entries
                && !inline_entry_view_visible
                && app.has_selected_entry_target() =>
        {
            view_selected(app)?
        }
        KeyCode::Right if app.focus == Focus::Entries && inline_entry_view_visible => {
            app.focus = Focus::EntryView;
        }
        KeyCode::Up if app.focus == Focus::EntryView => app.scroll_entry_view(-1),
        KeyCode::Down if app.focus == Focus::EntryView => app.scroll_entry_view(1),
        KeyCode::Char('k') if app.focus == Focus::EntryView => app.scroll_entry_view(-1),
        KeyCode::Char('j') if app.focus == Focus::EntryView => app.scroll_entry_view(1),
        KeyCode::PageUp if app.focus == Focus::EntryView => app.page_entry_view(-1),
        KeyCode::PageDown if app.focus == Focus::EntryView => app.page_entry_view(1),
        KeyCode::Home if app.focus == Focus::EntryView => app.entry_view_scroll = 0,
        KeyCode::End if app.focus == Focus::EntryView => app.entry_view_scroll = u16::MAX,
        KeyCode::Enter if app.can_act_on_selected_entry() => view_selected(app)?,
        KeyCode::Char('e') if app.focus == Focus::EntryView && app.has_selected_entry_target() => {
            edit_selected(terminal, app)?
        }
        KeyCode::Char('v') if app.focus == Focus::EntryView && app.has_selected_entry_target() => {
            view_selected(app)?
        }
        KeyCode::Char('d') if app.focus == Focus::EntryView && app.has_selected_entry_target() => {
            app.confirm_delete = true
        }
        KeyCode::Backspace if app.focus == Focus::Entries => {
            app.search_query.pop();
            app.update_search_results()?;
        }
        KeyCode::Char(ch) if app.focus == Focus::Entries => {
            app.search_query.push(ch);
            app.update_search_results()?;
        }
        KeyCode::Up => {
            app.move_selection(-1);
            keep_selection_visible(terminal, app)?;
        }
        KeyCode::Down => {
            app.move_selection(1);
            keep_selection_visible(terminal, app)?;
        }
        _ => {}
    }

    Ok(())
}

pub(crate) fn handle_mouse(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    mouse: MouseEvent,
) -> AppResult<()> {
    let size = terminal.size()?;
    let area = Rect::new(0, 0, size.width, size.height);
    handle_mouse_in_area(app, mouse, area)
}

fn handle_mouse_in_area(app: &mut App, mouse: MouseEvent, area: Rect) -> AppResult<()> {
    if app.new_journal_input.is_some() || app.confirm_delete {
        return Ok(());
    }

    app.normalize_focus(render::tui_layout(area, app).inline_entry_view_visible);
    let layout = render::tui_layout(area, app);

    if app.viewer.is_some() {
        match mouse.kind {
            MouseEventKind::ScrollUp => scroll_viewer(app, -1),
            MouseEventKind::ScrollDown => scroll_viewer(app, 1),
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
        && render::point_in_rect(area, mouse.column, mouse.row)
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
            app.journal_scroll,
            app.journals.len(),
        ) {
            app.select_journal(index);
        }
        return Ok(());
    }

    if let Some(area) = layout.entries
        && render::point_in_rect(area, mouse.column, mouse.row)
    {
        app.focus = Focus::Entries;
        let rows = render::entry_row_metadata(app);
        if let Some(index) =
            render::entry_index_at(area, mouse.column, mouse.row, app.entry_scroll, &rows)
        {
            app.select_entry_index(index);
            if !layout.inline_entry_view_visible {
                view_selected(app)?;
            }
        }
        return Ok(());
    }

    if let Some(area) = layout.entry_view
        && render::point_in_rect(area, mouse.column, mouse.row)
        && app.has_selected_entry_target()
    {
        app.focus = Focus::EntryView;
    }

    Ok(())
}

fn handle_wheel(app: &mut App, mouse: MouseEvent, layout: render::TuiLayout, delta: i16) {
    if let Some(area) = layout.entry_view
        && render::point_in_rect(area, mouse.column, mouse.row)
    {
        app.focus = Focus::EntryView;
        app.scroll_entry_view(delta);
        return;
    }

    if let Some(area) = layout.entries
        && render::point_in_rect(area, mouse.column, mouse.row)
    {
        let rows = render::entry_row_metadata(app);
        app.entry_scroll = render::scroll_offset(
            app.entry_scroll,
            delta,
            render::total_entry_row_height(&rows),
            render::panel_inner(area).height,
        );
        return;
    }

    if app.mode == Mode::Browse
        && let Some(area) = layout.journals
        && render::point_in_rect(area, mouse.column, mouse.row)
    {
        app.journal_scroll = render::scroll_offset(
            app.journal_scroll,
            delta,
            app.journals.len(),
            render::panel_inner(area).height,
        );
    }
}

fn keep_selection_visible(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> AppResult<()> {
    let size = terminal.size()?;
    let layout = render::tui_layout(Rect::new(0, 0, size.width, size.height), app);
    if app.focus == Focus::Journals && app.mode == Mode::Browse {
        if let Some(area) = layout.journals {
            render::ensure_index_visible(
                &mut app.journal_scroll,
                app.selected_journal,
                app.journals.len(),
                render::panel_inner(area).height,
            );
        }
    } else if let Some(area) = layout.entries {
        let rows = render::entry_row_metadata(app);
        render::ensure_entry_visible(
            &mut app.entry_scroll,
            &rows,
            app.selected_entry_index,
            render::panel_inner(area).height,
        );
    }

    Ok(())
}

fn scroll_viewer(app: &mut App, delta: i16) {
    let Some(viewer) = app.viewer.as_mut() else {
        return;
    };

    if delta.is_negative() {
        viewer.scroll = viewer.scroll.saturating_sub(delta.unsigned_abs());
    } else {
        viewer.scroll = viewer.scroll.saturating_add(delta as u16);
    }
}

fn move_focus_left(app: &mut App) {
    app.focus = match app.focus {
        Focus::EntryView => Focus::Entries,
        Focus::Entries => Focus::Journals,
        Focus::Journals => Focus::Journals,
    };
}

fn handle_right(app: &mut App, inline_entry_view_visible: bool) -> AppResult<()> {
    if app.focus == Focus::Entries && !inline_entry_view_visible && app.has_selected_entry_target()
    {
        view_selected(app)?;
    } else {
        move_focus_right(app, inline_entry_view_visible);
    }

    Ok(())
}

fn move_focus_right(app: &mut App, inline_entry_view_available: bool) {
    app.focus = match app.focus {
        Focus::Journals => Focus::Entries,
        Focus::Entries if inline_entry_view_available => Focus::EntryView,
        Focus::Entries => Focus::Entries,
        Focus::EntryView => Focus::EntryView,
    };
}

fn handle_enter(app: &mut App, inline_entry_view_available: bool) -> AppResult<()> {
    if app.focus == Focus::Journals {
        move_focus_right(app, inline_entry_view_available);
    } else if app.can_act_on_selected_entry() {
        view_selected(app)?;
    }

    Ok(())
}

fn handle_viewer_key(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    key: KeyEvent,
    inline_entry_view_visible: bool,
) -> AppResult<()> {
    if viewer_key_closes(key.code, inline_entry_view_visible) {
        app.viewer = None;
        return Ok(());
    }

    if matches!(key.code, KeyCode::Char('e')) {
        edit_viewer_entry(terminal, app)?;
        return Ok(());
    }

    let Some(viewer) = app.viewer.as_mut() else {
        return Ok(());
    };

    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            viewer.scroll = viewer.scroll.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            viewer.scroll = viewer.scroll.saturating_add(1);
        }
        KeyCode::PageUp => {
            viewer.scroll = viewer.scroll.saturating_sub(10);
        }
        KeyCode::PageDown => {
            viewer.scroll = viewer.scroll.saturating_add(10);
        }
        KeyCode::Home => viewer.scroll = 0,
        KeyCode::End => viewer.scroll = u16::MAX,
        _ => {}
    }

    Ok(())
}

fn viewer_key_closes(key: KeyCode, inline_entry_view_visible: bool) -> bool {
    matches!(key, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q'))
        || (key == KeyCode::Left && !inline_entry_view_visible)
}

fn edit_viewer_entry(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> AppResult<()> {
    let Some(viewer) = app.viewer.as_ref() else {
        return Ok(());
    };

    let path = viewer.path.clone();
    let editor = app.config.editor.clone();
    suspend_terminal(terminal, || open_editor(&editor, &path))?;
    set_updated_at_now(&path)?;
    refresh_viewer(app)?;
    app.refresh()?;
    app.set_status(format!("Edited {}", path.display()));
    Ok(())
}

fn handle_new_journal_input(app: &mut App, key: KeyEvent) -> AppResult<()> {
    match key.code {
        KeyCode::Esc => {
            app.new_journal_input = None;
            app.set_status("Cancelled");
        }
        KeyCode::Enter => submit_new_journal(app)?,
        KeyCode::Backspace => {
            if let Some(input) = app.new_journal_input.as_mut() {
                input.pop();
            }
        }
        KeyCode::Char(ch) => {
            if let Some(input) = app.new_journal_input.as_mut() {
                input.push(ch);
            }
        }
        _ => {}
    }
    Ok(())
}

fn submit_new_journal(app: &mut App) -> AppResult<()> {
    let value = app
        .new_journal_input
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_string();
    if value.is_empty() {
        app.set_status("Nothing added");
        app.new_journal_input = None;
        return Ok(());
    }

    let journal = create_journal(&app.config.journal_root, &value)?;
    app.refresh()?;
    app.select_journal_by_name(&journal.name);
    app.set_status(format!("Created journal {}", journal.name));
    app.new_journal_input = None;
    Ok(())
}

fn create_entry_in_selected_journal(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> AppResult<()> {
    if app.selected_journal().is_some() {
        new_entry(terminal, app)
    } else {
        app.set_status("Create a journal first with j");
        Ok(())
    }
}

fn new_entry(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> AppResult<()> {
    let Some(journal) = app.selected_journal().cloned() else {
        app.set_status("No journal selected");
        return Ok(());
    };

    let root = app.config.journal_root.clone();
    let editor = app.config.editor.clone();
    let journal_name = journal.name;
    suspend_terminal(terminal, || create_entry(&root, &journal_name, &editor))?;
    app.set_status("Entry saved");
    app.refresh()?;
    Ok(())
}

fn edit_selected(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> AppResult<()> {
    let Some(target) = app.selected_entry_target() else {
        return Ok(());
    };

    let editor = app.config.editor.clone();
    suspend_terminal(terminal, || open_editor(&editor, &target.path))?;
    set_updated_at_now(&target.path)?;
    app.set_status(format!("Edited {}", target.path.display()));
    app.refresh()?;
    Ok(())
}

fn view_selected(app: &mut App) -> AppResult<()> {
    let Some(target) = app.selected_entry_target() else {
        return Ok(());
    };

    let title = app
        .selected_entry_view()
        .map(|(title, _)| title)
        .unwrap_or_else(|| target.title.clone());
    let content = fs::read_to_string(&target.path)?;
    let (_, body) = split_front_matter(&content);
    app.viewer = Some(MarkdownView {
        title,
        path: target.path,
        content: body.trim_start().to_string(),
        scroll: 0,
    });
    Ok(())
}

fn refresh_viewer(app: &mut App) -> AppResult<()> {
    let Some(viewer) = app.viewer.as_mut() else {
        return Ok(());
    };

    let content = fs::read_to_string(&viewer.path)?;
    let (_, body) = split_front_matter(&content);
    viewer.content = body.trim_start().to_string();
    viewer.scroll = 0;
    Ok(())
}

fn delete_selected(app: &mut App) -> AppResult<()> {
    let Some(target) = app.selected_entry_target() else {
        return Ok(());
    };
    move_entry_to_trash(&app.config.journal_root, &target.path)?;

    app.set_status("Moved to trash");
    Ok(())
}

fn suspend_terminal<T>(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    action: impl FnOnce() -> AppResult<T>,
) -> AppResult<T> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    let result = action();
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture
    )?;
    enable_raw_mode()?;
    terminal.clear()?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crossterm::event::KeyModifiers;
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

    fn app_with_journals(names: &[&str]) -> App {
        let dir = tempdir().unwrap();
        for name in names {
            fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        let config = Config::new(dir.path().to_path_buf(), "true");
        let app = App::new(config).unwrap();
        std::mem::forget(dir);
        app
    }

    fn app_with_entries(count: usize) -> App {
        let dir = tempdir().unwrap();
        let entry_dir = dir.path().join("work").join("2026-07-01");
        fs::create_dir_all(&entry_dir).unwrap();
        for index in 0..count {
            fs::write(
                entry_dir.join(format!("{index}.md")),
                format!(
                    "---\ncreated_at: \"2026-07-01T10:{index:02}:00+02:00\"\n---\n\n# Entry {index}\nPreview {index}\n"
                ),
            )
            .unwrap();
        }
        let config = Config::new(dir.path().to_path_buf(), "true");
        let mut app = App::new(config).unwrap();
        app.select_journal_by_name("work");
        std::mem::forget(dir);
        app
    }

    #[test]
    fn enter_on_journals_moves_to_entries_like_right_arrow() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("work")).unwrap();
        let config = Config::new(dir.path().to_path_buf(), "true");
        let mut enter_app = App::new(config.clone()).unwrap();
        let mut right_app = App::new(config).unwrap();

        enter_app.focus = Focus::Journals;
        right_app.focus = Focus::Journals;

        handle_enter(&mut enter_app, true).unwrap();
        move_focus_right(&mut right_app, true);

        assert_eq!(enter_app.focus, Focus::Entries);
        assert_eq!(enter_app.focus, right_app.focus);
    }

    #[test]
    fn right_on_entry_opens_viewer_when_inline_entry_view_is_hidden() {
        let dir = tempdir().unwrap();
        let entry_dir = dir.path().join("work").join("2026-07-01");
        fs::create_dir_all(&entry_dir).unwrap();
        fs::write(entry_dir.join("a.md"), "---\ntags: []\n---\n\n# A\nBody\n").unwrap();
        let config = Config::new(dir.path().to_path_buf(), "true");
        let mut app = App::new(config).unwrap();
        app.select_journal_by_name("work");
        app.focus = Focus::Entries;

        handle_right(&mut app, false).unwrap();

        assert!(app.viewer.is_some());
        assert_eq!(app.focus, Focus::Entries);
    }

    #[test]
    fn viewer_title_matches_entry_view_timestamp_title() {
        let dir = tempdir().unwrap();
        let entry_dir = dir.path().join("work").join("2026-07-01");
        fs::create_dir_all(&entry_dir).unwrap();
        fs::write(
            entry_dir.join("a.md"),
            "---\ncreated_at: \"2026-07-01T10:23:00+02:00\"\n---\n\n# A\nBody\n",
        )
        .unwrap();
        let config = Config::new(dir.path().to_path_buf(), "true");
        let mut app = App::new(config).unwrap();
        app.select_journal_by_name("work");
        app.focus = Focus::Entries;

        handle_right(&mut app, false).unwrap();

        assert_eq!(app.viewer.as_ref().unwrap().title, "2026-07-01 10:23");
    }

    #[test]
    fn right_on_entry_focuses_entry_view_when_inline_entry_view_is_visible() {
        let dir = tempdir().unwrap();
        let entry_dir = dir.path().join("work").join("2026-07-01");
        fs::create_dir_all(&entry_dir).unwrap();
        fs::write(entry_dir.join("a.md"), "---\ntags: []\n---\n\n# A\nBody\n").unwrap();
        let config = Config::new(dir.path().to_path_buf(), "true");
        let mut app = App::new(config).unwrap();
        app.select_journal_by_name("work");
        app.focus = Focus::Entries;

        handle_right(&mut app, true).unwrap();

        assert!(app.viewer.is_none());
        assert_eq!(app.focus, Focus::EntryView);
    }

    #[test]
    fn left_closes_viewer_only_when_inline_entry_view_is_hidden() {
        assert!(viewer_key_closes(KeyCode::Left, false));
        assert!(!viewer_key_closes(KeyCode::Left, true));
    }

    #[test]
    fn wide_journal_click_selects_journal_and_keeps_journal_focus() {
        let mut app = app_with_journals(&["alpha", "beta"]);
        app.focus = Focus::Journals;
        app.selected_entry_index = 3;
        app.entry_view_scroll = 10;
        let area = Rect::new(0, 0, 120, 20);
        let layout = render::tui_layout(area, &app);
        let journals = render::panel_inner(layout.journals.unwrap());

        handle_mouse_in_area(
            &mut app,
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                journals.x,
                journals.y + 1,
            ),
            area,
        )
        .unwrap();

        assert_eq!(app.selected_journal, 1);
        assert_eq!(app.selected_entry_index, 0);
        assert_eq!(app.entry_view_scroll, 0);
        assert_eq!(app.focus, Focus::Journals);
    }

    #[test]
    fn compact_journal_click_moves_to_entries() {
        let mut app = app_with_journals(&["work"]);
        app.focus = Focus::Journals;
        let area = Rect::new(0, 0, 57, 20);
        let layout = render::tui_layout(area, &app);
        let journals = render::panel_inner(layout.journals.unwrap());

        handle_mouse_in_area(
            &mut app,
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                journals.x,
                journals.y,
            ),
            area,
        )
        .unwrap();

        assert_eq!(app.selected_journal, 0);
        assert_eq!(app.focus, Focus::Entries);
    }

    #[test]
    fn journal_panel_click_without_row_focuses_journals_without_changing_selection() {
        let mut app = app_with_journals(&["alpha"]);
        app.focus = Focus::Entries;
        let area = Rect::new(0, 0, 120, 20);
        let layout = render::tui_layout(area, &app);
        let journals = render::panel_inner(layout.journals.unwrap());

        handle_mouse_in_area(
            &mut app,
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                journals.x,
                journals.y + 4,
            ),
            area,
        )
        .unwrap();

        assert_eq!(app.selected_journal, 0);
        assert_eq!(app.focus, Focus::Journals);
    }

    #[test]
    fn wheel_over_journals_scrolls_without_changing_selection() {
        let mut app = app_with_journals(&["a", "b", "c", "d", "e", "f", "g"]);
        app.focus = Focus::Entries;
        let area = Rect::new(0, 0, 120, 8);
        let layout = render::tui_layout(area, &app);
        let journals = render::panel_inner(layout.journals.unwrap());

        handle_mouse_in_area(
            &mut app,
            mouse(MouseEventKind::ScrollDown, journals.x, journals.y),
            area,
        )
        .unwrap();

        assert_eq!(app.selected_journal, 0);
        assert_eq!(app.journal_scroll, 1);
        assert_eq!(app.focus, Focus::Entries);
    }

    #[test]
    fn wheel_over_entries_scrolls_without_changing_selection() {
        let mut app = app_with_entries(8);
        app.focus = Focus::Journals;
        let area = Rect::new(0, 0, 80, 8);
        let layout = render::tui_layout(area, &app);
        let entries = render::panel_inner(layout.entries.unwrap());

        handle_mouse_in_area(
            &mut app,
            mouse(MouseEventKind::ScrollDown, entries.x, entries.y),
            area,
        )
        .unwrap();

        assert_eq!(app.selected_entry_index, 0);
        assert_eq!(app.entry_scroll, 1);
        assert_eq!(app.focus, Focus::Journals);
    }

    #[test]
    fn entry_click_selects_row_and_opens_viewer_when_inline_entry_view_is_hidden() {
        let mut app = app_with_entries(2);
        app.focus = Focus::Entries;
        let area = Rect::new(0, 0, 80, 12);
        let layout = render::tui_layout(area, &app);
        let entries = render::panel_inner(layout.entries.unwrap());

        handle_mouse_in_area(
            &mut app,
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                entries.x,
                entries.y + 2,
            ),
            area,
        )
        .unwrap();

        assert_eq!(app.focus, Focus::Entries);
        assert_eq!(app.selected_entry_index, 0);
        assert!(app.viewer.is_some());
    }

    #[test]
    fn entry_panel_click_without_entry_row_focuses_entries_without_opening_viewer() {
        let mut app = app_with_entries(1);
        app.focus = Focus::EntryView;
        let area = Rect::new(0, 0, 120, 12);
        let layout = render::tui_layout(area, &app);
        let entries = render::panel_inner(layout.entries.unwrap());

        handle_mouse_in_area(
            &mut app,
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                entries.x,
                entries.y,
            ),
            area,
        )
        .unwrap();

        assert_eq!(app.focus, Focus::Entries);
        assert_eq!(app.selected_entry_index, 0);
        assert!(app.viewer.is_none());
    }

    #[test]
    fn entry_panel_empty_space_click_focuses_entries_without_opening_viewer() {
        let mut app = app_with_entries(1);
        app.focus = Focus::EntryView;
        let area = Rect::new(0, 0, 120, 12);
        let layout = render::tui_layout(area, &app);
        let entries = render::panel_inner(layout.entries.unwrap());

        handle_mouse_in_area(
            &mut app,
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                entries.x,
                entries.y + 5,
            ),
            area,
        )
        .unwrap();

        assert_eq!(app.focus, Focus::Entries);
        assert_eq!(app.selected_entry_index, 0);
        assert!(app.viewer.is_none());
    }

    #[test]
    fn wheel_over_entry_view_scrolls_entry_view_only() {
        let mut app = app_with_entries(6);
        app.focus = Focus::Entries;
        let area = Rect::new(0, 0, 120, 20);
        let layout = render::tui_layout(area, &app);
        let entry_view = render::panel_inner(layout.entry_view.unwrap());

        handle_mouse_in_area(
            &mut app,
            mouse(MouseEventKind::ScrollDown, entry_view.x, entry_view.y),
            area,
        )
        .unwrap();

        assert_eq!(app.entry_view_scroll, 1);
        assert_eq!(app.entry_scroll, 0);
        assert_eq!(app.selected_entry_index, 0);
        assert_eq!(app.focus, Focus::EntryView);
    }

    #[test]
    fn viewer_wheel_scrolls_and_clicks_do_not_close() {
        let mut app = app_with_entries(1);
        view_selected(&mut app).unwrap();

        handle_mouse_in_area(
            &mut app,
            mouse(MouseEventKind::ScrollDown, 1, 1),
            Rect::new(0, 0, 80, 20),
        )
        .unwrap();
        assert_eq!(app.viewer.as_ref().unwrap().scroll, 1);

        handle_mouse_in_area(
            &mut app,
            mouse(MouseEventKind::Down(MouseButton::Left), 1, 1),
            Rect::new(0, 0, 80, 20),
        )
        .unwrap();
        assert!(app.viewer.is_some());
    }
}
