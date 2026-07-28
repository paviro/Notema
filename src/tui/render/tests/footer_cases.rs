//! What the footer offers per focus, and how its hints route back to a click.

use super::*;

#[test]
fn journal_footer_omits_entry_actions() {
    let mut app = app_with_entry();
    app.nav.focus = Focus::Journals;

    let text = footer_text(&app, 200);

    assert!(!text.contains("enter  view"));
    assert!(!text.contains("e  edit"));
    assert!(!text.contains("d  del"));
}

#[test]
fn entries_footer_includes_entry_actions_when_an_entry_is_selected() {
    let mut app = app_with_entry();
    app.nav.focus = Focus::Entries;

    let text = footer_text(&app, 200);

    // The direct entry-action chips are present; view is gone (clicking the row
    // already opens it).
    assert!(text.contains("e  edit"));
    assert!(text.contains("t  tags"));
    assert!(text.contains("p  people"));
    assert!(text.contains("s  star"));
    assert!(text.contains("d  del"));
    assert!(!text.contains("enter  view"));
    // With an entry selected the footer matches the reader: no global tail.
    assert!(!text.contains("/  search"));
    assert!(!text.contains("?  help"));
    assert!(!text.contains("q  quit"));

    // The empty entries list keeps the tail — it has no entry actions to show.
    app.nav.selected_entry_index = None;
    let empty = footer_text(&app, 200);
    assert!(empty.contains("/  search"));
    assert!(empty.contains("?  help"));
    assert!(empty.contains("q  quit"));
}

#[test]
fn entries_and_reader_share_the_entry_actions_but_only_entries_shows_the_index() {
    let mut app = app_with_entry();

    app.nav.focus = Focus::Entries;
    let entries = footer_text(&app, 200);
    app.nav.focus = Focus::Reader;
    let reader = footer_text(&app, 200);

    // Both render `focused_entry_footer`, so the shared entry actions cannot drift.
    for label in ["e  edit", "t  tags", "s  star", "d  del"] {
        assert!(entries.contains(label), "entries missing {label}");
        assert!(reader.contains(label), "reader missing {label}");
    }
    // `b` opens the filter browser from the entries column but is inert in the
    // reader, so only the entries footer advertises it.
    assert!(entries.contains("b  filter"), "entries missing filter chip");
    assert!(
        !reader.contains("filter"),
        "reader should not show the filter chip"
    );
}

#[test]
fn reader_footer_drops_the_close_chip_and_global_tail() {
    let mut app = app_with_entry();
    app.nav.focus = Focus::Reader;

    // The full-screen reader renders this exact footer (via `footer_lines`), so it
    // matches the inline reader by construction — no separate expanded footer.
    let text = footer_text(&app, 200);

    for label in ["e  edit", "t  tags", "s  star", "d  del"] {
        assert!(text.contains(label), "missing {label}");
    }
    // No close chip, no global tail. Enter/Esc/← still collapse full screen by key;
    // they just aren't advertised on the busy bar.
    for absent in ["close", "/  search", "?  help", "q  quit"] {
        assert!(!text.contains(absent), "still has {absent}");
    }
}

#[test]
fn expanded_entry_draws_confirm_delete_overlay() {
    let mut app = app_with_entry();
    app.nav.focus = Focus::Reader;
    app.begin_confirm_delete();

    let text = render_text(app, 80, 20);

    assert!(text.contains("Confirm Delete"));
    assert!(text.contains("Move entry to trash?"));
    assert!(text.contains("Delete") && text.contains("Cancel"));
}

#[test]
fn entries_footer_omits_entry_actions_without_a_selection() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("work")).unwrap();
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    app.nav.focus = Focus::Entries;

    let text = footer_text(&app, 200);

    assert!(!text.contains("enter  view"));
    assert!(!text.contains("e  edit"));
    assert!(!text.contains("d  del"));
}

#[test]
fn search_results_footer_shows_escape_and_entry_actions() {
    let mut app = app_with_entry();
    app.nav.mode = Mode::Search;
    app.nav.focus = Focus::Entries;
    app.search.query = "body".into();
    app.search.hits = vec![SearchHit {
        id: app.library.entries[0].id.clone(),
        journal: "work".to_string(),
        created_at: None,
        title: "A".to_string(),
        preview: "Body".to_string(),
        starred: false,
    }];

    let text = footer_text(&app, 200);

    // The query now lives on the entry panel's top-right border, not the footer.
    // While typing (Entries focus) the footer carries only the exit hint; the
    // entry actions appear once a result is opened in the reader.
    assert!(!text.contains("Search all: body"));
    assert!(!text.contains("enter  view"));
    assert!(text.contains("esc  exit search"));
    assert!(!text.contains("type query"));
    assert!(!text.contains("backspace"));
    assert!(!text.contains("e  edit"));
    assert!(!text.contains("d  del"));
}

