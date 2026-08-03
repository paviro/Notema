//! The reader pane: metadata rows and their wrapping, the environment strip,
//! pill geometry, and the pinned-versus-scrolling metadata split.

use super::*;

fn metadata_values<'a>(
    tags: &'a [String],
    feelings: &'a [String],
    mood: Option<i8>,
) -> EntryMetadataValues<'a> {
    EntryMetadataValues {
        tags,
        people: &[],
        activities: &[],
        feelings,
        mood,
        environment: &[],
    }
}

#[test]
fn metadata_hit_map_accounts_for_mood_row() {
    let area = Rect::new(42, 0, 60, 19);
    let tags = vec!["work".to_string()];
    let feelings = vec!["focused".to_string()];
    let values = metadata_values(&tags, &feelings, Some(2));
    let layout =
        crate::tui::surface::entry_metadata_layout(&theme::Theme::terminal_default(), area, values);
    let chips = layout.chips.unwrap();
    // Mood bar on its own row, then a blank gap row, then the chips.
    assert_eq!(chips.y, layout.mood.unwrap().y + 2);

    // One glyph-led flow: " * focused  # work " — the feeling pill spans 11
    // cells (its glyph, "focused", and padding), then a separator space, then
    // the tag pill.
    assert_eq!(
        metadata_at_point(
            &theme::Theme::terminal_default(),
            area,
            chips.x,
            chips.y,
            values
        ),
        Some((MetadataChip::Feelings, "focused".to_string()))
    );
    assert_eq!(
        metadata_at_point(
            &theme::Theme::terminal_default(),
            area,
            chips.x + 12,
            chips.y,
            values
        ),
        Some((MetadataChip::Tags, "work".to_string()))
    );
    // The separator cell between the two pills hits nothing.
    assert_eq!(
        metadata_at_point(
            &theme::Theme::terminal_default(),
            area,
            chips.x + 11,
            chips.y,
            values
        ),
        None
    );
}

#[test]
fn metadata_layout_places_environment_strip_before_mood_and_rows() {
    let area = Rect::new(42, 0, 60, 19);
    let tags = vec!["work".to_string()];
    let environment = crate::tui::env_strip::environment_items(
        &crate::tui::theme::Theme::terminal_default(),
        Some("Testville, Testland"),
        None,
        None,
        None,
    );
    let values = EntryMetadataValues {
        tags: &tags,
        people: &[],
        activities: &[],
        feelings: &[],
        mood: Some(2),
        environment: &environment,
    };

    let layout =
        crate::tui::surface::entry_metadata_layout(&theme::Theme::terminal_default(), area, values);
    let strip = layout.environment.expect("environment strip is laid out");
    let mood = layout.mood.unwrap();
    let chips = layout.chips.unwrap();

    // Stacked right under the separator, above the mood bar and chip rows, and
    // it does not participate in the click hit-test.
    assert_eq!(strip.y, layout.metadata.unwrap().y + 1);
    assert!(mood.y >= strip.y + strip.height);
    assert!(chips.y > mood.y);
    assert_eq!(
        metadata_at_point(
            &theme::Theme::terminal_default(),
            area,
            strip.x,
            strip.y,
            values
        ),
        None
    );
}

#[test]
fn environment_strip_height_reflects_wrapped_rows() {
    let area = Rect::new(0, 0, 24, 60);

    let short = crate::tui::env_strip::environment_items(
        &crate::tui::theme::Theme::terminal_default(),
        Some("Cafe"),
        None,
        None,
        None,
    );
    let long = crate::tui::env_strip::environment_items(
        &crate::tui::theme::Theme::terminal_default(),
        Some("Grand Central Station Cafe"),
        None,
        None,
        None,
    );
    let values = |environment| EntryMetadataValues {
        tags: &[],
        people: &[],
        activities: &[],
        feelings: &[],
        mood: None,
        environment,
    };
    let short_layout = crate::tui::surface::entry_metadata_layout(
        &theme::Theme::terminal_default(),
        area,
        values(&short),
    );
    let long_layout = crate::tui::surface::entry_metadata_layout(
        &theme::Theme::terminal_default(),
        area,
        values(&long),
    );

    assert_eq!(short_layout.environment.unwrap().height, 1);
    assert!(long_layout.environment.unwrap().height >= 2);
}

