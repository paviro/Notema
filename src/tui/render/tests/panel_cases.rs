//! Panel titles and counts, and the compact, two-column and fullscreen renders.

use super::*;

#[test]
fn list_panels_show_counts_in_bottom_titles() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let work_entry_dir = root.join("work").join("2026-07-01");
    fs::create_dir_all(&work_entry_dir).unwrap();
    fs::write(
        work_entry_dir.join("a.md"),
        "+++\nschema_version = 1\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n+++\n\n# A\nBody\n",
    )
    .unwrap();
    fs::write(
        work_entry_dir.join("b.md"),
        "+++\nschema_version = 1\n[time]\ncreated_at = \"2026-07-01T11:00:00+02:00\"\n+++\n\n# B\nBody\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("personal")).unwrap();

    let config = Config::new(root);
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    app.nav.focus = Focus::Entries;

    let rendered = render_text(app, 130, 20);

    assert!(rendered.contains("2 journals"));
    assert!(rendered.contains("2 entries"));
}

#[test]
fn compact_render_shows_only_the_active_step() {
    let mut journals_app = app_with_entry();
    journals_app.nav.focus = Focus::Journals;
    let journals = render_text(journals_app, 57, 16);
    assert!(journals.contains(" Journals "));
    assert!(!journals.contains(" Entries "));
    assert!(!journals.contains("2026-07-01 10:00"));

    let mut entries_app = app_with_entry();
    entries_app.nav.focus = Focus::Entries;
    let entries = render_text(entries_app, 57, 16);
    assert!(entries.contains(" Entries "));
    assert!(!entries.contains(" Journals "));
    assert!(!entries.contains("2026-07-01 10:00"));

    let mut reader_focus_app = app_with_entry();
    reader_focus_app.nav.focus = Focus::Reader;
    let reader_focus = render_text(reader_focus_app, 57, 16);
    assert!(!reader_focus.contains(" Entries "));
    assert!(!reader_focus.contains(" Journals "));
    assert!(reader_focus.contains("Body"));
}

#[test]
fn two_column_render_follows_active_column_pair() {
    let mut journals_app = app_with_entry();
    journals_app.nav.focus = Focus::Journals;
    let journals = render_text(journals_app, 90, 16);
    assert!(journals.contains(" Journals "));
    assert!(journals.contains(" Entries "));
    assert!(!journals.contains("2026-07-01 10:00"));

    let mut entries_app = app_with_entry();
    entries_app.nav.focus = Focus::Entries;
    let entries = render_text(entries_app, 90, 16);
    assert!(entries.contains(" Entries "));
    assert!(!entries.contains(" Journals "));
    assert!(entries.contains("Wednesday, 1 July 2026, 10:00"));
}

#[test]
fn selected_journal_and_entry_remain_reversed_when_reader_is_focused() {
    let mut app = app_with_entry();
    app.nav.focus = Focus::Reader;

    let backend = render_app(app, 130, 20);
    let buffer = backend.buffer();

    // Journal box 0 spans rows 2-4 (after the leading offset); its inside is
    // reversed while selected.
    assert!(
        buffer
            .cell((2, 3))
            .unwrap()
            .modifier
            .contains(Modifier::REVERSED)
    );
    assert!(
        buffer
            .cell((24, 3))
            .unwrap()
            .modifier
            .contains(Modifier::REVERSED)
    );
}

#[test]
fn multi_col_fullscreen_takes_the_whole_width_and_hides_columns() {
    let mut app = app_with_entry();
    app.nav.focus = Focus::Reader;
    app.nav.reader_fullscreen = true;

    let layout = tui_layout(Rect::new(0, 0, 130, 20), &app);
    assert!(layout.journals.is_none());
    assert!(layout.entries.is_none());
    assert_eq!(layout.reader.unwrap().area.width, 130);

    let text = render_text(app, 130, 20);
    assert!(!text.contains(" Journals "));
    assert!(!text.contains(" Entries "));
    assert!(text.contains("Body"));
}

#[test]
fn selected_entry_is_not_reversed_when_journals_are_focused() {
    let mut app = app_with_entry();
    app.nav.focus = Focus::Journals;

    let backend = render_app(app, 120, 20);
    let buffer = backend.buffer();

    // The selected journal box (rows 2-4) is reversed, but no entry in the
    // entries column is, since journals hold focus.
    assert!(
        buffer
            .cell((2, 3))
            .unwrap()
            .modifier
            .contains(Modifier::REVERSED)
    );
    assert!(
        !buffer
            .cell((29, 3))
            .unwrap()
            .modifier
            .contains(Modifier::REVERSED)
    );
}
