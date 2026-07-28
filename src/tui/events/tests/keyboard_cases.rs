use super::*;

#[test]
fn action_errors_become_toasts_and_keep_the_event_loop_running() {
    let dir = tempdir().unwrap();
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);

    let outcome = recover_action_error(&mut app, Err(anyhow::anyhow!("write failed"))).unwrap();

    assert_eq!(outcome, DispatchOutcome::Continue);
    let toast = app.toasts.items().last().unwrap();
    assert_eq!(toast.variant, crate::tui::state::ToastVariant::Error);
    assert_eq!(toast.message, "Action failed: write failed");
}

#[test]
fn external_links_are_returned_as_runtime_effects() {
    let mut app = app_with_journals(&["work"]);

    assert_eq!(
        actions::open_reader_link(&mut app, "https://example.com", None).unwrap(),
        Some(Effect::Open {
            target: OpenTarget::Uri("https://example.com".to_string()),
            success_message: "Opened link".to_string(),
        })
    );
    assert!(app.toasts.items().is_empty());
}

#[test]
fn geocoding_starts_only_when_the_runtime_executes_its_effect() {
    let mut app = app_with_entries(1);
    app.begin_edit_location();
    app.edit_location_state_mut()
        .unwrap()
        .query
        .set_text("Berlin");
    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    let outcome = dispatch_action(
        &mut terminal,
        &mut app,
        Action::Location(LocationAction::Resolve),
    )
    .unwrap();

    assert!(matches!(outcome.effects.as_slice(), [Effect::Geocode(_)]));
    assert!(!app.geocode.has_pending());
}

#[test]
fn external_open_failures_become_error_toasts() {
    let mut app = app_with_journals(&["work"]);

    apply_background_action(
        &mut app,
        BackgroundAction::ExternalOpenFailed("no handler".to_string()),
    );

    let toast = app.toasts.items().last().expect("error toast");
    assert_eq!(toast.variant, crate::tui::state::ToastVariant::Error);
    assert_eq!(toast.message, "Couldn't open link: no handler");
}

#[test]
fn a_failed_watcher_registration_names_the_reload_it_costs() {
    for (target, expected) in [
        (
            WatchTarget::Journal,
            "Live journal reload unavailable: oops",
        ),
        (WatchTarget::Theme, "Live theme reload unavailable: oops"),
    ] {
        let mut app = app_with_journals(&["work"]);

        let outcome = apply_background_action(
            &mut app,
            BackgroundAction::WatcherUnavailable {
                target,
                error: "oops".to_string(),
            },
        );

        assert!(outcome.redraw);
        let toast = app.toasts.items().last().expect("warning toast");
        assert_eq!(toast.variant, crate::tui::state::ToastVariant::Warning);
        assert_eq!(toast.message, expected);
    }
}

/// The whole point of the journal-list path: none of these walk the corpus, so
/// none of them may end up queueing a reload behind the user's back.
#[test]
fn journal_bookkeeping_never_asks_for_a_corpus_walk() {
    let mut app = app_with_journals(&["work"]);

    app.begin_new_journal_input();
    app.new_journal_input_mut().unwrap().set_text("personal");
    actions::submit_new_journal(&mut app).unwrap();

    assert!(
        app.library
            .journals
            .iter()
            .any(|journal| journal.name == "personal")
    );
    assert!(!app.library_reload.has_pending());

    app.select_journal_by_name("personal");
    actions::toggle_archive_selected_journal(&mut app).unwrap();

    assert!(
        app.library
            .journals
            .iter()
            .any(|journal| journal.name == "personal.archived")
    );
    assert!(!app.library_reload.has_pending());
}

#[test]
fn a_watcher_that_lost_track_asks_for_a_quiet_reload() {
    let mut app = app_with_journals(&["work"]);

    let outcome = apply_background_action(&mut app, BackgroundAction::WatcherLostTrack);

    assert!(app.library_reload.has_pending());
    // Nothing for the user to read: the reload repaints when it lands.
    assert!(!outcome.redraw);
    assert!(app.toasts.items().is_empty());
}

#[test]
fn browse_r_maps_to_manual_library_refresh() {
    let dir = tempdir().unwrap();
    let app = new_app(Config::new(dir.path().to_path_buf()));

    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('r')), true),
        Some(Action::ReloadLibrary { rebuild: false })
    );
    // Shifted, the expensive one: the cache rebuild is a separate gesture so the
    // routine refresh never pays a full re-read.
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('R')), true),
        Some(Action::ReloadLibrary { rebuild: true })
    );
}

