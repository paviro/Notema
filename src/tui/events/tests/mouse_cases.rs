use super::*;

#[test]
fn wide_journal_click_selects_journal_and_keeps_journal_focus() {
    let mut app = app_with_journals(&["alpha", "beta"]);
    app.nav.focus = Focus::Journals;
    app.nav.selected_entry_index = Some(3);
    app.nav.scroll.reader = 10;
    let layout = render::tui_layout(Rect::new(0, 0, 120, 20), &app);
    let journals = layout.journals.unwrap().content;

    // Row 0 is the leading offset, rows 1-3 the first journal box, so the
    // second journal box starts at row 4.
    mouse_in_area(
        &mut app,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            journals.x,
            journals.y + 4,
        ),
        120,
        20,
    );

    assert_eq!(app.selected_journal_index(), 1);
    // Selecting a journal clears the entry selection so the insights column shows
    // instead of an entry reader.
    assert_eq!(app.nav.selected_entry_index, None);
    assert!(app.show_journal_insights());
    assert_eq!(app.nav.scroll.reader, 0);
    assert_eq!(app.nav.focus, Focus::Journals);
}

#[test]
fn compact_journal_click_moves_to_entries() {
    let mut app = app_with_journals(&["work"]);
    app.nav.focus = Focus::Journals;
    let layout = render::tui_layout(Rect::new(0, 0, 57, 20), &app);
    let journals = layout.journals.unwrap().content;

    mouse_in_area(
        &mut app,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            journals.x,
            journals.y,
        ),
        57,
        20,
    );

    assert_eq!(app.selected_journal_index(), 0);
    assert_eq!(app.nav.focus, Focus::Entries);
}

#[test]
fn journal_panel_click_without_row_focuses_journals_without_changing_selection() {
    let mut app = app_with_journals(&["alpha"]);
    app.nav.focus = Focus::Entries;
    let layout = render::tui_layout(Rect::new(0, 0, 130, 20), &app);
    let journals = layout.journals.unwrap().content;

    mouse_in_area(
        &mut app,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            journals.x,
            journals.y + 4,
        ),
        130,
        20,
    );

    assert_eq!(app.selected_journal_index(), 0);
    assert_eq!(app.nav.focus, Focus::Journals);
}

#[test]
fn wheel_over_journals_scrolls_without_changing_selection() {
    let mut app = app_with_journals(&["a", "b", "c", "d", "e", "f", "g"]);
    app.nav.focus = Focus::Entries;
    let layout = render::tui_layout(Rect::new(0, 0, 130, 8), &app);
    let journals = layout.journals.unwrap().content;

    mouse_in_area(
        &mut app,
        mouse(MouseEventKind::ScrollDown, journals.x, journals.y),
        130,
        8,
    );

    assert_eq!(app.selected_journal_index(), 0);
    // Pixel-row lists scroll two rows per notch (their items are several rows tall).
    assert_eq!(app.nav.journal_list.offset(), 2);
    assert_eq!(app.nav.focus, Focus::Entries);
}

#[test]
fn wheel_over_entries_scrolls_without_changing_selection() {
    let mut app = app_with_entries(8);
    app.nav.focus = Focus::Journals;
    let layout = render::tui_layout(Rect::new(0, 0, 90, 8), &app);
    let entries = layout.entries.unwrap().panel.content;

    mouse_in_area(
        &mut app,
        mouse(MouseEventKind::ScrollDown, entries.x, entries.y),
        90,
        8,
    );

    assert_eq!(app.nav.selected_entry_index, Some(0));
    // Pixel-row lists scroll two rows per notch (their items are several rows tall).
    assert_eq!(app.nav.entry_list.offset(), 2);
    assert_eq!(app.nav.focus, Focus::Journals);
}

