//! The internal editor pane: its frame, metadata menu, environment strip, and
//! how its body framing agrees with the reader.

use super::*;

#[test]
fn internal_editor_renders_in_reader_pane() {
    let mut app = app_with_entry();
    app.open_editor_for_selected().unwrap();
    let text = render_text(app, INLINE_READER_MIN_WIDTH, 30);
    // The textarea shows the raw markdown source (with the leading `#`), unlike
    // the viewer which renders the heading, so the literal `# A` proves the
    // editor drew in the pane.
    assert!(text.contains("# A"));
    // The editor footer replaces the browse hints.
    assert!(text.contains("ctrl+s"));
}

#[test]
fn internal_editor_renders_full_screen() {
    let mut app = app_with_entry();
    app.open_editor_for_selected().unwrap();
    app.nav.reader_fullscreen = true;
    let text = render_text(app, INLINE_READER_MIN_WIDTH, 30);
    assert!(text.contains("# A"));
    assert!(text.contains("ctrl+s"));
}

#[test]
fn internal_editor_new_entry_renders_in_pane_not_insights() {
    let mut app = app_with_journals(&["work"]);
    app.select_journal_by_name("work");
    app.open_editor_for_new();
    // Not fullscreen: the entry list column is still present alongside the editor.
    let text = render_text(app, INLINE_READER_MIN_WIDTH, 30);
    assert!(text.contains("New entry")); // editor pane title, not the insights panel
    assert!(text.contains("ctrl+s")); // editor footer
}

#[test]
fn internal_editor_metadata_menu_renders() {
    let mut app = app_with_entry();
    app.open_editor_for_selected().unwrap();
    app.editor.as_mut().unwrap().prompt = crate::tui::editor_state::EditorPrompt::MetadataMenu;
    let text = render_text(app, INLINE_READER_MIN_WIDTH, 30);
    assert!(text.contains("Add Metadata"));
    assert!(text.contains("Feelings"));
}

/// The editor's metadata section renders the entry's location just like the
/// viewer — both go through `EntryMetadata::for_entry`, so a front-matter
/// field can't show in one mode and vanish in the other.
#[test]
fn internal_editor_shows_entry_location() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n\n[location]\nname = \"Testville Cafe\"\n+++\n\n# A\nBody\n",
    )
    .unwrap();
    let mut app = new_app(Config::new(dir.path().to_path_buf()));
    app.select_journal_by_name("work");
    app.open_editor_for_selected().unwrap();

    let text = render_text(app, INLINE_READER_MIN_WIDTH, 30);
    assert!(text.contains("Testville Cafe"), "editor pane was:\n{text}");
}

/// An entry with environment tables, as `open_editor_for_selected` loads it.
fn app_editing_entry_with_environment() -> (tempfile::TempDir, AppModel) {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n\n\
         [location]\nname = \"Testville\"\n\n\
         [weather]\ncondition = \"clear\"\ntemperature_celsius = 18.4\n\n\
         [celestial]\nmoon_phase_name = \"full\"\n+++\n\n# A\nBody\n",
    )
    .unwrap();
    let mut app = new_app(Config::new(dir.path().to_path_buf()));
    app.select_journal_by_name("work");
    app.open_editor_for_selected().unwrap();
    (dir, app)
}

/// The editor's strip shows the entry's saved environment, not just its
/// location — the gap that had the editor rendering less than the viewer.
#[test]
fn internal_editor_shows_the_saved_environment_strip() {
    let (_dir, app) = app_editing_entry_with_environment();
    let text = render_text(app, 120, 40);
    assert!(text.contains("o 18°C clear"), "editor pane was:\n{text}");
    assert!(text.contains("O full moon"), "editor pane was:\n{text}");
}

/// A location changed mid-edit refetches the environment; the strip must follow
/// it rather than keep showing the tables the entry was saved with.
#[test]
fn internal_editor_strip_prefers_a_landed_fetch_over_the_saved_tables() {
    let (_dir, mut app) = app_editing_entry_with_environment();
    app.editor.as_mut().unwrap().environment = Some(notema_context::EnvironmentReport {
        weather: Some(notema_domain::Weather {
            condition: Some("fog".to_string()),
            temperature_celsius: Some(3.0),
            ..Default::default()
        }),
        ..Default::default()
    });

    let text = render_text(app, 120, 40);
    assert!(text.contains("3°C fog"), "editor pane was:\n{text}");
    assert!(!text.contains("18°C clear"), "editor pane was:\n{text}");
}

/// A new entry has no saved tables, so its strip comes from the fetch its own
/// location triggered.
#[test]
fn internal_editor_new_entry_shows_its_fetched_environment() {
    let mut app = app_with_journals(&["work"]);
    app.select_journal_by_name("work");
    app.open_editor_for_new();
    app.editor.as_mut().unwrap().environment = Some(notema_context::EnvironmentReport {
        weather: Some(notema_domain::Weather {
            condition: Some("fog".to_string()),
            temperature_celsius: Some(3.0),
            ..Default::default()
        }),
        ..Default::default()
    });

    let text = render_text(app, 120, 40);
    assert!(text.contains("3°C fog"), "editor pane was:\n{text}");
}