#[test]
fn search_key_only_fires_where_its_scope_is_clear() {
    let mut app = app_with_entries(1);

    app.nav.focus = Focus::Journals;
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('/')), true),
        Some(Action::Search(SearchAction::Begin))
    );

    app.nav.focus = Focus::Entries;
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('/')), true),
        Some(Action::Search(SearchAction::Begin))
    );

    // The reader and insights columns have no obvious search target, so `/` is
    // inert there.
    app.nav.focus = Focus::Reader;
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('/')), true),
        None
    );

    app.nav.focus = Focus::Insights;
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('/')), true),
        None
    );
}

#[test]
fn enter_on_journals_moves_to_entries_like_right_arrow() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("work")).unwrap();
    let config = Config::new(dir.path().to_path_buf());
    let mut enter_app = new_app(config.clone());
    let mut right_app = new_app(config);

    enter_app.nav.focus = Focus::Journals;
    right_app.nav.focus = Focus::Journals;

    // Enter and Right on Journals both resolve to move_focus_right
    enter_app.move_focus_right(true);
    right_app.move_focus_right(true);

    assert_eq!(enter_app.nav.focus, Focus::Entries);
    assert_eq!(enter_app.nav.focus, right_app.nav.focus);
}

#[test]
fn right_on_entry_expands_when_inline_reader_is_hidden() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n+++\n\n# A\nBody\n",
    )
    .unwrap();
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    app.nav.focus = Focus::Entries;

    // Right on Entries when not reader_available → ViewSelected → view_selected
    view_selected(&mut app).unwrap();

    assert_eq!(app.nav.focus, Focus::Reader);
}

#[test]
fn expanded_entry_title_matches_reader_timestamp_title() {
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
    app.nav.focus = Focus::Entries;

    view_selected(&mut app).unwrap();

    let (title, _) = app.selected_reader().unwrap();
    assert_eq!(title, "Wednesday, 1 July 2026, 10:23");
}

#[test]
fn right_on_entry_focuses_reader_when_reader_is_available() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n+++\n\n# A\nBody\n",
    )
    .unwrap();
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    app.nav.focus = Focus::Entries;

    // Right on Entries when reader_available → FocusRight → focus to Reader
    app.move_focus_right(true);

    assert_eq!(app.nav.focus, Focus::Reader);
}

#[test]
fn keyboard_and_footer_edit_use_the_same_action() {
    let mut app = app_with_entries(1);
    app.nav.focus = Focus::Entries;

    let key_action = keyboard::key_to_action(&app, key(KeyCode::Char('e')), true);
    let hint_action = mouse::hint_id_to_action(&app, render::HintId::EditSelected);

    assert_eq!(
        key_action,
        Some(Action::Browser(BrowserAction::EditSelected))
    );
    assert_eq!(
        hint_action,
        Some(Action::Browser(BrowserAction::EditSelected))
    );
}

#[test]
fn editor_footer_hints_route_to_editor_actions() {
    let mut app = app_with_entries(1);
    app.open_editor_for_selected().unwrap();
    app.state.ui.show_hints = false;

    assert_eq!(
        mouse::hint_id_to_action(&app, render::HintId::EditorSave),
        Some(Action::Editor(EditorAction::Save))
    );
    assert_eq!(
        mouse::hint_id_to_action(&app, render::HintId::EditorDiscard),
        Some(Action::Editor(EditorAction::RequestDiscard))
    );
    assert_eq!(
        mouse::hint_id_to_action(&app, render::HintId::EditorMetadata),
        Some(Action::Editor(EditorAction::OpenMetadataMenu))
    );
}

#[test]
fn right_past_entries_focuses_insights_and_arrows_cycle_tabs() {
    let mut app = app_with_entries(1);
    app.nav.focus = Focus::Entries;
    // No entry selected → the right column is the insights reader.
    app.nav.selected_entry_index = None;
    assert!(app.show_journal_insights());

    // Right past Entries focuses the panel on its first tab.
    app.move_focus_right(true);
    assert_eq!(app.nav.focus, Focus::Insights);
    assert_eq!(app.nav.insights_tab, InsightsTab::Overview);
    assert!(app.insights_panel_focused());

    // Right steps forward through the tabs without leaving the panel.
    app.move_focus_right(true);
    assert_eq!(app.nav.focus, Focus::Insights);
    assert_eq!(app.nav.insights_tab, InsightsTab::Writing);

    // Left steps back; from the first tab it leaves to the entries column.
    app.move_focus_left();
    assert_eq!(
        (app.nav.focus, app.nav.insights_tab),
        (Focus::Insights, InsightsTab::Overview)
    );
    app.move_focus_left();
    assert_eq!(app.nav.focus, Focus::Entries);
}

#[test]
fn right_reaches_insights_in_single_panel_layout() {
    let mut app = app_with_entries(1);
    app.nav.focus = Focus::Entries;
    // No entry selected → the entries column shows the journal insights.
    app.nav.selected_entry_index = None;
    assert!(app.show_journal_insights());

    // At single-panel width (entry view unavailable) Right still focuses the panel,
    // which renders full-screen; Left from the first tab returns to the entries list.
    app.move_focus_right(false);
    assert_eq!(app.nav.focus, Focus::Insights);
    assert_eq!(app.nav.insights_tab, InsightsTab::Overview);

    app.move_focus_left();
    assert_eq!(app.nav.focus, Focus::Entries);
}