#[test]
fn reader_wraps_long_location_hanging_under_its_glyph() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
            entry_dir.join("a.md"),
            "+++\nschema_version = 1\n\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n\n[location]\nname = \"Grand Central Station Cafe\"\n+++\n\n# A\nBody\n",
        )
        .unwrap();
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    app.nav.focus = Focus::Reader;

    let reader = Rect::new(0, 0, 24, 60 - footer_height(&app, 24));
    let environment = crate::tui::env_strip::environment_items(
        &crate::tui::theme::Theme::terminal_default(),
        Some("Grand Central Station Cafe"),
        None,
        None,
        None,
    );
    let values = EntryMetadataValues {
        tags: &[],
        people: &[],
        activities: &[],
        feelings: &[],
        mood: None,
        environment: &environment,
    };
    let metadata = crate::tui::surface::entry_metadata_layout(
        &theme::Theme::terminal_default(),
        reader,
        values,
    );
    let strip = metadata.environment.expect("environment strip is laid out");
    assert!(strip.height >= 2, "label should wrap: {strip:?}");

    let backend = render_app(app, 24, 60);
    let buffer = backend.buffer();

    // Line 0 leads with the location glyph; continuation rows indent two cells
    // under it, so the glyph column stays blank below.
    assert_eq!(
        buffer.cell((strip.x, strip.y)).unwrap().symbol(),
        crate::tui::theme::Theme::terminal_default()
            .env_glyphs()
            .location
            .to_string()
    );
    assert_eq!(buffer.cell((strip.x, strip.y + 1)).unwrap().symbol(), " ");
    assert_eq!(
        buffer.cell((strip.x + 1, strip.y + 1)).unwrap().symbol(),
        " "
    );
    assert_ne!(
        buffer.cell((strip.x + 2, strip.y + 1)).unwrap().symbol(),
        " ",
        "continuation text starts after the two-cell indent"
    );
}

#[test]
fn metadata_hit_map_uses_terminal_cell_width_for_wide_text() {
    let area = Rect::new(42, 0, 60, 19);
    let tags = vec!["集中".to_string()];
    let feelings = vec!["嬉しい".to_string()];
    let values = metadata_values(&tags, &feelings, None);
    let layout =
        crate::tui::surface::entry_metadata_layout(&theme::Theme::terminal_default(), area, values);
    let chips = layout.chips.unwrap();

    // " 嬉しい  集中 " — the feeling pill spans 8 terminal cells (6 for the
    // wide glyphs + padding), the tag pill starts one separator later.
    assert_eq!(
        metadata_at_point(
            &theme::Theme::terminal_default(),
            area,
            chips.x + 5,
            chips.y,
            values
        ),
        Some((MetadataChip::Feelings, "嬉しい".to_string()))
    );
    assert_eq!(
        metadata_at_point(
            &theme::Theme::terminal_default(),
            area,
            chips.x + 9 + 2,
            chips.y,
            values
        ),
        Some((MetadataChip::Tags, "集中".to_string()))
    );
}

#[test]
fn metadata_rows_wrap_without_leading_separator() {
    let values = vec![
        "calm".to_string(),
        "focused".to_string(),
        "tired".to_string(),
    ];

    let rows = crate::tui::surface::metadata_value_rows("Feelings: ".len() as u16, 20, &values);

    assert_eq!(rows, vec![vec![0], vec![1, 2]]);
}