#[test]
fn narrow_footer_wraps_actions_below_columns() {
    let mut app = app_with_entry();
    app.nav.focus = Focus::Entries;

    let layout = tui_layout(Rect::new(0, 0, 60, 20), &app);

    assert!(layout.footer.height > 1);
    assert_eq!(layout.footer.height, footer_height(&app, 60));
    assert_eq!(layout.content.height, 20 - layout.footer.height);
}

#[test]
fn wrapped_footer_hint_routing_uses_visible_row() {
    let mut app = app_with_entry();
    app.nav.focus = Focus::Entries;

    let width = 60;
    let origin_y = 18;
    let text = footer_text(&app, width);
    let (row_index, line) = text
        .split('\n')
        .enumerate()
        .find(|(_, line)| line.contains("t  tags"))
        .expect("metadata hint present");
    let col = line.find("t  tags").unwrap() as u16;

    assert_eq!(
        footer_hint_id_at_point(&app, 0, origin_y, width, col, origin_y + row_index as u16),
        Some(HintId::EditTags)
    );
}

#[test]
fn footer_hint_routing_uses_typed_ids() {
    let mut app = app_with_entry();
    app.nav.focus = Focus::Entries;
    let text = footer_text(&app, 200);

    assert_eq!(
        footer_hint_id_at(&app, 0, 200, text.find("t  tags").unwrap() as u16),
        Some(HintId::EditTags)
    );
    assert_eq!(
        footer_hint_id_at(&app, 0, 200, text.find("e  edit").unwrap() as u16),
        Some(HintId::EditSelected)
    );
}

#[test]
fn fullscreen_reader_footer_routes_via_the_shared_hit_test() {
    let mut app = app_with_entry();
    app.nav.focus = Focus::Reader;
    let width = 120;
    let origin_y = 19;
    // The full-screen footer renders through `footer_lines`/`footer_hint_id_at_point`
    // now — flush, no inset — so its clicks route through the same hit-test as the
    // inline footer.
    let text = footer_text(&app, width);
    let (row_index, line) = text
        .split('\n')
        .enumerate()
        .find(|(_, line)| line.contains("t  tags"))
        .expect("metadata hint present");
    let col = line.find("t  tags").unwrap() as u16;

    assert_eq!(
        footer_hint_id_at_point(&app, 0, origin_y, width, col, origin_y + row_index as u16),
        Some(HintId::EditTags)
    );
}

/// Every hint is clickable at its own rendered position, whatever row the grid
/// places it on.
fn assert_hints_routable(hints: &[Hint], width: u16) {
    let text = hint_grid_text(hints, width);
    for (row_index, line) in text.split('\n').enumerate() {
        for hint in hints {
            // The `key  label` pair is unambiguous within a row (a bare label can
            // be a substring of another hint's text).
            let needle = format!("{}  {}", hint.key_hint, hint.label);
            if let Some(col) = line.find(&needle) {
                assert_eq!(
                    hint_id_at_wrapped(hints, 0, 0, width, col as u16, row_index as u16),
                    Some(hint.id),
                    "hint {:?} on row {row_index}",
                    hint.label
                );
            }
        }
    }
}

#[test]
fn dialog_hints_wrap_and_remain_clickable_by_row() {
    let hints = metadata_dialog_hints(EditMetadataFocus::List, true);

    assert!(hint_height(hints, 29) >= 2, "expected the hints to wrap");
    assert_hints_routable(hints, 29);
}

#[test]
fn dialog_hint_routing_uses_typed_ids() {
    assert_hints_routable(metadata_dialog_hints(EditMetadataFocus::List, true), 200);
    assert_hints_routable(metadata_dialog_hints(EditMetadataFocus::Input, true), 200);
    assert_hints_routable(metadata_dialog_hints(EditMetadataFocus::Input, false), 200);
    assert_hints_routable(feelings_dialog_hints(EditMetadataFocus::List), 200);
    assert_hints_routable(mood_dialog_hints(), 200);
}