#[test]
fn enter_expands_and_collapses_the_insights_panel() {
    let mut app = app_with_entries(1);
    app.nav.selected_entry_index = None;
    app.nav.focus = Focus::Insights;

    // Enter on the focused panel expands it; Enter/Esc collapse it back.
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Enter), true),
        Some(Action::Insights(InsightsAction::SetFullscreen(true)))
    );
    app.nav.insights_fullscreen = true;
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Enter), true),
        Some(Action::Insights(InsightsAction::SetFullscreen(false)))
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Esc), true),
        Some(Action::Insights(InsightsAction::SetFullscreen(false)))
    );

    // Leaving the panel (Left from the first tab) resets full-screen so it
    // re-opens collapsed.
    app.move_focus_left();
    assert_eq!(app.nav.focus, Focus::Entries);
    assert!(!app.nav.insights_fullscreen);
}

#[test]
fn scope_key_toggles_only_while_insights_panel_is_focused() {
    let mut app = app_with_entries(1);
    app.nav.focus = Focus::Journals;
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('g')), true),
        None
    );

    app.nav.focus = Focus::Insights;
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('g')), true),
        Some(Action::Insights(InsightsAction::ToggleScope))
    );
}

#[test]
fn window_key_cycles_timeframe_only_on_driver_tabs() {
    let mut app = app_with_entries(1);
    app.nav.focus = Focus::Insights;

    // Overview doesn't window, so `w` is inert there.
    app.nav.insights_tab = InsightsTab::Overview;
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('w')), true),
        None
    );

    // On Drivers it cycles the rolling window forward, wrapping back to Overall.
    app.nav.insights_tab = InsightsTab::Drivers;
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('w')), true),
        Some(Action::Insights(InsightsAction::CycleTimeframe))
    );
    assert_eq!(InsightsTimeframe::Overall.next(), InsightsTimeframe::Year);
    assert_eq!(InsightsTimeframe::Week.next(), InsightsTimeframe::Overall);
}

#[test]
fn clicking_a_border_tab_focuses_the_panel_and_selects_that_tab() {
    let mut app = app_with_entries(1);
    // Reader state so the insights panel is the right-hand column.
    app.nav.selected_entry_index = None;
    app.nav.focus = Focus::Journals;

    // Click the "Drivers" label in the insights panel's top border. At width 160 the
    // panel is wide enough for full titles; Drivers is the fourth (last) tab, at x≈117.
    mouse_in_area(&mut app, mouse(down(), 117, 0), 160, 20);

    assert_eq!(app.nav.focus, Focus::Insights);
    assert_eq!(app.nav.insights_tab, InsightsTab::Drivers);
}

#[test]
fn multi_col_enter_focuses_then_expands_then_collapses() {
    let mut app = app_with_entries(1);

    // First Enter opens the focused reader pane (not full screen yet).
    view_selected(&mut app).unwrap();
    assert_eq!(app.nav.focus, Focus::Reader);
    assert!(!app.nav.reader_fullscreen);

    // Second Enter expands to full screen.
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Enter), true),
        Some(Action::Reader(ReaderAction::SetFullscreen(true)))
    );
    app.nav.reader_fullscreen = true;

    // Third Enter closes full screen (collapses back to the focused pane).
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Enter), true),
        Some(Action::Reader(ReaderAction::SetFullscreen(false)))
    );
}

#[test]
fn multi_col_fullscreen_esc_collapses_and_left_is_inert() {
    let mut app = app_with_entries(1);
    view_selected(&mut app).unwrap();
    app.nav.reader_fullscreen = true;

    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Esc), true),
        Some(Action::Reader(ReaderAction::SetFullscreen(false)))
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Left), true),
        None
    );
}

#[test]
fn single_col_viewer_exits_on_enter_esc_and_left() {
    let mut app = app_with_entries(1);
    view_selected(&mut app).unwrap();

    // In single-column the viewer is full screen by nature; Enter/Esc/Left all exit.
    for code in [KeyCode::Enter, KeyCode::Esc, KeyCode::Left] {
        assert_eq!(
            keyboard::key_to_action(&app, key(code), false),
            Some(Action::Browser(BrowserAction::FocusLeft)),
            "{code:?}"
        );
    }
}

#[test]
fn leaving_the_viewer_clears_fullscreen() {
    let mut app = app_with_entries(1);
    view_selected(&mut app).unwrap();
    app.nav.reader_fullscreen = true;

    app.move_focus_left();

    assert_eq!(app.nav.focus, Focus::Entries);
    assert!(!app.nav.reader_fullscreen);
}

