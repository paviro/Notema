use crate::{
    AppResult,
    markdown::split_front_matter,
    storage::{create_entry, create_journal, move_entry_to_trash, open_editor, set_updated_at_now},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{fs, io};

use super::terminal::suspend_terminal;
use crate::tui::app::{App, MarkdownView};

pub(super) fn edit_viewer_entry(
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

pub(super) fn submit_new_journal(app: &mut App) -> AppResult<()> {
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

pub(super) fn create_entry_in_selected_journal(
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

pub(super) fn edit_selected(
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

pub(super) fn view_selected(app: &mut App) -> AppResult<()> {
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

pub(super) fn delete_selected(app: &mut App) -> AppResult<()> {
    let Some(target) = app.selected_entry_target() else {
        return Ok(());
    };
    move_entry_to_trash(&app.config.journal_root, &target.path)?;

    app.set_status("Moved to trash");
    Ok(())
}