#[test]
fn entry_click_selects_row_without_opening_viewer_when_reader_is_visible() {
    let mut app = app_with_entries(2);
    app.nav.focus = Focus::Journals;
    let layout = render::tui_layout(Rect::new(0, 0, 130, 12), &app);
    let geo = layout.entries.unwrap();
    let entries = geo.panel.content;
    let rows = render::entry_row_metadata(&app, geo.text_width);
    let y_off: u16 = rows
        .iter()
        .take_while(|row| row.item_index != Some(1))
        .map(|row| row.height)
        .sum();

    mouse_in_area(
        &mut app,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            entries.x,
            entries.y + y_off,
        ),
        130,
        12,
    );

    assert_eq!(app.nav.focus, Focus::Entries);
    assert_eq!(app.nav.selected_entry_index, Some(1));
}

#[test]
fn entry_panel_month_divider_click_deselects_to_journal_insights() {
    let mut app = app_with_entries(1);
    app.nav.focus = Focus::Reader;
    let layout = render::tui_layout(Rect::new(0, 0, 120, 12), &app);
    let entries = layout.entries.unwrap().panel.content;

    // The top row is the month divider, not an entry.
    mouse_in_area(
        &mut app,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            entries.x,
            entries.y,
        ),
        120,
        12,
    );

    assert_eq!(app.nav.focus, Focus::Entries);
    assert_eq!(app.nav.selected_entry_index, None);
}

#[test]
fn entry_panel_empty_space_click_deselects_to_journal_insights() {
    let mut app = app_with_entries(1);
    app.nav.focus = Focus::Reader;
    let layout = render::tui_layout(Rect::new(0, 0, 130, 20), &app);
    let geo = layout.entries.unwrap();
    let entries = geo.panel.content;
    let rows = render::entry_row_metadata(&app, geo.text_width);
    // First empty row below the (single entry's) list content.
    let total: u16 = rows.iter().map(|row| row.height).sum();

    mouse_in_area(
        &mut app,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            entries.x,
            entries.y + total,
        ),
        130,
        20,
    );

    assert_eq!(app.nav.focus, Focus::Entries);
    assert_eq!(app.nav.selected_entry_index, None);
}

#[test]
fn wheel_over_reader_scrolls_reader_only() {
    let mut app = app_with_entries(6);
    app.nav.focus = Focus::Entries;
    let layout = render::tui_layout(Rect::new(0, 0, 120, 20), &app);
    let reader = layout.reader.unwrap().content;

    mouse_in_area(
        &mut app,
        mouse(MouseEventKind::ScrollDown, reader.x, reader.y),
        120,
        20,
    );

    assert_eq!(app.nav.scroll.reader, 1);
    assert_eq!(app.nav.entry_list.offset(), 0);
    assert_eq!(app.nav.selected_entry_index, Some(0));
    assert_eq!(app.nav.focus, Focus::Reader);
}

#[test]
fn expanded_entry_wheel_scrolls_and_clicks_do_not_close() {
    let mut app = app_with_entries(1);
    view_selected(&mut app).unwrap();

    mouse_in_area(&mut app, mouse(MouseEventKind::ScrollDown, 1, 1), 80, 20);
    assert_eq!(app.nav.scroll.reader, 1);

    mouse_in_area(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), 1, 1),
        80,
        20,
    );
    assert_eq!(app.nav.focus, Focus::Reader);
}

#[test]
fn multi_col_fullscreen_body_click_does_not_collapse() {
    let mut app = app_with_entries(1);
    view_selected(&mut app).unwrap();
    app.nav.reader_fullscreen = true;

    // A click inside the full-screen body (not on a metadata chip) must leave the
    // viewer expanded rather than collapsing it back to the pane.
    mouse_in_area(
        &mut app,
        mouse(MouseEventKind::Down(MouseButton::Left), 5, 10),
        130,
        20,
    );

    assert_eq!(app.nav.focus, Focus::Reader);
    assert!(app.nav.reader_fullscreen);
}