/// The editor's panel carries the same word-count label the viewer's does.
#[test]
fn internal_editor_shows_the_word_count() {
    let (_dir, app) = app_editing_entry_with_environment();
    // "# A" + "Body" — the raw source the textarea holds.
    let text = render_text(app, 120, 40);
    assert!(text.contains("3 words"), "editor pane was:\n{text}");
}

/// Both metadata paint paths run the mood row through `mood_line`, so a narrow
/// pane drops the pole labels whether the block is pinned or scrolling — the
/// pinned path used to keep them and crush the bar instead.
#[test]
fn pinned_and_scrolling_mood_rows_agree_on_a_narrow_pane() {
    let theme = theme::Theme::terminal_default();
    let metadata = notema_domain::Metadata {
        mood: Some(2),
        ..Default::default()
    };
    let entry_metadata =
        super::metadata::EntryMetadata::for_entry(&theme, &metadata, Default::default());
    // Tall enough to pin the block, narrow enough to lose the labels.
    let area = Rect::new(0, 0, 22, 40);
    let pinned = render_to_text(area.width, area.height, |frame| {
        let layout =
            crate::tui::surface::entry_metadata_layout(&theme, area, entry_metadata.values());
        super::metadata::draw_metadata_section(
            &theme,
            frame,
            layout,
            &entry_metadata,
            crate::tui::state::HoverTarget::None,
        );
    });
    assert!(!pinned.contains("Miserable"), "pinned row was:\n{pinned}");
    assert!(!pinned.contains("Blissful"), "pinned row was:\n{pinned}");
}

/// The viewer and the editor frame the body identically, so toggling between
/// them can't shift or re-wrap the text.
#[test]
fn reader_and_editor_frame_the_body_the_same() {
    let theme = theme::Theme::terminal_default();
    let metadata = notema_domain::Metadata {
        tags: vec!["one".to_string()],
        mood: Some(2),
        ..Default::default()
    };
    let entry_metadata =
        super::metadata::EntryMetadata::for_entry(&theme, &metadata, Default::default());
    for area in [
        Rect::new(0, 0, 120, 40),
        Rect::new(0, 0, 60, 30),
        // Short enough that the metadata gives up its pinned slot.
        Rect::new(0, 0, 120, 12),
    ] {
        let layout = crate::config::LayoutSection::default();
        let reader = super::layout::EntryBodyFrame::new(
            &theme,
            area,
            entry_metadata.values(),
            layout.reader_body(),
        );
        let editor = super::layout::EntryBodyFrame::new(
            &theme,
            area,
            entry_metadata.values(),
            layout.editor_body(),
        );
        assert_eq!(reader.body, editor.body, "body rect differs at {area:?}");
        assert_eq!(
            reader.metadata_scrolls(),
            editor.metadata_scrolls(),
            "metadata placement differs at {area:?}"
        );
        assert_eq!(
            reader.top_pad(1),
            editor.top_pad(1),
            "top padding differs at {area:?}"
        );
    }
}

/// The top padding is a floor: a body long enough that centering would sit it
/// higher than the ramp keeps the ramp, and scrolls if it must.
#[test]
fn top_padding_outranks_centering_on_a_long_body() {
    let theme = theme::Theme::terminal_default();
    let metadata = notema_domain::Metadata {
        tags: vec!["one".to_string()],
        mood: Some(2),
        ..Default::default()
    };
    let entry_metadata =
        super::metadata::EntryMetadata::for_entry(&theme, &metadata, Default::default());
    // Wide enough for a ramp — it needs two gutter columns per blank line — and
    // tall enough for centering to beat it on a short body.
    let area = Rect::new(0, 0, 160, 40);
    let mut layout = crate::config::LayoutSection::default();
    // The reader centers by default; match it so both surfaces are exercised.
    layout.editor.body_center_vertically = Some(true);
    for body_layout in [layout.reader_body(), layout.editor_body()] {
        let frame =
            super::layout::EntryBodyFrame::new(&theme, area, entry_metadata.values(), body_layout);
        let height = frame.body.height as usize;
        // A body filling the pane never centers, so this is the ramp itself.
        let ramp = frame.top_pad(height);
        assert!(ramp > 0, "no ramp to measure against at {area:?}");

        // Short body: centering sits it below the ramp, so it wins.
        assert!(frame.centers(1));
        assert_eq!(frame.top_pad(1), 0);
        assert!(frame.centered(1).y > frame.body.y + ramp);

        // Near-full body: centering would sit it above the ramp, so the ramp holds.
        let long = height - 2;
        assert!(!frame.centers(long));
        assert_eq!(frame.top_pad(long), ramp);
        assert_eq!(frame.centered(long), frame.body);

        // The viewer measures `centered` a ramp later than `top_pad`, so the
        // two must still agree there.
        assert!(!frame.centers(long + ramp as usize));
    }
}