#[test]
fn browse_l_opens_the_location_dialog() {
    let mut app = app_with_entries(1);
    app.nav.focus = Focus::Entries;
    app.nav.selected_entry_index = Some(0);

    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('l')), true),
        Some(Action::Location(LocationAction::BeginEdit))
    );
}

#[test]
fn location_dialog_keys_route_by_focus() {
    let mut app = app_with_entries(1);
    app.nav.focus = Focus::Entries;
    app.nav.selected_entry_index = Some(0);
    app.begin_edit_location();

    // Opens focused on the address field (top): chars type in, and Enter looks
    // the query up (nothing resolved yet).
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('x')), true),
        Some(Action::Overlay(OverlayAction::InputKey(key(
            KeyCode::Char('x')
        ))))
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Tab), true),
        Some(Action::Location(LocationAction::SwitchFocus))
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Esc), true),
        Some(Action::Overlay(OverlayAction::Cancel))
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Enter), true),
        Some(Action::Location(LocationAction::Resolve))
    );

    // With a preset present, focus can reach the list (Query → Name → List),
    // where Enter picks a row.
    {
        let state = app.edit_location_state_mut().unwrap();
        state.presets.push(LocationPreset {
            label: "Berlin".to_string(),
            location: notema_domain::Location {
                city: Some("Berlin".to_string()),
                ..Default::default()
            },
        });
        state.switch_focus(); // Query -> Name
        state.switch_focus(); // Name -> List
        assert_eq!(
            state.focus,
            crate::tui::features::location::EditLocationFocus::List
        );
    }
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Enter), true),
        Some(Action::Location(LocationAction::SelectRow))
    );
}

#[test]
fn location_ctrl_l_grabs_device_and_plain_l_types() {
    let mut app = app_with_entries(1);
    app.nav.focus = Focus::Entries;
    app.nav.selected_entry_index = Some(0);
    app.begin_edit_location();

    // Ctrl+L grabs the device's current location from any focus...
    let ctrl_l = KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL);
    assert_eq!(
        keyboard::key_to_action(&app, ctrl_l, true),
        Some(Action::Location(LocationAction::GrabDevice))
    );
    // ...but a bare 'l' is still text typed into the query field.
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('l')), true),
        Some(Action::Overlay(OverlayAction::InputKey(key(
            KeyCode::Char('l')
        ))))
    );
}

#[test]
fn location_query_enter_saves_once_the_query_is_resolved() {
    let mut app = app_with_entries(1);
    app.nav.focus = Focus::Entries;
    app.nav.selected_entry_index = Some(0);
    app.begin_edit_location();
    {
        let state = app.edit_location_state_mut().unwrap();
        state.focus = crate::tui::features::location::EditLocationFocus::Query;
        state.query = "52.5, 13.4".into();
        state.query_looked_up = false;
    }

    // Before a lookup, Enter in the address field queries.
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Enter), true),
        Some(Action::Location(LocationAction::Resolve))
    );

    // Once resolved, Enter saves instead of re-querying.
    app.edit_location_state_mut().unwrap().query_looked_up = true;
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Enter), true),
        Some(Action::Location(LocationAction::Save))
    );
}

#[test]
fn snapshot_restores_fullscreen_across_an_edit() {
    let mut app = app_with_entries(1);
    view_selected(&mut app).unwrap();
    app.nav.reader_fullscreen = true;

    let snapshot = ReaderSnapshot::capture(&app);
    app.nav.reader_fullscreen = false;
    app.nav.focus = Focus::Entries;
    restore_reader_or_close(&mut app, snapshot);

    assert_eq!(app.nav.focus, Focus::Reader);
    assert!(app.nav.reader_fullscreen);
}