#[test]
fn reader_wraps_metadata_rows_without_leading_space_or_separator() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
            entry_dir.join("a.md"),
            "+++\nschema_version = 1\n\n[entry]\ntags = [\"work\", \"personal\", \"health\"]\nfeelings = [\"calm\", \"focused\", \"tired\"]\n\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n+++\n\n# A\nBody\n",
        )
        .unwrap();
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    app.nav.focus = Focus::Reader;

    let tags = vec![
        "work".to_string(),
        "personal".to_string(),
        "health".to_string(),
    ];
    let feelings = vec![
        "calm".to_string(),
        "focused".to_string(),
        "tired".to_string(),
    ];
    let reader = Rect::new(0, 0, 24, 60 - footer_height(&app, 24));
    let values = metadata_values(&tags, &feelings, None);
    let metadata = crate::tui::surface::entry_metadata_layout(
        &theme::Theme::terminal_default(),
        reader,
        values,
    );
    let chips = metadata.chips.unwrap();

    let backend = render_app(app, 24, 60);
    let buffer = backend.buffer();

    // At 20 content cells the six glyph-led pills flow onto four rows:
    // " * calm  * focused " / " * tired  # work " / " # personal " / " # health ".
    // A blank spacer separates each pair, so the four rows span seven display
    // rows (chip rows on even offsets, spacers on odd).
    assert_eq!(chips.height, 7);
    // A wrapped row starts with its first pill: the one-cell pill padding, then
    // the category glyph, then the value — no separator, no extra leading space.
    assert_eq!(buffer.cell((chips.x, chips.y + 2)).unwrap().symbol(), " ");
    assert_eq!(
        buffer.cell((chips.x + 1, chips.y + 2)).unwrap().symbol(),
        "*"
    );
    assert_eq!(
        buffer.cell((chips.x + 1, chips.y + 4)).unwrap().symbol(),
        "#"
    );
    // The pill padding is part of the hit region, and the flow keeps each
    // value's category across the wrap.
    assert_eq!(
        metadata_at_point(
            &theme::Theme::terminal_default(),
            reader,
            chips.x,
            chips.y + 2,
            values
        ),
        Some((MetadataChip::Feelings, "tired".to_string()))
    );
    assert_eq!(
        metadata_at_point(
            &theme::Theme::terminal_default(),
            reader,
            chips.x,
            chips.y + 4,
            values
        ),
        Some((MetadataChip::Tags, "personal".to_string()))
    );
    // Spacer rows click nothing.
    assert_eq!(
        metadata_at_point(
            &theme::Theme::terminal_default(),
            reader,
            chips.x,
            chips.y + 1,
            values
        ),
        None
    );
    assert_eq!(
        metadata_at_point(
            &theme::Theme::terminal_default(),
            reader,
            chips.x,
            chips.y + 3,
            values
        ),
        None
    );
}

#[test]
fn short_reader_scrolls_metadata_after_body() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    let body = (1..=40)
        .map(|index| format!("Line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
            entry_dir.join("a.md"),
            format!(
                "+++\nschema_version = 1\n\n[entry]\ntags = [\"tiny-screen\"]\nfeelings = [\"focused\"]\n\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n+++\n\n# A\n{body}\n",
            ),
        )
        .unwrap();
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    app.nav.focus = Focus::Reader;

    let top = render_text(app, 80, 20);
    assert!(!top.contains("tiny-screen"));

    let mut app = new_app(Config::new(dir.path().to_path_buf()));
    app.select_journal_by_name("work");
    app.nav.focus = Focus::Reader;
    app.nav.scroll.reader = u16::MAX;

    let bottom = render_text(app, 80, 20);
    assert!(bottom.contains(" * focused   # tiny-screen "));
}

#[test]
fn metadata_pins_only_when_body_keeps_min_height() {
    let tags = vec!["x".to_string()];
    let values = EntryMetadataValues {
        tags: &tags,
        people: &[],
        activities: &[],
        feelings: &[],
        mood: None,
        environment: &[],
    };
    // Separator + one tag row = 2 metadata rows; inner height = area.height - 2. The
    // body needs 20 lines, so pin only once the inner height reaches 22 (area 24).
    assert!(metadata_scrolls_with_body(
        &theme::Theme::terminal_default(),
        Rect::new(0, 0, 80, 23),
        values
    )); // inner 21 → body 19
    assert!(!metadata_scrolls_with_body(
        &theme::Theme::terminal_default(),
        Rect::new(0, 0, 80, 24),
        values
    )); // inner 22 → body 20
}

