//! The journal and entry columns: archived dividers, entry hit-testing across
//! month labels, and the lines an entry or search-hit box draws.

use super::*;

#[test]
fn journal_column_inserts_archived_divider_between_sections() {
    let app = app_with_journals(&["work", "zeta", "old.archived"]);
    let rows = crate::tui::entry_rows::journal_list_rows(&app, 16);
    let meta = crate::tui::entry_rows::rows_meta(&rows);

    // Two active journals, then a non-selectable divider row, then the archived one.
    let indices: Vec<Option<usize>> = meta.iter().map(|m| m.item_index).collect();
    assert_eq!(indices, vec![Some(0), Some(1), None, Some(2)]);

    // The rendered column carries the "Archived" divider and the archived
    // journal's display name (no ".archived" suffix leaks into the UI).
    let text = render_text(app, 120, 24);
    assert!(text.contains("Archived"));
    assert!(!text.contains(".archived"));
    // The panel count includes the archived journal (2 active + 1 archived).
    assert!(text.contains("3 journals"));
}

#[test]
fn journal_column_has_no_divider_without_archived_journals() {
    let app = app_with_journals(&["work", "zeta"]);
    let rows = crate::tui::entry_rows::journal_list_rows(&app, 16);
    let meta = crate::tui::entry_rows::rows_meta(&rows);
    assert!(meta.iter().all(|m| m.item_index.is_some()));
}

#[test]
fn search_hit_box_flags_archived_journal_bottom_right() {
    let rendered = rendered_lines(&entry_box_lines(
        &theme::Theme::terminal_default(),
        Some("Sun 05 Jul 2026"),
        "14:30",
        "hit body",
        Some("personal"),
        Some("Archived"),
        40,
    ));
    let bottom = rendered.last().unwrap();
    // Journal display name on the left, the `Archived` flag on the right.
    assert!(bottom.starts_with("└ personal "));
    assert!(bottom.ends_with("Archived ┘"));
}

#[test]
fn entry_hit_testing_ignores_month_divider_and_maps_boxed_entries() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n+++\n\n# A\nFirst preview\n",
    )
    .unwrap();
    fs::write(
        entry_dir.join("b.md"),
        "+++\nschema_version = 1\n[time]\ncreated_at = \"2026-07-01T11:00:00+02:00\"\n+++\n\n# B\nSecond preview\n",
    )
    .unwrap();
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    let area = EntryListGeometry::new(&theme::Theme::terminal_default(), Rect::new(0, 0, 40, 16));
    // text_width=10 wraps each preview onto 2 lines, so a box is 4 rows tall
    // (top border + 2 preview lines + bottom border). A single month divider
    // row leads the list; the day rides on the first entry's border, and a
    // blank spacer row separates consecutive entries.
    let rows = entry_row_metadata(&app, 10);

    assert_eq!(
        rows,
        vec![
            RowMeta {
                item_index: None,
                height: 1,
            },
            RowMeta {
                item_index: Some(0),
                height: 4,
            },
            RowMeta {
                item_index: None,
                height: 1,
            },
            RowMeta {
                item_index: Some(1),
                height: 4,
            },
        ]
    );
    // Rows: month divider (y 1), entry 0 (y 2-5), spacer (y 6), entry 1 (y 7-10).
    assert_eq!(entry_index_at(area, 2, 1, 0, &rows), None);
    assert_eq!(entry_index_at(area, 2, 2, 0, &rows), Some(0));
    assert_eq!(entry_index_at(area, 2, 5, 0, &rows), Some(0));
    assert_eq!(entry_index_at(area, 2, 6, 0, &rows), None);
    assert_eq!(entry_index_at(area, 2, 7, 0, &rows), Some(1));
    assert_eq!(entry_index_at(area, 2, 10, 0, &rows), Some(1));
}

#[test]
fn first_month_rides_border_and_next_month_takes_over_after_scrolling() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();
    // Two July entries (newest, listed first) over many June entries. The
    // June entries give the list a viewport-full of rows below the June
    // divider so it can actually be scrolled above the top.
    let mut days = vec![("2026-07-02", "2026-07-02T10:00:00+02:00")];
    days.push(("2026-07-01", "2026-07-01T10:00:00+02:00"));
    for day in 1..=10 {
        days.push((
            Box::leak(format!("2026-06-{day:02}").into_boxed_str()),
            Box::leak(format!("2026-06-{day:02}T10:00:00+02:00").into_boxed_str()),
        ));
    }
    for (index, (dir_day, ts)) in days.iter().enumerate() {
        let entry_dir = root.join("work").join(dir_day);
        fs::create_dir_all(&entry_dir).unwrap();
        fs::write(
            entry_dir.join(format!("e{index}.md")),
            format!("+++\nschema_version = 1\n[time]\ncreated_at = \"{ts}\"\n+++\n\n# e{index}\nBody text\n"),
        )
        .unwrap();
    }

    // Before scrolling, the first month (July) already rides the border and
    // its divider is absent from the list body (row 0 is the leading blank).
    let top_unscrolled = render_top_border(app_for(&dir), 57, 12);
    assert!(top_unscrolled.contains("July 2026"), "{top_unscrolled:?}");

    // Scroll far enough that the June divider clears the top; June takes over.
    let mut app = app_for(&dir);
    *app.nav.entry_list.offset_mut() = 100;
    let backend = render_app(app, 57, 12);
    let top = (0..57)
        .map(|x| backend.buffer().cell((x, 0)).unwrap().symbol().to_string())
        .collect::<String>();
    assert!(top.contains("June 2026"), "top border was: {top:?}");
}