#[test]
fn typed_hint_ids_route_to_actions_without_string_parsing() {
    let mut app = app_with_entries(1);
    app.nav.focus = Focus::Entries;

    assert_eq!(
        mouse::hint_id_to_action(&app, render::HintId::EditTags),
        Some(Action::Metadata(MetadataAction::BeginEdit(
            crate::tui::state::MetadataKind::Tags,
        )))
    );
    assert_eq!(
        mouse::hint_id_to_action(&app, render::HintId::EditPeople),
        Some(Action::Metadata(MetadataAction::BeginEdit(
            crate::tui::state::MetadataKind::People,
        )))
    );
    assert_eq!(
        mouse::hint_id_to_action(&app, render::HintId::ToggleStarred),
        Some(Action::Browser(BrowserAction::ToggleStarred))
    );
    assert_eq!(
        mouse::hint_id_to_action(&app, render::HintId::EditSelected),
        Some(Action::Browser(BrowserAction::EditSelected))
    );
    assert_eq!(
        mouse::hint_id_to_action(&app, render::HintId::MetadataToggle),
        None
    );

    app.begin_edit_tags();
    if let Some(state) = app.edit_metadata_state_mut() {
        state.all_values.push(("work".to_string(), 1));
        state.filtered.push(0);
    }
    assert_eq!(
        mouse::hint_id_to_action(&app, render::HintId::MetadataToggle),
        Some(Action::Metadata(MetadataAction::Toggle))
    );
    assert_eq!(
        mouse::hint_id_to_action(&app, render::HintId::MetadataSave),
        Some(Action::Metadata(MetadataAction::Save))
    );
    assert_eq!(
        mouse::hint_id_to_action(&app, render::HintId::CancelOverlay),
        Some(Action::Overlay(OverlayAction::Cancel))
    );

    // Location hints route to their identically-named actions.
    assert_eq!(
        mouse::hint_id_to_action(&app, render::HintId::LocationResolve),
        Some(Action::Location(LocationAction::Resolve))
    );
    assert_eq!(
        mouse::hint_id_to_action(&app, render::HintId::LocationSelectRow),
        Some(Action::Location(LocationAction::SelectRow))
    );
    assert_eq!(
        mouse::hint_id_to_action(&app, render::HintId::LocationSave),
        Some(Action::Location(LocationAction::Save))
    );
}

#[test]
fn enter_in_metadata_input_saves_when_input_is_empty() {
    let mut app = app_with_entries(1);
    app.begin_edit_tags();
    let state = app.edit_metadata_state_mut().unwrap();
    state.focus = EditMetadataFocus::Input;
    state.input.clear();

    assert_eq!(
        keyboard::key_to_action(
            &app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            true
        ),
        Some(Action::Metadata(MetadataAction::Save))
    );

    app.edit_metadata_state_mut().unwrap().input = "rust".into();
    assert_eq!(
        keyboard::key_to_action(
            &app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            true
        ),
        Some(Action::Metadata(MetadataAction::AddFromInput))
    );
}

#[test]
fn arrows_in_metadata_input_move_the_caret_for_mid_string_edits() {
    let mut app = app_with_entries(1);
    app.begin_edit_tags();
    let state = app.edit_metadata_state_mut().unwrap();
    state.focus = EditMetadataFocus::Input;
    state.input = "rst".into();

    // Left in the focused input routes to the field like any editing key...
    assert_eq!(
        keyboard::key_to_action(
            &app,
            KeyEvent::new(KeyCode::Left, KeyModifiers::empty()),
            true
        ),
        Some(Action::Overlay(OverlayAction::InputKey(key(KeyCode::Left))))
    );

    // ...which resolves to this dialog's input and edits at the caret.
    let input = app.focused_text_input_mut().unwrap();
    input.input(key(KeyCode::Left));
    input.input(key(KeyCode::Left));
    input.input(key(KeyCode::Char('u')));
    assert_eq!(
        app.edit_metadata_state().unwrap().input.as_str(),
        "rust",
        "insert lands at the caret, not the end"
    );
}

/// The suggestion list draws over the results, so it has to be scrupulous about
/// which keys it takes. With nothing highlighted it takes none of the ones that
/// already meant something: `Enter` opens the selected result and `Up` moves the
/// entry list, so a query typed to narrow by substring behaves as it always did.
#[test]
fn the_suggestion_list_claims_no_key_until_it_is_arrowed_into() {
    let mut app = app_with_entries(1);
    app.begin_search();
    for ch in "tags:".chars() {
        app.search_input_key(key(KeyCode::Char(ch)));
    }

    // Nothing to suggest in this fixture, so the list is not up. Whatever the
    // keys mean here is the baseline the list must not disturb.
    assert!(!app.suggestions_open());
    let without_list = |code| keyboard::key_to_action(&app, key(code), true);
    let (enter, up) = (without_list(KeyCode::Enter), without_list(KeyCode::Up));
    assert_eq!(
        without_list(KeyCode::Esc),
        Some(Action::Search(SearchAction::Exit))
    );

    // With rows to show but none highlighted, only `Down`, `Tab` and `Esc` are
    // claimed.
    app.search.suggestions.rows = vec![crate::tui::features::search::suggest::SuggestionRow {
        label: "apple".to_string(),
        value: "apple".to_string(),
        count: 1,
    }];
    assert!(app.suggestions_open());
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Up), true),
        up,
        "Up is the entry list's until a row is highlighted"
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Enter), true),
        enter,
        "Enter still means what it meant"
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Down), true),
        Some(Action::Search(SearchAction::MoveSuggestion(1)))
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Tab), true),
        Some(Action::Search(SearchAction::CommitSuggestion))
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Esc), true),
        Some(Action::Search(SearchAction::DismissSuggestions)),
        "the first Esc closes the list, not search"
    );

    // Arrowed into, `Enter` and `Up` become the list's — deliberately entered.
    app.move_suggestion_highlight(1);
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Enter), true),
        Some(Action::Search(SearchAction::CommitSuggestion))
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Up), true),
        Some(Action::Search(SearchAction::MoveSuggestion(-1)))
    );

    // And `Up` off the first row hands both back.
    app.move_suggestion_highlight(-1);
    assert!(app.search.suggestions.highlighted().is_none());
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Enter), true),
        enter
    );
    assert_eq!(keyboard::key_to_action(&app, key(KeyCode::Up), true), up);

    // Dismissed, the list is down and Esc exits search again — but `Tab` brings
    // it back, so Esc is not a trap.
    app.dismiss_suggestions();
    assert!(!app.suggestions_open());
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Esc), true),
        Some(Action::Search(SearchAction::Exit))
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Tab), true),
        Some(Action::Search(SearchAction::CommitSuggestion))
    );
}