#[test]
fn metadata_refresh_restores_expanded_reader_and_scroll() {
    let mut app = app_with_entries(1);
    view_selected(&mut app).unwrap();
    app.nav.scroll.reader = 7;

    let snapshot = ReaderSnapshot::capture(&app);
    app.begin_edit_tags();
    super::actions::set_metadata_on_entry(
        &mut app,
        crate::tui::state::MetadataKind::Tags,
        &["work".to_string()],
    )
    .unwrap();
    restore_reader_or_close(&mut app, snapshot);
    app.close_overlay();

    assert_eq!(app.nav.focus, Focus::Reader);
    assert_eq!(app.nav.scroll.reader, 7);
    assert_eq!(app.selected_entry_tags(), vec!["work".to_string()]);
    assert!(!app.has_overlay());
}

#[test]
fn confirmed_delete_from_expanded_entry_closes_viewer() {
    let mut app = app_with_entries(1);
    view_selected(&mut app).unwrap();
    app.nav.scroll.reader = 5;
    app.begin_confirm_delete();

    assert_eq!(app.nav.focus, Focus::Reader);

    confirm_delete(&mut app).unwrap();

    assert_eq!(app.nav.focus, Focus::Entries);
    assert_eq!(app.nav.scroll.reader, 0);
    assert_eq!(app.current_entry_list_len(), 0);
    assert!(!app.has_overlay());
}

#[test]
fn search_from_reader_resets_focus_and_scroll() {
    let mut app = app_with_entries(1);
    view_selected(&mut app).unwrap();
    app.nav.scroll.reader = 5;

    app.begin_search();

    assert_eq!(app.nav.focus, Focus::Entries);
    assert_eq!(app.nav.mode, crate::tui::app::Mode::Search);
    assert_eq!(app.nav.scroll.reader, 0);
}

#[test]
fn select_created_entry_path_opens_expanded_reader() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let entry_dir = root.join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n+++\n\n# Existing\nBody\n",
    )
    .unwrap();

    let config = Config::new(root.clone());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    view_selected(&mut app).unwrap();
    app.nav.scroll.reader = 9;

    let store = JournalStore::for_config(&root.join("config.toml"), &root).unwrap();
    let created = store
        .create_entry(
            notema_storage::EntryDraft::new(
                "work",
                "# Created\nBody\n",
                &notema_domain::Metadata::default(),
            ),
            notema_storage::EntryAssetOptions::default(),
        )
        .unwrap()
        .path;
    app.refresh().unwrap();
    let created_id = notema_storage::entry_id(&created).unwrap();
    assert!(app.select_entry_by_id(&created_id, true));
    app.nav.focus = Focus::Reader;

    assert_eq!(app.nav.focus, Focus::Reader);
    assert_eq!(app.nav.scroll.reader, 0);
    assert_eq!(app.selected_entry_target().unwrap().path, created);
}

#[test]
fn wheel_over_tag_dialog_list_scrolls_without_selection_or_toggle_change() {
    let mut app = app_with_entries(1);
    app.begin_edit_tags();
    set_tag_dialog_items(&mut app, 20);
    let layout =
        render::metadata_dialog_layout(&app.appearance.theme, Rect::new(0, 0, 120, 20), 20);

    mouse_in_area(
        &mut app,
        mouse(MouseEventKind::ScrollDown, layout.list.x, layout.list.y),
        120,
        20,
    );

    let state = app.edit_metadata_state().unwrap();
    assert_eq!(state.offset(), 1);
    assert_eq!(state.selected_index(), Some(0));
    assert!(state.selected.is_empty());
}

#[test]
fn click_on_tag_dialog_row_selects_and_toggles_it() {
    let mut app = app_with_entries(1);
    app.begin_edit_tags();
    set_tag_dialog_items(&mut app, 5);
    let layout = render::metadata_dialog_layout(&app.appearance.theme, Rect::new(0, 0, 120, 20), 5);

    mouse_in_area(
        &mut app,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            layout.list.x,
            layout.list.y + 2,
        ),
        120,
        20,
    );

    let state = app.edit_metadata_state().unwrap();
    assert_eq!(state.selected_index(), Some(2));
    assert_eq!(state.selected, vec!["tag-02"]);
}