/// The sticky label is placed from offsets derived from the built rows, so it has
/// to hand over on exactly the row its divider scrolls past: not one row early
/// (the divider would be pinned and listed at once), not one row late.
#[test]
fn the_border_label_takes_over_on_the_row_its_divider_scrolls_past() {
    let dir = month_boundary_journal();

    // The row the divider is drawn on, measured from the rows themselves rather
    // than from the section offsets under test.
    let app = app_for(&dir);
    let cache = app.entry_rows(entry_text_width(&dir));
    let mut divider_top = 0usize;
    for (row, meta) in cache.rows.iter().zip(&cache.meta) {
        if row.text().contains("June 2026") {
            break;
        }
        divider_top += meta.height as usize;
    }
    // The label takes over once the divider has scrolled strictly above the top.
    let expected = divider_top + 1;

    let mut switched_at = None;
    for offset in 0..expected + 4 {
        let (border, body) = border_and_body(&dir, offset);
        let pinned_june = border.contains("June 2026");
        assert!(
            !(pinned_june && body.contains("June 2026")),
            "June is pinned to the border while its divider is still listed \
             at offset {offset}: {border:?}"
        );
        if pinned_june && switched_at.is_none() {
            switched_at = Some(offset);
        }
        assert_eq!(
            pinned_june,
            offset >= expected,
            "border label at offset {offset} (divider sits at {divider_top}): {border:?}"
        );
    }
    assert_eq!(switched_at, Some(expected));
}

/// The text width the entry panel renders at, read back from the layout so the
/// row cache under test is the one the render used.
fn entry_text_width(dir: &tempfile::TempDir) -> u16 {
    let mut app = app_for(dir);
    let mut view = crate::tui::ui::ViewState::default();
    render_to_text(57, 12, |frame| draw_app(frame, &mut app, &mut view));
    view.layout
        .and_then(|layout| layout.entries)
        .expect("entry list rendered")
        .text_width
}

/// A `work` journal of two July entries above ten June ones — enough rows below
/// the June divider to scroll it clear of the top.
fn month_boundary_journal() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let mut days = vec![
        ("2026-07-02", "2026-07-02T10:00:00+02:00"),
        ("2026-07-01", "2026-07-01T10:00:00+02:00"),
    ];
    for day in 1..=10 {
        days.push((
            Box::leak(format!("2026-06-{day:02}").into_boxed_str()),
            Box::leak(format!("2026-06-{day:02}T10:00:00+02:00").into_boxed_str()),
        ));
    }
    for (index, (dir_day, ts)) in days.iter().enumerate() {
        let entry_dir = dir.path().join("work").join(dir_day);
        fs::create_dir_all(&entry_dir).unwrap();
        fs::write(
            entry_dir.join(format!("e{index}.md")),
            format!("+++\nschema_version = 1\n[time]\ncreated_at = \"{ts}\"\n+++\n\n# e{index}\nBody text\n"),
        )
        .unwrap();
    }
    dir
}

/// The entry panel's top border row and its body rows, scrolled to `offset`.
fn border_and_body(dir: &tempfile::TempDir, offset: usize) -> (String, String) {
    const WIDTH: u16 = 57;
    const HEIGHT: u16 = 12;
    let mut app = app_for(dir);
    *app.nav.entry_list.offset_mut() = offset;
    let backend = render_app(app, WIDTH, HEIGHT);
    let row = |y: u16| {
        (0..WIDTH)
            .map(|x| backend.buffer().cell((x, y)).unwrap().symbol().to_string())
            .collect::<String>()
    };
    (row(0), (1..HEIGHT).map(row).collect::<Vec<_>>().join("\n"))
}

fn app_for(dir: &tempfile::TempDir) -> AppModel {
    let mut app = new_app(Config::new(dir.path().to_path_buf()));
    app.select_journal_by_name("work");
    app.nav.focus = Focus::Entries;
    app
}

fn render_top_border(app: AppModel, width: u16, height: u16) -> String {
    let backend = render_app(app, width, height);
    (0..width)
        .map(|x| backend.buffer().cell((x, 0)).unwrap().symbol().to_string())
        .collect()
}