/// The commit has to be reachable by key, for every facet the list offers —
/// `Tab` under a `location:` prefix is the one that differs, because its value
/// arrives unquoted.
#[test]
fn tab_commits_a_suggestion_for_every_facet() {
    for (typed, expected) in [
        ("tags:apr", "tags:\"apricot\""),
        ("location:berl", "location:Berlin, Germany"),
    ] {
        let mut app = crate::tui::test_support::app_in_temp(|root| {
            let dir = root.join("work").join("2026-07-01");
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("a.md"),
                "+++\nschema_version = 1\n\n[entry]\ntags = [\"apricot\"]\n\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n\n[location]\ncity = \"Berlin\"\ncountry = \"Germany\"\n+++\n\nbody\n",
            )
            .unwrap();
        });
        app.begin_search();
        for ch in typed.chars() {
            app.search_input_key(key(KeyCode::Char(ch)));
        }

        assert_eq!(
            keyboard::key_to_action(&app, key(KeyCode::Tab), true),
            Some(Action::Search(SearchAction::CommitSuggestion)),
            "{typed}"
        );
        app.commit_first_suggestion();
        assert_eq!(app.search.query.as_str(), expected);
    }
}

/// Arrowing past the last visible row scrolls it into view. The popup's viewport
/// is only known to the render, so this is what proves the handler recovers it —
/// and the second frame proves the draw then leaves the offset alone.
#[test]
fn arrowing_down_the_suggestion_list_scrolls_the_row_into_view() {
    let mut app = app_offering_suggestions(12);
    let (mut terminal, view) = render_view(&mut app, 120, 30);
    let viewport = view
        .interactions
        .dialog_list_viewport(crate::tui::ui::DialogId::SearchSuggestions)
        .expect("the popup registers its viewport");
    assert!(viewport < 12, "the list has to overflow");

    for _ in 0..12 {
        let action = keyboard::key_to_action(&app, key(KeyCode::Down), true).expect("Down maps");
        dispatch_action(&mut terminal, &mut app, action).unwrap();
    }

    assert_eq!(app.search.suggestions.selected_index(), Some(11));
    assert_eq!(
        app.search.suggestions.offset(),
        12 - viewport as usize,
        "the last row is on screen"
    );
    render_view(&mut app, 120, 30);
    assert_eq!(app.search.suggestions.offset(), 12 - viewport as usize);
}

/// Down re-enters a scrolled-away list at its first row, the same way it enters
/// one that was never arrowed into — the wheel handed the list back.
#[test]
fn down_after_a_wheel_re_enters_the_suggestion_list_at_the_top() {
    use crate::tui::ui::InteractionKind;

    let mut app = app_offering_suggestions(12);
    let area = Rect::new(0, 0, 120, 30);
    app.move_suggestion_highlight(1);

    let (mut terminal, view) = render_view(&mut app, area.width, area.height);
    let (col, row) = find_interaction(&view, area.width, area.height, |kind| {
        matches!(
            kind,
            InteractionKind::DialogRow {
                dialog: crate::tui::ui::DialogId::SearchSuggestions,
                ..
            }
        )
    })
    .expect("a suggestion row is registered");
    for _ in 0..6 {
        let action = mouse::mouse_to_action(
            &app,
            mouse(MouseEventKind::ScrollDown, col, row),
            area,
            &view,
            false,
        )
        .expect("the wheel resolves");
        dispatch_action(&mut terminal, &mut app, action).unwrap();
    }
    render_view(&mut app, area.width, area.height);
    assert!(app.search.suggestions.offset() > 0);

    let action = keyboard::key_to_action(&app, key(KeyCode::Down), true).expect("Down maps");
    dispatch_action(&mut terminal, &mut app, action).unwrap();

    assert_eq!(app.search.suggestions.selected_index(), Some(0));
    assert_eq!(
        app.search.suggestions.offset(),
        0,
        "the list came back to the row it entered at"
    );
}