#[test]
fn click_on_tag_dialog_placeholder_row_does_not_toggle() {
    let mut app = app_with_entries(1);
    app.begin_edit_tags();
    let state = app.edit_metadata_state_mut().unwrap();
    state.all_values = vec![("work".to_string(), 1)];
    state.filtered.clear();
    state.input = "missing".into();
    state.normalize_list_state();
    let layout = render::metadata_dialog_layout(&app.appearance.theme, Rect::new(0, 0, 120, 12), 0);

    mouse_in_area(
        &mut app,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            layout.list.x,
            layout.list.y,
        ),
        120,
        12,
    );

    let state = app.edit_metadata_state().unwrap();
    assert_eq!(state.selected_index(), None);
    assert!(state.selected.is_empty());
}

#[test]
fn click_on_tag_input_row_switches_focus_to_input() {
    let mut app = app_with_entries(1);
    app.begin_edit_tags();
    set_tag_dialog_items(&mut app, 3);
    let layout = render::metadata_dialog_layout(&app.appearance.theme, Rect::new(0, 0, 120, 16), 3);

    mouse_in_area(
        &mut app,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            layout.input.x,
            layout.input.y,
        ),
        120,
        16,
    );

    assert_eq!(
        app.edit_metadata_state().unwrap().focus,
        EditMetadataFocus::Input
    );
}

#[test]
fn click_on_feeling_dialog_header_expands_then_feeling_toggles() {
    let mut app = app_with_entries(1);
    app.begin_edit_feelings();

    let feelings_layout = |app: &AppModel| {
        let state = app.edit_feeling_state().unwrap();
        let all_len = state.item_count();
        render::feelings_dialog_layout(
            &app.appearance.theme,
            Rect::new(0, 0, 120, 20),
            all_len,
            &state.selected,
        )
    };

    // Clicking the first (header) row folds that group open.
    let layout = feelings_layout(&app);
    mouse_in_area(
        &mut app,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            layout.list.x,
            layout.list.y,
        ),
        120,
        20,
    );
    assert!(app.edit_feeling_state().unwrap().expanded[0]);

    // The first feeling now sits directly below the header; clicking it selects it.
    let layout = feelings_layout(&app);
    mouse_in_area(
        &mut app,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            layout.list.x,
            layout.list.y + 1,
        ),
        120,
        20,
    );

    let state = app.edit_feeling_state().unwrap();
    let FeelingRow::Feeling { group, feeling } = state.visible_rows()[1] else {
        panic!("row 1 should be a feeling");
    };
    let word = state.groups[group].feelings[feeling].name;
    assert_eq!(state.selected, vec![word.to_string()]);
}

#[test]
fn click_and_drag_on_mood_bar_set_nearest_scores() {
    let mut app = app_with_entries(1);
    app.begin_edit_mood();
    let layout = render::mood_dialog_layout(&app.appearance.theme, Rect::new(0, 0, 120, 20));

    mouse_in_area(
        &mut app,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            layout.bar.x,
            layout.bar.y,
        ),
        120,
        20,
    );
    assert_eq!(app.edit_mood_state().unwrap().draft, -5);

    mouse_in_area(
        &mut app,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            layout.bar.x + layout.bar.width / 2,
            layout.bar.y,
        ),
        120,
        20,
    );
    assert_eq!(app.edit_mood_state().unwrap().draft, 0);

    mouse_in_area(
        &mut app,
        mouse(
            MouseEventKind::Drag(MouseButton::Left),
            layout.bar.x + layout.bar.width - 1,
            layout.bar.y,
        ),
        120,
        20,
    );
    assert_eq!(app.edit_mood_state().unwrap().draft, 5);
}

/// The entry-list scrollbar geometry for a 60-entry list in a 120×20 area.
struct EntryBarFixture {
    app: AppModel,
    area: Rect,
    bar: Rect,
    total: usize,
    viewport: u16,
    max: usize,
}