fn plain_entry(created_at: Option<&str>, preview: &str) -> Entry {
    Entry {
        id: "id".to_string(),
        journal: "work".to_string(),
        path: PathBuf::from("id.md"),
        encryption_state: EntryEncryptionState::Plain,
        created_at: created_at.map(notema_domain::Timestamp::parse),
        edited_at: None,
        preview: preview.to_string(),
        activities: Vec::new(),
        feelings: Vec::new(),
        people: Vec::new(),
        tags: Vec::new(),
        mood: None,
        starred: false,
        location: None,
        weather: None,
        celestial: None,
        air_quality: None,
        import: None,
        body: String::new(),
        word_count: 0,
        search_haystack: String::new(),
        warning: None,
    }
}

#[test]
fn entry_list_lines_put_time_on_right_of_border() {
    let entry = plain_entry(Some("2026-07-01T10:23:00+02:00"), "Preview");

    let rendered = rendered_lines(&entry_list_lines(
        &theme::Theme::terminal_default(),
        &entry,
        None,
        30,
    ));

    assert_eq!(rendered.len(), 3);
    // No date on the first line here, so the time sits alone on the right.
    assert!(rendered[0].starts_with('┌'));
    assert!(rendered[0].ends_with("10:23 ┐"));
    assert!(!rendered[0].contains('·'));
    assert!(rendered[1].starts_with("│ Preview"));
    assert!(rendered[1].ends_with('│'));
    assert!(rendered[2].starts_with('└'));
    assert!(rendered[2].ends_with('┘'));
}

#[test]
fn entry_list_lines_put_day_left_and_time_right() {
    let entry = plain_entry(Some("2026-07-05T14:30:00+02:00"), "Body");

    let rendered = rendered_lines(&entry_list_lines(
        &theme::Theme::terminal_default(),
        &entry,
        Some("Sunday 05"),
        30,
    ));

    assert!(rendered[0].starts_with("┌ Sunday 05 "));
    assert!(rendered[0].ends_with("14:30 ┐"));
    assert!(!rendered[0].contains('·'));
}

#[test]
fn entry_box_lines_without_timestamp_render_plain_top_border() {
    let rendered = rendered_lines(&entry_box_lines(
        &theme::Theme::terminal_default(),
        None,
        "",
        "just a preview",
        None,
        None,
        30,
    ));

    assert_eq!(rendered[0], format!("┌{}┐", "─".repeat(32)));
    assert!(rendered[1].starts_with("│ just a preview"));
}

#[test]
fn search_hit_box_shows_date_time_and_journal() {
    let rendered = rendered_lines(&entry_box_lines(
        &theme::Theme::terminal_default(),
        Some("Sun 05 Jul 2026"),
        "14:30",
        "hit body",
        Some("work"),
        None,
        30,
    ));

    assert!(rendered[0].starts_with("┌ Sun 05 Jul 2026 "));
    assert!(rendered[0].ends_with("14:30 ┐"));
    assert!(rendered[1].starts_with("│ hit body"));
    // Journal on the bottom-left.
    assert!(rendered.last().unwrap().starts_with("└ work "));
}

#[test]
fn entry_group_labels_use_created_timestamp() {
    let entry = Entry {
        id: "id".to_string(),
        journal: "work".to_string(),
        path: PathBuf::from("work/2026-01-01/id.md"),
        encryption_state: EntryEncryptionState::Plain,
        created_at: Some(notema_domain::Timestamp::parse("2026-07-01T10:23:00+02:00")),
        edited_at: None,
        preview: String::new(),
        activities: Vec::new(),
        feelings: Vec::new(),
        people: Vec::new(),
        tags: Vec::new(),
        mood: None,
        starred: false,
        location: None,
        weather: None,
        celestial: None,
        air_quality: None,
        import: None,
        body: String::new(),
        word_count: 0,
        search_haystack: String::new(),
        warning: None,
    };

    assert_eq!(entry_month_label(&entry), Some("July 2026".to_string()));
    assert_eq!(entry_day_label(&entry), Some("Wednesday 01".to_string()));
}

#[test]
fn entry_group_labels_fall_back_to_filename_date() {
    let entry = Entry {
        id: "id".to_string(),
        journal: "work".to_string(),
        path: PathBuf::from("work/2026/07/01/2026-07-01T10-23-00-id.md"),
        encryption_state: EntryEncryptionState::Plain,
        created_at: None,
        edited_at: None,
        preview: String::new(),
        activities: Vec::new(),
        feelings: Vec::new(),
        people: Vec::new(),
        tags: Vec::new(),
        mood: None,
        starred: false,
        location: None,
        weather: None,
        celestial: None,
        air_quality: None,
        import: None,
        body: String::new(),
        word_count: 0,
        search_haystack: String::new(),
        warning: None,
    };

    assert_eq!(entry_month_label(&entry), Some("July 2026".to_string()));
    assert_eq!(entry_day_label(&entry), Some("Wednesday 01".to_string()));
}
