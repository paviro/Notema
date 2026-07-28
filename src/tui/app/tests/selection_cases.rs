//! What the panes show and act on: click registration, entry selection, focus
//! moves, reader state, and the width thresholds that pick a layout.

use super::*;

#[test]
fn register_left_click_detects_a_double_click_on_the_same_cell() {
    let mut nav = Nav::default();
    // A first press is never a double-click.
    assert!(!nav.register_left_click(10, 4));
    // A second press on the same cell (well within the window) is.
    assert!(nav.register_left_click(10, 4));
    // The match cleared the record, so a third quick press starts fresh — a
    // triple-click isn't two overlapping doubles.
    assert!(!nav.register_left_click(10, 4));
    // A press on a different cell doesn't pair with the previous one.
    assert!(!nav.register_left_click(11, 4));
    // But a second press on that new cell does.
    assert!(nav.register_left_click(11, 4));
}

#[test]
fn changing_selected_entry_resets_reader_scroll() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n+++\n\n# A\n",
    )
    .unwrap();
    fs::write(
        entry_dir.join("b.md"),
        "+++\nschema_version = 1\n+++\n\n# B\n",
    )
    .unwrap();

    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    app.nav.focus = Focus::Entries;
    app.nav.scroll.reader = 20;

    app.move_selection(1);

    assert_eq!(app.nav.scroll.reader, 0);
}

#[test]
fn scrolling_up_past_first_entry_deselects_and_shows_insights() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n+++\n\n# A\n",
    )
    .unwrap();
    fs::write(
        entry_dir.join("b.md"),
        "+++\nschema_version = 1\n+++\n\n# B\n",
    )
    .unwrap();

    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    assert_eq!(app.nav.selected_entry_index, Some(0));

    // Up from the first entry deselects, revealing the journal insights reader.
    app.move_selection(-1);
    assert_eq!(app.nav.selected_entry_index, None);
    assert!(app.show_journal_insights());
    assert!(!app.entries_highlighted());
    assert!(app.selected_entry_target().is_none());

    // Down reselects the first entry.
    app.move_selection(1);
    assert_eq!(app.nav.selected_entry_index, Some(0));
    assert!(!app.show_journal_insights());
}

#[test]
fn focusing_journals_shows_insights_even_with_a_lingering_entry_selection() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n+++\n\n# A\n",
    )
    .unwrap();

    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    app.select_entry_index(0);
    app.nav.focus = Focus::Entries;
    // Focused on the entry, its reader shows and its row is highlighted.
    assert!(!app.show_journal_insights());
    assert!(app.entries_highlighted());

    // Moving focus back to the journal column (e.g. clicking the already-selected
    // journal, or Left from the entry) leaves the selection index untouched, but the
    // right column must revert to insights and the row must lose its highlight — the two
    // never disagree.
    app.nav.focus = Focus::Journals;
    assert_eq!(app.nav.selected_entry_index, Some(0));
    assert!(app.show_journal_insights());
    assert!(!app.entries_highlighted());
}

#[test]
fn focusing_insights_shows_insights_even_with_a_lingering_entry_selection() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n+++\n\n# A\n",
    )
    .unwrap();

    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    app.select_entry_index(0);
    app.nav.focus = Focus::Entries;
    assert!(!app.show_journal_insights());
    assert!(app.entries_highlighted());

    // Clicking the insights column focuses it but leaves the selection index
    // lingering. The insights panel shares the right-hand pane with the entry
    // viewer, so it must show insights and drop the row highlight — not reopen
    // the entry that was just closed.
    app.nav.focus = Focus::Insights;
    assert_eq!(app.nav.selected_entry_index, Some(0));
    assert!(app.show_journal_insights());
    assert!(!app.entries_highlighted());
}

#[test]
fn hidden_journals_launch_focuses_entries_with_insights_reader() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n+++\n\n# A\n",
    )
    .unwrap();

    let config = Config::new(dir.path().to_path_buf());
    let mut state = crate::config::State::default();
    state.ui.show_journals = false;
    let app = new_app_with_state(config, state);

    assert_eq!(app.nav.focus, Focus::Entries);
    assert_eq!(app.nav.selected_entry_index, None);
    assert!(app.show_journal_insights());
}

#[test]
fn selected_reader_title_uses_entry_timestamp() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n[time]\ncreated_at = \"2026-07-01T10:23:00+02:00\"\n+++\n\n# A\nBody\n",
    )
    .unwrap();

    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");

    let (title, content) = app.selected_reader().unwrap();

    assert_eq!(title, "Wednesday, 1 July 2026, 10:23");
    assert_eq!(content, "# A\nBody\n");
}

#[test]
fn search_reader_title_uses_entry_timestamp() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n[time]\ncreated_at = \"2026-07-01T10:23:00+02:00\"\n+++\n\n# A\nneedle\n",
    )
    .unwrap();

    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    app.begin_search();
    app.search.query = "needle".into();
    app.update_search_results();

    let (title, content) = app.selected_reader().unwrap();

    assert_eq!(title, "Wednesday, 1 July 2026, 10:23");
    assert_eq!(content, "# A\nneedle\n");
}

#[test]
fn journal_focus_does_not_make_entry_targets_actionable() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n+++\n\n# A\n",
    )
    .unwrap();

    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");

    app.nav.focus = Focus::Journals;
    assert!(!app.can_act_on_selected_entry());

    app.nav.focus = Focus::Entries;
    assert!(app.can_act_on_selected_entry());
}

#[test]
fn compact_width_uses_single_panel_without_inline_reader() {
    assert!(single_panel_is_active(TWO_PANEL_MIN_WIDTH - 1));
    assert!(!inline_reader_is_visible(TWO_PANEL_MIN_WIDTH - 1));
    assert!(!reader_is_available(TWO_PANEL_MIN_WIDTH - 1));
    assert!(reader_is_available(TWO_PANEL_MIN_WIDTH));
}

#[test]
fn inline_reader_uses_minimum_three_column_width() {
    assert!(!inline_reader_is_visible(INLINE_READER_MIN_WIDTH - 1));
    assert!(inline_reader_is_visible(INLINE_READER_MIN_WIDTH));
}