fn entry_bar_fixture() -> EntryBarFixture {
    let app = app_with_entries(60);
    let entries = render::tui_layout(Rect::new(0, 0, 120, 20), &app)
        .entries
        .expect("entries panel");
    let area = entries.panel.area;
    let cache = app.entry_rows(entries.text_width);
    let total = cache.total_height;
    let viewport = entries.viewport_height;
    let max = total.saturating_sub(viewport as usize);
    assert!(max > 0, "entry list should overflow so a bar is drawn");
    EntryBarFixture {
        bar: scroll::scrollbar_bar_rect(&app.appearance.theme, area),
        app,
        area,
        total,
        viewport,
        max,
    }
}

#[test]
fn scrollbar_arrows_step_one_line_without_dragging() {
    let EntryBarFixture {
        mut app, bar, max, ..
    } = entry_bar_fixture();
    let up_arrow = bar.y;
    let down_arrow = bar.y + bar.height - 1;

    // The down arrow steps one line down; no drag begins.
    mouse_in_area(&mut app, mouse(down(), bar.x, down_arrow), 120, 20);
    assert_eq!(app.nav.entry_list.offset(), 1);
    assert!(app.scrollbar.active.is_none());
    assert_eq!(app.nav.focus, Focus::Entries);

    // The up arrow steps back.
    mouse_in_area(&mut app, mouse(down(), bar.x, up_arrow), 120, 20);
    assert_eq!(app.nav.entry_list.offset(), 0);
    assert!(app.scrollbar.active.is_none());
    assert!(max > 1);
}

#[test]
fn scrollbar_thumb_press_grabs_without_jumping() {
    let EntryBarFixture {
        mut app,
        bar,
        total,
        viewport,
        max,
        ..
    } = entry_bar_fixture();
    *app.nav.entry_list.offset_mut() = max / 2;
    let before = app.nav.entry_list.offset();

    let position = scroll::scrollbar_position(before, total, viewport);
    let (thumb_top, thumb_len) =
        scroll::scrollbar_thumb(bar, total, viewport, position).expect("thumb");

    // Pressing straight on the thumb grabs it and leaves the scroll untouched.
    mouse_in_area(
        &mut app,
        mouse(down(), bar.x, thumb_top + thumb_len / 2),
        120,
        20,
    );
    assert_eq!(app.nav.entry_list.offset(), before);
    assert_eq!(app.scrollbar.active, Some(ScrollbarDrag::EntryList));
}

#[test]
fn scrollbar_track_press_jumps_then_drag_tracks_the_cursor() {
    let EntryBarFixture {
        mut app,
        area,
        bar,
        max,
        ..
    } = entry_bar_fixture();
    let bottom_track = bar.y + bar.height - 2; // last track row, above the down arrow
    let top_track = bar.y + 1; // first track row, below the up arrow

    // Press empty track near the bottom → thumb jumps down under the cursor.
    mouse_in_area(&mut app, mouse(down(), bar.x, bottom_track), 120, 20);
    assert_eq!(app.scrollbar.active, Some(ScrollbarDrag::EntryList));
    assert!(
        app.nav.entry_list.offset() > max / 2,
        "expected a large jump, got {}",
        app.nav.entry_list.offset()
    );

    // Drag to the top, cursor drifted off the bar column → scroll to 0.
    mouse_in_area(&mut app, mouse(drag(), 0, top_track), 120, 20);
    assert_eq!(app.nav.entry_list.offset(), 0);

    // Release clears the drag.
    mouse_in_area(&mut app, mouse(up(), 0, top_track), 120, 20);
    assert!(app.scrollbar.active.is_none());

    // The grab region spans the bar column plus one on each side.
    for col in [bar.x - 1, bar.x + 1] {
        assert!(col >= area.x && col < area.x + area.width + 1);
        mouse_in_area(&mut app, mouse(down(), col, bottom_track), 120, 20);
        assert_eq!(app.scrollbar.active, Some(ScrollbarDrag::EntryList));
        mouse_in_area(&mut app, mouse(up(), col, bottom_track), 120, 20);
    }
}

