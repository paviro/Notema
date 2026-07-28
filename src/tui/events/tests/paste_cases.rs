//! Bracketed-paste routing: a pasted block lands in the caret's text sink in one
//! edit, and single-line fields fold newlines instead of splitting.

use super::*;

/// A `TestBackend` terminal to drive `handle_paste`, which only needs it for the
/// generic `dispatch_action` path.
fn test_terminal() -> ratatui::Terminal<ratatui::backend::TestBackend> {
    ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap()
}

#[test]
fn paste_into_editor_inserts_a_multiline_block() {
    let mut app = app_with_journals(&["work"]);
    app.open_editor_for_new();
    let mut terminal = test_terminal();

    handle_paste(&mut terminal, &mut app, "line one\nline two".to_string()).unwrap();

    // A real block insert keeps the newline (the editor is multi-line); replaying
    // it as key events never would, since Enter isn't part of pasted text.
    assert_eq!(app.editor.as_ref().unwrap().text(), "line one\nline two");
}

#[test]
fn paste_into_editor_splits_carriage_return_lines() {
    let mut app = app_with_journals(&["work"]);
    app.open_editor_for_new();
    let mut terminal = test_terminal();

    // iTerm2 and Terminal.app convert every newline to `\r` when sending a pasted
    // block, and the textarea only splits on `\n`.
    handle_paste(
        &mut terminal,
        &mut app,
        "line one\rline two\r\nline three".to_string(),
    )
    .unwrap();

    let editor = app.editor.as_ref().unwrap();
    assert_eq!(editor.text(), "line one\nline two\nline three");
    assert_eq!(editor.textarea.lines().len(), 3);
}

#[test]
fn paste_into_editor_drops_escape_sequences() {
    let mut app = app_with_journals(&["work"]);
    app.open_editor_for_new();
    let mut terminal = test_terminal();

    // The renderer writes line content to the terminal verbatim, so a pasted
    // escape would be executed rather than shown.
    handle_paste(&mut terminal, &mut app, "a\x1b[31mb".to_string()).unwrap();

    assert_eq!(app.editor.as_ref().unwrap().text(), "a[31mb");
}

#[test]
fn a_control_only_paste_leaves_the_editor_untouched() {
    let mut app = app_with_journals(&["work"]);
    app.open_editor_for_new();
    let mut terminal = test_terminal();

    handle_paste(&mut terminal, &mut app, "\x1b\x07".to_string()).unwrap();

    let editor = app.editor.as_mut().unwrap();
    assert_eq!(editor.text(), "");
    assert!(!editor.textarea.undo(), "nothing was pushed to history");
}

#[test]
fn paste_into_search_field_folds_newlines_onto_one_line() {
    let mut app = app_with_entries(1);
    app.begin_search();
    let mut terminal = test_terminal();

    handle_paste(&mut terminal, &mut app, "hello\r\nworld".to_string()).unwrap();

    // One space, not one per control char.
    assert_eq!(app.search.query.as_str(), "hello world");
}

#[test]
fn paste_with_no_focused_field_is_inert() {
    let mut app = app_with_entries(1);
    // Browse mode, no editor, no overlay: nothing owns the caret.
    let mut terminal = test_terminal();

    handle_paste(&mut terminal, &mut app, "ignored".to_string()).unwrap();

    assert!(app.editor.is_none());
    assert!(app.search.query.is_empty());
}