/// Begin link-hint mode and render, so the labels under test are the ones the
/// frame actually painted.
fn begin_hints(app: &mut AppModel) {
    app.reader_hints.begin();
    render_and_sync(app, 80, 24);
}

#[test]
fn o_activates_link_hints_on_the_focused_reader() {
    let mut app = app_reading("A [link](https://example.com) here.");
    // The count `o` is gated on comes from the drawn body, like the labels do.
    render_and_sync(&mut app, 80, 24);

    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('o')), true),
        Some(Action::ReaderHint(ReaderHintAction::Begin))
    );
}

#[test]
fn o_is_inert_without_a_selected_entry() {
    let mut app = app_with_journals(&["work"]);
    app.nav.focus = Focus::Reader;

    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('o')), true),
        None
    );
}

/// Nothing openable, nothing to press — `o` goes quiet, the same as `i` on an
/// entry with no images.
#[test]
fn o_is_inert_with_nothing_to_open() {
    let mut app = app_reading("Just prose, no links at all.");
    render_and_sync(&mut app, 80, 24);

    assert_eq!(app.reader_openable, 0);
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('o')), true),
        None
    );
}

#[test]
fn typing_a_unique_hint_label_opens_its_link() {
    let mut app = app_reading("A [link](https://example.com) here.");
    begin_hints(&mut app);
    let label = app.reader_hints.labels()[0].label.clone();
    assert_eq!(label, "a", "one target takes the first home-row letter");

    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    let outcome = dispatch_action(
        &mut terminal,
        &mut app,
        Action::ReaderHint(ReaderHintAction::Push('a')),
    )
    .unwrap();

    assert_eq!(
        outcome.effects.as_slice(),
        [Effect::Open {
            target: OpenTarget::Uri("https://example.com".to_string()),
            success_message: "Opened link".to_string(),
        }]
    );
    assert!(
        !app.reader_hints.is_active(),
        "the mode closes on activation"
    );
}

#[test]
fn typing_a_hint_label_for_an_anchor_scrolls_to_its_heading() {
    let filler = "\n\nfiller line".repeat(40);
    let mut app = app_reading(&format!("[go](#details){filler}\n\n## Details\n\ntail"));
    begin_hints(&mut app);

    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    dispatch_action(
        &mut terminal,
        &mut app,
        Action::ReaderHint(ReaderHintAction::Push('a')),
    )
    .unwrap();

    assert!(app.nav.scroll.reader > 0, "jumped to the heading");
    assert!(app.reader_anchor_flash.is_some(), "and flashed it");
}

#[test]
fn a_two_character_label_needs_both_keys() {
    // Past the 26-letter alphabet, so every label is two characters wide.
    let links: String = (0..30)
        .map(|index| format!("[link {index}](https://example.com/{index}) "))
        .collect();
    let mut app = app_reading(&links);
    begin_hints(&mut app);
    assert_eq!(app.reader_hints.labels()[0].label, "aa");

    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    let outcome = dispatch_action(
        &mut terminal,
        &mut app,
        Action::ReaderHint(ReaderHintAction::Push('a')),
    )
    .unwrap();

    assert!(outcome.effects.is_empty(), "one key only narrows the field");
    assert_eq!(app.reader_hints.pending(), "a");
    assert!(app.reader_hints.is_active());

    let outcome = dispatch_action(
        &mut terminal,
        &mut app,
        Action::ReaderHint(ReaderHintAction::Push('a')),
    )
    .unwrap();
    assert_eq!(outcome.effects.len(), 1, "the second key opens it");
}

/// A letter that matches nothing is swallowed, not an exit — which letters mean
/// nothing changes with the entry, so only Esc may end the mode.
#[test]
fn an_unmatched_letter_is_ignored_and_keeps_hint_mode_up() {
    let mut app = app_reading("A [link](https://example.com) here.");
    begin_hints(&mut app);

    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    for ch in ['z', 'o'] {
        let outcome = dispatch_action(
            &mut terminal,
            &mut app,
            Action::ReaderHint(ReaderHintAction::Push(ch)),
        )
        .unwrap();

        assert!(outcome.effects.is_empty(), "{ch} opens nothing");
        assert!(app.reader_hints.is_active(), "{ch} does not end the mode");
        assert!(app.reader_hints.pending().is_empty(), "{ch} is not banked");
    }
    assert!(app.toasts.items().is_empty(), "a miss is silent");
}