/// The scrolling layout must produce exactly the rows the pinned layout
/// reserves — the two render modes share one height truth or the pinned
/// split truncates.
#[test]
fn metadata_scroll_lines_match_the_pinned_section_height() {
    let metadata = notema_domain::Metadata {
        tags: vec!["work".to_string(), "health".to_string()],
        feelings: vec![
            "calm".to_string(),
            "focused".to_string(),
            "tired".to_string(),
        ],
        people: vec!["Alex".to_string()],
        mood: Some(2),
        location: Some(notema_domain::Location {
            name: Some("Grand Central Station Cafe".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let weather = notema_domain::Weather {
        condition: Some("rain".to_string()),
        temperature_celsius: Some(12.0),
        feels_like_celsius: Some(7.0),
        ..Default::default()
    };
    let celestial = notema_domain::Celestial {
        moon_phase_name: Some("full".to_string()),
        sunrise: Some("2026-07-01T05:12:00+02:00".to_string()),
        sunset: Some("2026-07-01T21:48:00+02:00".to_string()),
        ..Default::default()
    };
    let air = notema_domain::AirQuality {
        european_aqi: Some(84),
        ..Default::default()
    };
    let active_theme = theme::Theme::terminal_default();
    let entry_metadata = super::metadata::EntryMetadata::for_entry(
        &active_theme,
        &metadata,
        crate::tui::env_strip::EnvironmentRef {
            weather: Some(&weather),
            celestial: Some(&celestial),
            air_quality: Some(&air),
        },
    );

    for width in [20u16, 32, 48, 80] {
        let lines = super::metadata::metadata_section_lines(
            &theme::Theme::terminal_default(),
            width,
            &entry_metadata,
        );
        let height = crate::tui::surface::metadata_section_height(width, entry_metadata.values());
        assert_eq!(
            lines.len() as u16,
            height,
            "modes disagree at width {width}"
        );
    }
}

/// Every pill style occupies its category glyph plus value-width + 2 cells, so
/// switching themes can never change the layout or the hit-test; only the ink
/// differs.
#[test]
fn pill_styles_share_geometry_across_reversed_bg_and_bracket() {
    let metadata = notema_domain::Metadata {
        tags: vec!["work".to_string()],
        ..Default::default()
    };
    let reversed_theme = theme::Theme::terminal_default();
    let entry_metadata =
        super::metadata::EntryMetadata::for_entry(&reversed_theme, &metadata, Default::default());
    let tags_line =
        |theme| super::metadata::metadata_section_lines(theme, 40, &entry_metadata)[1].clone();

    // The default (classic/e-ink) look: inverted pills, led by the tag glyph.
    let reversed = tags_line(&reversed_theme);
    assert_eq!(reversed.spans[0].content.as_ref(), " # work ");
    assert!(
        reversed.spans[0]
            .style
            .add_modifier
            .contains(Modifier::REVERSED)
    );

    let bg_theme = theme::test_theme_from_toml(
        "[metadata.pills]\nstyle = \"bg\"\ntags = { fg = \"#101010\", bg = \"#aabbcc\" }",
    );
    let bg = tags_line(&bg_theme);
    assert_eq!(bg.spans[0].content.as_ref(), " # work ");
    assert_eq!(
        bg.spans[0].style.bg,
        Some(ratatui::style::Color::Rgb(0xaa, 0xbb, 0xcc))
    );

    let bracket_theme = theme::test_theme_from_toml("[metadata.pills]\nstyle = \"bracket\"");
    let bracket = tags_line(&bracket_theme);
    assert_eq!(bracket.spans[0].content.as_ref(), "[# work]");

    assert_eq!(reversed.width(), bg.width());
    assert_eq!(bg.width(), bracket.width());
}

/// The environment strip renders straight from the entry's front-matter
/// tables: weather with temperature, air quality and pollen only when bad,
/// moon, sun times at the entry's own offset, and the location.
#[test]
fn reader_renders_the_environment_strip_from_front_matter() {
    let entry_text = |aqi: i64, grass_pollen: f64| {
        format!(
            "+++\nschema_version = 1\n\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n\n\
             [location]\nname = \"Testville\"\n\n\
             [weather]\ncondition = \"clear\"\ntemperature_celsius = 18.4\n\n\
             [celestial]\nmoon_phase_name = \"full\"\nsunrise = \"2026-07-01T05:12:00+02:00\"\nsunset = \"2026-07-01T21:48:00+02:00\"\n\n\
             [air_quality]\neuropean_aqi = {aqi}\ngrass_pollen = {grass_pollen}\n+++\n\n# A\nBody\n"
        )
    };
    let render = |aqi: i64, grass_pollen: f64| {
        let dir = tempdir().unwrap();
        let entry_dir = dir.path().join("work").join("2026-07-01");
        fs::create_dir_all(&entry_dir).unwrap();
        fs::write(entry_dir.join("a.md"), entry_text(aqi, grass_pollen)).unwrap();
        let mut app = new_app(Config::new(dir.path().to_path_buf()));
        app.select_journal_by_name("work");
        app.nav.focus = Focus::Reader;
        render_text(app, 120, 40)
    };

    // Glyphs are the fallback theme's — classic's all-ASCII set.
    let bad_air = render(72, 80.0);
    assert!(bad_air.contains("o 18°C clear"), "strip was:\n{bad_air}");
    assert!(bad_air.contains("! AQI 72"));
    assert!(bad_air.contains("% high grass pollen"));
    assert!(bad_air.contains("O full moon"));
    assert!(bad_air.contains("^ 05:12 v 21:48"));
    assert!(bad_air.contains("@ Testville"));

    // Clean air and unremarkable pollen never render — both badges only
    // appear from their warning bands upward.
    let clean_air = render(55, 10.0);
    assert!(!clean_air.contains("AQI"), "strip was:\n{clean_air}");
    assert!(!clean_air.contains("pollen"));
    assert!(clean_air.contains("o 18°C clear"));
}

/// The mouse path must feed the hit-test the same environment items the
/// viewer drew: the strip's rows shift every chip row below them, so a hit-test
/// that omits the strip lands clicks on the wrong row.
#[test]
fn chip_hit_test_accounts_for_the_environment_strip_rows() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n\n[entry]\ntags = [\"work\"]\n\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n\n[location]\nname = \"Testville Cafe\"\n+++\n\n# A\nBody\n",
    )
    .unwrap();
    let mut app = new_app(Config::new(dir.path().to_path_buf()));
    app.select_journal_by_name("work");
    app.nav.focus = Focus::Reader;

    let tags = app.selected_entry_tags();
    let environment = app.selected_entry_env_items();
    assert!(!environment.is_empty(), "the accessor must carry the strip");
    let reader = Rect::new(0, 0, 80, 40);
    let values = EntryMetadataValues {
        tags: &tags,
        people: &[],
        activities: &[],
        feelings: &[],
        mood: None,
        environment: &environment,
    };
    let chips = crate::tui::surface::entry_metadata_layout(
        &theme::Theme::terminal_default(),
        reader,
        values,
    )
    .chips
    .unwrap();

    // A click on the drawn chips row resolves its pill…
    assert_eq!(
        metadata_at_point(
            &theme::Theme::terminal_default(),
            reader,
            chips.x + 1,
            chips.y,
            values
        ),
        Some((MetadataChip::Tags, "work".to_string()))
    );
    // …and the strip really is part of the section's shape: without it the
    // hit-test would reason about a shorter section than the one drawn (the
    // old `location: None` bug), so the section rects must disagree.
    let without_strip = EntryMetadataValues {
        environment: &[],
        ..values
    };
    assert_ne!(
        crate::tui::surface::entry_metadata_layout(
            &theme::Theme::terminal_default(),
            reader,
            without_strip
        )
        .metadata,
        crate::tui::surface::entry_metadata_layout(
            &theme::Theme::terminal_default(),
            reader,
            values
        )
        .metadata,
    );
}

#[test]
fn reader_renders_feelings_metadata() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
            entry_dir.join("a.md"),
            "+++\nschema_version = 1\n\n[entry]\nfeelings = [\"calm\", \"focused\"]\n\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n+++\n\n# A\nBody\n",
        )
        .unwrap();
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    app.nav.focus = Focus::Reader;

    let rendered = render_text(app, 120, 20);

    // Each value is a glyph-led padded pill in one label-less flow; feelings
    // lead with their category glyph: " * calm   * focused ".
    assert!(rendered.contains(" * calm   * focused "));
}

#[test]
fn reader_renders_indented_mermaid_diagram() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
            entry_dir.join("a.md"),
            "+++\nschema_version = 1\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n+++\n\n# A\n```mermaid\n  graph TD\n      A[Open journal] --> B[Write entry]\n      B --> C{Preview}\n      C -->|looks good| D[Save]\n      C -->|needs work| B\n  ```\n",
        )
        .unwrap();
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    app.nav.focus = Focus::Reader;

    let rendered = render_text(app, 140, 28);

    assert!(rendered.contains("mermaid"));
    assert!(rendered.contains("Open journal"));
    assert!(rendered.contains("Write entry"));
}