#[test]
fn reader_scrollbar_press_and_drag_scroll_the_reader() {
    let mut app = app_with_entry();
    app.library.entries[0].body = (0..80).map(|i| format!("line {i}\n")).collect();
    app.select_entry_index(0);
    app.nav.focus = Focus::Reader;
    let reader = render::tui_layout(Rect::new(0, 0, 140, 20), &app)
        .reader
        .expect("reader panel");
    let bar = scroll::scrollbar_bar_rect(&app.appearance.theme, reader.area);

    // Press the bottom track row → the reader scroll jumps and the drag arms.
    mouse_in_area(
        &mut app,
        mouse(down(), bar.x, bar.y + bar.height - 2),
        140,
        20,
    );
    assert_eq!(app.scrollbar.active, Some(ScrollbarDrag::Reader));
    assert!(app.nav.scroll.reader > 0);

    // Drag to the top with the cursor drifted off the bar column → back to the start.
    mouse_in_area(&mut app, mouse(drag(), 0, bar.y + 1), 140, 20);
    assert_eq!(app.nav.scroll.reader, 0);

    // Release clears the drag.
    mouse_in_area(&mut app, mouse(up(), 0, bar.y + 1), 140, 20);
    assert!(app.scrollbar.active.is_none());
}

#[test]
fn scrollbar_track_press_scrolls_journals() {
    let names: Vec<String> = (0..60).map(|i| format!("journal-{i:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut app = app_with_journals(&refs);
    let journals = render::tui_layout(Rect::new(0, 0, 120, 20), &app)
        .journals
        .expect("journals panel");
    let bar = scroll::scrollbar_bar_rect(&app.appearance.theme, journals.area);
    // The journal list uses the same pixel-row model as entries: each box is
    // JOURNAL_BOX_HEIGHT tall, so the total content height is journals × that.
    let list_area = render::journal_list_rect(journals.content);
    let total_height = app.library.journals.len() * render::JOURNAL_BOX_HEIGHT as usize;
    let max = total_height.saturating_sub(list_area.height as usize);
    assert!(max > 0, "journals list should overflow so a bar is drawn");

    // Press the bottom track row → thumb jumps down.
    mouse_in_area(
        &mut app,
        mouse(down(), bar.x, bar.y + bar.height - 2),
        120,
        20,
    );
    assert_eq!(app.scrollbar.active, Some(ScrollbarDrag::Journals));
    assert!(app.nav.journal_list.offset() > 0);
}

// ── Scroll-burst coalescing ────────────────────────────────────────────────

fn wheel_event(kind: MouseEventKind) -> Event {
    Event::Mouse(mouse(kind, 0, 0))
}

#[test]
fn fold_leading_wheel_nets_opposing_scrolls() {
    let up = wheel_event(MouseEventKind::ScrollUp);
    let down = wheel_event(MouseEventKind::ScrollDown);
    // Five up + two down → net -3, all seven consumed.
    let events = vec![
        up.clone(),
        up.clone(),
        up.clone(),
        up.clone(),
        up,
        down.clone(),
        down,
    ];
    assert_eq!(fold_leading_wheel(&events), (-3, 7));
}

#[test]
fn fold_leading_wheel_stops_at_first_non_wheel() {
    let down = wheel_event(MouseEventKind::ScrollDown);
    let key = Event::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::empty()));
    // Only the leading wheel run is folded; the key stays for later handling.
    let events = vec![
        down.clone(),
        down,
        key,
        wheel_event(MouseEventKind::ScrollUp),
    ];
    assert_eq!(fold_leading_wheel(&events), (2, 2));
}

#[test]
fn fold_leading_wheel_edge_cases() {
    assert_eq!(fold_leading_wheel(&[]), (0, 0));
    let single = vec![wheel_event(MouseEventKind::ScrollUp)];
    assert_eq!(fold_leading_wheel(&single), (-1, 1));
    // A leading non-wheel event consumes nothing.
    let click = vec![Event::Mouse(mouse(down(), 0, 0))];
    assert_eq!(fold_leading_wheel(&click), (0, 0));
}