/// With enough targets to reach it, `o` is a label like any other — the key
/// that opened the mode is not spent on closing it.
#[test]
fn o_opens_its_target_once_the_alphabet_reaches_it() {
    let links: String = (0..20)
        .map(|index| format!("[link {index}](https://example.com/{index}) "))
        .collect();
    let mut app = app_reading(&links);
    begin_hints(&mut app);
    let labelled = app
        .reader_hints
        .labels()
        .iter()
        .find(|hint| hint.label == "o")
        .expect("o is assigned once there are 18+ targets")
        .target
        .clone();

    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    let outcome = dispatch_action(
        &mut terminal,
        &mut app,
        Action::ReaderHint(ReaderHintAction::Push('o')),
    )
    .unwrap();

    let ReaderLinkTarget::Uri(uri) = labelled else {
        panic!("expected a uri target");
    };
    assert_eq!(
        outcome.effects.as_slice(),
        [Effect::Open {
            target: OpenTarget::Uri(uri),
            success_message: "Opened link".to_string(),
        }]
    );
}

#[test]
fn esc_closes_the_hints_before_it_collapses_fullscreen() {
    let mut app = app_reading("A [link](https://example.com) here.");
    app.nav.reader_fullscreen = true;
    begin_hints(&mut app);

    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Esc), true),
        Some(Action::ReaderHint(ReaderHintAction::Cancel))
    );
    assert!(
        app.nav.reader_fullscreen,
        "still fullscreen after the first Esc"
    );
}

#[test]
fn scroll_keys_stay_live_while_the_hints_are_up() {
    let mut app = app_reading("A [link](https://example.com) here.");
    begin_hints(&mut app);

    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Down), true),
        Some(Action::Reader(ReaderAction::ScrollLines(1)))
    );
}

#[test]
fn digits_no_longer_open_images() {
    let app = app_reading("A [link](https://example.com) here.");

    for ch in ['0', '1', '9'] {
        assert_eq!(
            keyboard::key_to_action(&app, key(KeyCode::Char(ch)), true),
            None,
            "{ch}"
        );
    }
}

#[test]
fn hint_mode_ends_when_the_reader_loses_focus() {
    let mut app = app_reading("A [link](https://example.com) here.");
    begin_hints(&mut app);
    assert!(app.reader_hints.is_active());

    app.nav.focus = Focus::Entries;
    render_and_sync(&mut app, 80, 24);

    assert!(!app.reader_hints.is_active());
}

/// An overlay suspends the mode: the frame paints no chips, so it goes down.
#[test]
fn hint_mode_ends_quietly_when_an_overlay_opens() {
    let mut app = app_reading("A [link](https://example.com) here.");
    begin_hints(&mut app);

    app.open_help();
    render_and_sync(&mut app, 80, 24);

    assert!(!app.reader_hints.is_active());
    assert!(app.toasts.items().is_empty(), "a suspension says nothing");
}

/// Labels cover the whole entry, so scrolling neither relabels nor disturbs a
/// half-typed one — the chips scroll with the text they sit in.
#[test]
fn scrolling_keeps_the_labels_and_a_pending_prefix() {
    let links: String = (0..30)
        .map(|index| format!("[link {index}](https://example.com/{index})\n\n"))
        .collect();
    let mut app = app_reading(&links);
    begin_hints(&mut app);
    let before: Vec<_> = app
        .reader_hints
        .labels()
        .iter()
        .map(|hint| hint.target.clone())
        .collect();
    app.reader_hints.push('a');

    app.scroll_reader(6);
    render_and_sync(&mut app, 80, 24);

    let after: Vec<_> = app
        .reader_hints
        .labels()
        .iter()
        .map(|hint| hint.target.clone())
        .collect();
    assert_eq!(before, after, "the label set spans the whole entry");
    assert_eq!(app.reader_hints.pending(), "a");
}

/// A target scrolled out of sight still opens: its chip is part of the body, so
/// the label stays valid wherever the reader happens to be parked.
#[test]
fn a_label_scrolled_out_of_view_still_opens() {
    let links: String = (0..30)
        .map(|index| format!("[link {index}](https://example.com/{index})\n\n"))
        .collect();
    let mut app = app_reading(&links);
    begin_hints(&mut app);
    app.scroll_reader(40);
    render_and_sync(&mut app, 80, 24);

    // 30 targets, so labels are two characters wide.
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    dispatch_action(
        &mut terminal,
        &mut app,
        Action::ReaderHint(ReaderHintAction::Push('a')),
    )
    .unwrap();
    let outcome = dispatch_action(
        &mut terminal,
        &mut app,
        Action::ReaderHint(ReaderHintAction::Push('a')),
    )
    .unwrap();

    assert_eq!(
        outcome.effects.as_slice(),
        [Effect::Open {
            target: OpenTarget::Uri("https://example.com/0".to_string()),
            success_message: "Opened link".to_string(),
        }]
    );
}

/// Reached only past the `o` gate — by a stale click, say — the mode collapses
/// on the first frame and says nothing about it.
#[test]
fn hint_mode_collapses_silently_with_nothing_to_label() {
    let mut app = app_reading("Just prose, no links at all.");

    begin_hints(&mut app);

    assert!(!app.reader_hints.is_active());
    assert!(app.toasts.items().is_empty());
}
