use super::*;
use crate::tui::state::MetadataKind;

// ── Settings menu / theme picker routing ─────────────────────────────────────

#[test]
fn comma_opens_settings_in_browse_but_not_over_dialogs() {
    let mut app = app_with_entries(1);
    app.nav.focus = Focus::Entries;

    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char(',')), true),
        Some(Action::Settings(SettingsAction::OpenSettings))
    );

    // With a dialog open the key belongs to that overlay, not settings.
    app.begin_edit_tags();
    assert_ne!(
        keyboard::key_to_action(&app, key(KeyCode::Char(',')), true),
        Some(Action::Settings(SettingsAction::OpenSettings))
    );
}

// ── Help cheatsheet ──────────────────────────────────────────────────────────

#[test]
fn question_mark_opens_help_from_browse_and_search_panes() {
    let mut app = app_with_entries(1);
    app.nav.focus = Focus::Entries;
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('?')), true),
        Some(Action::Overlay(OverlayAction::OpenHelp))
    );

    // In search, `?` opens the cheatsheet from a result view but types into the
    // search field.
    app.begin_search();
    app.nav.focus = Focus::Reader;
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('?')), true),
        Some(Action::Overlay(OverlayAction::OpenHelp))
    );
    app.nav.focus = Focus::Entries;
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('?')), true),
        Some(Action::Overlay(OverlayAction::InputKey(key(
            KeyCode::Char('?')
        ))))
    );
}

#[test]
fn help_overlay_scrolls_on_arrows_and_closes_only_on_esc() {
    let mut app = app_with_entries(1);
    app.open_help();

    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Down), true),
        Some(Action::Overlay(OverlayAction::HelpScroll(1)))
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::PageUp), true),
        Some(Action::Overlay(OverlayAction::HelpScroll(-10)))
    );
    // A quit key is swallowed, not treated as a dismiss.
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('q')), true),
        None
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Esc), true),
        Some(Action::Overlay(OverlayAction::Cancel))
    );
}

#[test]
fn help_tabs_switch_on_tab_and_arrow_keys_and_reset_scroll() {
    let mut app = app_with_entries(1);
    app.open_help();

    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Tab), true),
        Some(Action::Overlay(OverlayAction::HelpNextTab))
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Left), true),
        Some(Action::Overlay(OverlayAction::HelpPrevTab))
    );

    let backend = ratatui::backend::TestBackend::new(80, 20);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    dispatch_action(
        &mut terminal,
        &mut app,
        Action::Overlay(OverlayAction::HelpScroll(3)),
    )
    .unwrap();
    dispatch_action(
        &mut terminal,
        &mut app,
        Action::Overlay(OverlayAction::HelpNextTab),
    )
    .unwrap();
    match app.overlay {
        crate::tui::state::Overlay::Help { tab, scroll } => {
            assert_eq!(tab, crate::tui::state::HelpTab::Search);
            assert_eq!(scroll, 0, "switching tabs resets the scroll");
        }
        _ => panic!("help overlay closed on tab switch"),
    }
}

#[test]
fn help_tab_click_selects_that_tab() {
    use crate::tui::ui::InteractionKind;

    let mut app = app_with_entries(1);
    app.open_help();
    let area = Rect::new(0, 0, 90, 30);
    let (_, view) = render_view(&mut app, area.width, area.height);

    let (col, row) = find_interaction(&view, area.width, area.height, |kind| {
        matches!(
            kind,
            InteractionKind::HelpTab(crate::tui::state::HelpTab::Search)
        )
    })
    .expect("help tab registered");

    assert_eq!(
        mouse::mouse_to_action(&app, mouse(down(), col, row), area, &view, false),
        Some(Action::Overlay(OverlayAction::HelpSelectTab(
            crate::tui::state::HelpTab::Search
        )))
    );
}

/// The overlay is modal: only its own regions act, so a click on the panes
/// behind it produces no action at all. Asserting only that the overlay stays
/// open would pass just as well if the click had focused the pane behind it.
#[test]
fn click_outside_the_help_overlay_is_inert() {
    let mut app = app_with_entries(1);
    app.open_help();
    let area = Rect::new(0, 0, 90, 30);
    let (_, view) = render_view(&mut app, area.width, area.height);

    assert_eq!(
        mouse::mouse_to_action(&app, mouse(down(), 1, 1), area, &view, false),
        None
    );

    mouse_in_area(&mut app, mouse(down(), 1, 1), area.width, area.height);
    assert!(matches!(
        app.overlay,
        crate::tui::state::Overlay::Help { .. }
    ));
}

/// The editor's reference rides a prompt, not an overlay, so its close chip has
/// to end the prompt instead of cancelling an overlay.
#[test]
fn editor_shortcut_close_chip_ends_the_prompt() {
    use crate::tui::ui::InteractionKind;

    let mut app = app_with_entries(1);
    app.select_entry_index(0);
    app.open_editor_for_selected().unwrap();
    app.editor.as_mut().unwrap().prompt = EditorPrompt::Help { scroll: 0 };
    let area = Rect::new(0, 0, 90, 30);
    let (_, view) = render_view(&mut app, area.width, area.height);

    let (col, row) = find_interaction(&view, area.width, area.height, |kind| {
        matches!(kind, InteractionKind::Hint(render::HintId::CancelOverlay))
    })
    .expect("close chip registered");

    assert_eq!(
        mouse::mouse_to_action(&app, mouse(down(), col, row), area, &view, false),
        Some(Action::Editor(EditorAction::ClosePrompt))
    );
}

#[test]
fn help_hint_click_opens_the_cheatsheet() {
    let app = app_with_entries(1);
    assert_eq!(
        mouse::hint_id_to_action(&app, render::HintId::Help),
        Some(Action::Overlay(OverlayAction::OpenHelp))
    );
}

#[test]
fn wheel_over_help_scrolls_it_without_closing() {
    let mut app = app_with_entries(1);
    app.open_help();

    mouse_in_area(&mut app, mouse(MouseEventKind::ScrollDown, 5, 5), 80, 20);

    // The wheel bumps the reference's scroll and the overlay stays open — the
    // early-return keeps the event off the panes behind it.
    match app.overlay {
        crate::tui::state::Overlay::Help { scroll, .. } => assert_eq!(scroll, 1),
        _ => panic!("help overlay closed on wheel"),
    }
}

#[test]
fn settings_dialog_routes_move_toggle_adjust_and_close() {
    let mut app = app_with_journals(&["work"]);
    app.open_settings();

    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Down), true),
        Some(Action::Metadata(MetadataAction::MoveSelection(1)))
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Up), true),
        Some(Action::Metadata(MetadataAction::MoveSelection(-1)))
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Enter), true),
        Some(Action::Settings(SettingsAction::Activate))
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char(' ')), true),
        Some(Action::Settings(SettingsAction::Activate))
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Left), true),
        Some(Action::Settings(SettingsAction::Adjust(-1)))
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Right), true),
        Some(Action::Settings(SettingsAction::Adjust(1)))
    );
    // Esc closes the dialog outright — there's no submenu to step back to.
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Esc), true),
        Some(Action::Overlay(OverlayAction::Cancel))
    );
}

/// The cursor only ever lands on setting rows, never a category sub-header, and
/// clamps at both ends of the list.
#[test]
fn settings_dialog_navigation_skips_headers_and_clamps() {
    use crate::tui::state::{ListNav, SettingsItem};
    let mut app = app_with_journals(&["work"]);
    app.open_settings();
    let count = app.settings_state().unwrap().items.len();

    for _ in 0..count + 2 {
        let state = app.settings_state().unwrap();
        let index = state.selected_index().unwrap();
        assert!(matches!(state.items[index], SettingsItem::Row { .. }));
        app.settings_state_mut().unwrap().move_down();
    }
    for _ in 0..count + 2 {
        let state = app.settings_state().unwrap();
        let index = state.selected_index().unwrap();
        assert!(matches!(state.items[index], SettingsItem::Row { .. }));
        app.settings_state_mut().unwrap().move_up();
    }
}

/// Toggling a bool setting flips the config field and persists.
#[test]
fn settings_activate_toggles_and_persists_bool() {
    let mut app = app_with_journals(&["work"]);
    let before = app.services.config.ui.layout.reader.body_center_vertically;

    app.open_settings();
    // "Center body vertically" is Reader's first row (item 5: Appearance header,
    // its two rows, a spacer, the Reader header, then this row).
    app.settings_select(5);
    app.settings_activate();

    assert_eq!(
        app.services.config.ui.layout.reader.body_center_vertically,
        !before
    );
}

/// The dialog opens seeded on the Theme row, so activating it opens the picker
/// straight away — no submenu to drill into first.
#[test]
fn settings_activate_theme_row_opens_the_picker() {
    let mut app = app_with_journals(&["work"]);
    app.open_settings();
    app.settings_activate();
    assert!(app.theme_picker_state().is_some());
    // Closing the picker returns to the settings dialog it was launched from.
    app.theme_picker_cancel();
    assert!(app.settings_state().is_some());
}

/// Adjusting the numeric row steps within its clamp and persists.
#[test]
fn settings_adjust_steps_the_number() {
    let mut app = app_with_journals(&["work"]);
    app.services.config.ui.layout.reader.body_max_width = 100;

    app.open_settings();
    // "Max body width" is Reader's second row (item 6).
    app.settings_select(6);
    app.settings_adjust(1);
    assert_eq!(app.services.config.ui.layout.reader.body_max_width, 105);
    app.settings_adjust(-1);
    assert_eq!(app.services.config.ui.layout.reader.body_max_width, 100);
}

/// Stepping down from the minimum snaps to 0 ("Off"), and back up returns to it.
#[test]
fn settings_adjust_snaps_max_body_width_off() {
    let mut app = app_with_journals(&["work"]);
    app.services.config.ui.layout.reader.body_max_width = 40;

    app.open_settings();
    // "Max body width" is Reader's second row (item 6).
    app.settings_select(6);
    app.settings_adjust(-1);
    assert_eq!(app.services.config.ui.layout.reader.body_max_width, 0);
    // Further left stays at Off rather than jumping back up.
    app.settings_adjust(-1);
    assert_eq!(app.services.config.ui.layout.reader.body_max_width, 0);
    assert_eq!(
        app.settings_state()
            .and_then(|s| s.selected_row())
            .map(|(_, row)| row.value(&app.services.config)),
        Some("Unlimited".to_string())
    );
    app.settings_adjust(1);
    assert_eq!(app.services.config.ui.layout.reader.body_max_width, 40);
}

/// Top padding labels 0 as "None" but still increments normally from it.
#[test]
fn settings_adjust_top_padding_shows_none_at_zero() {
    let mut app = app_with_journals(&["work"]);
    app.services.config.ui.layout.reader.body_max_top_padding = 0;

    app.open_settings();
    // "Max body top padding" is Reader's third row (item 7).
    app.settings_select(7);
    assert_eq!(
        app.settings_state()
            .and_then(|s| s.selected_row())
            .map(|(_, row)| row.value(&app.services.config)),
        Some("None".to_string())
    );
    app.settings_adjust(1);
    assert_eq!(app.services.config.ui.layout.reader.body_max_top_padding, 1);
}

/// Select the Editor row labelled `label` in the open settings dialog.
fn select_editor_row(app: &mut crate::tui::app::AppModel, label: &str) {
    use crate::tui::features::settings::SettingCategory;
    use crate::tui::state::SettingsItem;

    let index = app
        .settings_state()
        .unwrap()
        .items
        .iter()
        .position(|item| {
            matches!(item, SettingsItem::Row { category: SettingCategory::Editor, index }
                if SettingCategory::Editor.rows()[*index].label() == label)
        })
        .unwrap_or_else(|| panic!("no Editor row labelled {label}"));
    app.settings_select(index);
}

fn selected_value(app: &crate::tui::app::AppModel) -> String {
    app.settings_state()
        .and_then(|s| s.selected_row())
        .map(|(_, row)| row.value(&app.services.config))
        .expect("a selected row")
}

/// An editor number steps down past its off state into Inherit, and back out.
#[test]
fn settings_editor_number_steps_into_inherit() {
    let mut app = app_with_journals(&["work"]);
    app.services.config.ui.layout.reader.body_max_width = 80;
    app.open_settings();
    select_editor_row(&mut app, "Max body width");

    assert_eq!(selected_value(&app), "Inherit");
    assert_eq!(app.services.config.ui.layout.editor_body().max_width, 80);

    app.settings_adjust(1);
    assert_eq!(selected_value(&app), "Unlimited");
    app.settings_adjust(1);
    assert_eq!(selected_value(&app), "40");
    assert_eq!(app.services.config.ui.layout.editor_body().max_width, 40);

    // Stops at Inherit rather than wrapping back to the top.
    app.settings_adjust(-1);
    app.settings_adjust(-1);
    assert_eq!(selected_value(&app), "Inherit");
    app.settings_adjust(-1);
    assert_eq!(selected_value(&app), "Inherit");
    assert_eq!(app.services.config.ui.layout.editor_body().max_width, 80);
}

/// The editor's centering starts off rather than inherited, and cycles
/// Inherit → Off → On.
#[test]
fn settings_editor_centering_starts_off_and_cycles() {
    let mut app = app_with_journals(&["work"]);
    assert!(app.services.config.ui.layout.reader.body_center_vertically);
    app.open_settings();
    select_editor_row(&mut app, "Center body vertically");

    assert_eq!(selected_value(&app), "Off");
    assert!(
        !app.services
            .config
            .ui
            .layout
            .editor_body()
            .center_vertically
    );

    app.settings_adjust(-1);
    assert_eq!(selected_value(&app), "Inherit");
    assert!(
        app.services
            .config
            .ui
            .layout
            .editor_body()
            .center_vertically
    );

    app.settings_activate();
    assert_eq!(selected_value(&app), "Off");
    app.settings_activate();
    assert_eq!(selected_value(&app), "On");
    app.settings_activate();
    assert_eq!(selected_value(&app), "Inherit");
}

#[test]
fn theme_picker_keys_route_to_dedicated_actions() {
    let mut app = app_with_journals(&["work"]);
    app.open_theme_picker();

    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Up), true),
        Some(Action::Metadata(MetadataAction::MoveSelection(-1)))
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Down), true),
        Some(Action::Metadata(MetadataAction::MoveSelection(1)))
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Enter), true),
        Some(Action::Settings(SettingsAction::ThemePickerConfirm))
    );
    // Esc reverts through the dedicated cancel, not the generic overlay close.
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Esc), true),
        Some(Action::Settings(SettingsAction::ThemePickerCancel))
    );

    // The picker's hint chips route to the same actions.
    assert_eq!(
        mouse::hint_id_to_action(&app, render::HintId::ThemePickerApply),
        Some(Action::Settings(SettingsAction::ThemePickerConfirm))
    );
    assert_eq!(
        mouse::hint_id_to_action(&app, render::HintId::ThemePickerRevert),
        Some(Action::Settings(SettingsAction::ThemePickerCancel))
    );
}

// ── Hover ─────────────────────────────────────────────────────────────────────

#[test]
fn hover_tracks_journal_rows_without_moving_selection() {
    let mut app = app_with_journals(&["work", "zeta"]);
    let area = Rect::new(0, 0, 120, 20);
    let journals = render::tui_layout(area, &app)
        .journals
        .expect("journals panel");
    let list = render::journal_list_rect(journals.content);
    let selected_before = app.nav.journal_list.selected();

    // The middle line of the second journal's row.
    let row = list.y + render::journal_row_height(&app.appearance.theme) + 1;
    assert!(apply_hover(&mut app, list.x + 2, row, area));
    assert_eq!(app.hover, HoverTarget::Journal(1));
    assert_eq!(
        app.nav.journal_list.selected(),
        selected_before,
        "hover must never move the journal selection"
    );

    // Motion within the same row doesn't ask for a repaint.
    assert!(!apply_hover(&mut app, list.x + 3, row, area));

    // The run loop dispatches SetHover(None) ahead of every key event — pin
    // the handler half of that "any key clears the glow" contract.
    let backend = ratatui::backend::TestBackend::new(area.width, area.height);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    dispatch_action(&mut terminal, &mut app, Action::SetHover(HoverTarget::None)).unwrap();
    assert_eq!(app.hover, HoverTarget::None);
}

#[test]
fn hover_translation_does_not_mutate_the_model() {
    let mut app = app_with_journals(&["work", "zeta"]);
    let area = Rect::new(0, 0, 120, 20);
    let (_, view) = render_view(&mut app, area.width, area.height);
    let journals = render::tui_layout(area, &app)
        .journals
        .expect("journals panel");
    let list = render::journal_list_rect(journals.content);
    let row = list.y + render::journal_row_height(&app.appearance.theme) + 1;
    let selected_before = app.nav.journal_list.selected();
    let hover_before = app.hover;

    assert_eq!(
        mouse::hover_action_at(&app, list.x + 2, row, area, &view),
        Action::SetHover(HoverTarget::Journal(1))
    );
    assert_eq!(app.nav.journal_list.selected(), selected_before);
    assert_eq!(app.hover, hover_before);
}

#[test]
fn click_translation_does_not_mutate_the_model() {
    let mut app = app_with_journals(&["work", "zeta"]);
    let area = Rect::new(0, 0, 120, 20);
    let backend = ratatui::backend::TestBackend::new(area.width, area.height);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    let mut view = crate::tui::ui::ViewState::default();
    let theme = app.appearance.theme.clone();
    terminal
        .draw(|frame| {
            let mut context = crate::tui::ui::RenderContext::new(&theme, &mut view);
            crate::tui::render::draw(frame, &mut app, &mut context);
        })
        .unwrap();
    let journals = view.layout.unwrap().journals.unwrap();
    let list = render::journal_list_rect(journals.content);
    let row = list.y + render::journal_row_height(&app.appearance.theme) + 1;
    let before = (
        app.nav.focus,
        app.nav.journal_list.selected(),
        app.nav.entry_list.selected(),
        app.nav.scroll.reader,
        app.toasts.items().len(),
    );

    let action = mouse::mouse_to_action(&app, mouse(down(), list.x + 2, row), area, &view, false);

    assert_eq!(
        action,
        Some(Action::Mouse(action::MouseAction::JournalClick {
            index: Some(1),
            compact: false,
        }))
    );
    assert_eq!(
        before,
        (
            app.nav.focus,
            app.nav.journal_list.selected(),
            app.nav.entry_list.selected(),
            app.nav.scroll.reader,
            app.toasts.items().len(),
        )
    );
}

#[test]
fn hover_finds_footer_hints() {
    let mut app = app_with_journals(&["work"]);
    let area = Rect::new(0, 0, 120, 20);
    let footer = render::tui_layout(area, &app).footer;
    let hovered = (footer.x..footer.x + footer.width).any(|col| {
        apply_hover(&mut app, col, footer.y, area);
        matches!(app.hover, HoverTarget::FooterHint(_))
    });
    assert!(hovered, "no footer hint hoverable on the browse footer");
}

#[test]
fn hover_tracks_insights_tabs_without_switching_tabs() {
    let mut app = app_with_entries(1);
    app.nav.selected_entry_index = None;
    app.nav.insights_tab = InsightsTab::Overview;
    let area = Rect::new(0, 0, 140, 20);
    let insights = render::tui_layout(area, &app)
        .insights
        .expect("insights panel");
    let col = (insights.area.x..insights.area.x + insights.area.width)
        .find(|col| {
            render::insights_tab_at(&app.appearance.theme, insights.area, *col, insights.area.y)
                == Some(InsightsTab::Writing)
        })
        .expect("writing tab");

    assert!(apply_hover(&mut app, col, insights.area.y, area));
    assert_eq!(app.hover, HoverTarget::InsightsTab(InsightsTab::Writing));
    assert_eq!(app.nav.insights_tab, InsightsTab::Overview);
}

#[test]
fn theme_picker_hover_targets_rows_without_selecting() {
    let mut app = app_with_journals(&["work"]);
    app.open_theme_picker();
    let area = Rect::new(0, 0, 90, 30);
    let state = app.theme_picker_state().expect("picker open");
    let len = state.entries.len();
    assert!(len > 1, "picker should list the bundled themes");
    let initial = state.selected_index();
    let offset = state.offset();
    let target = if initial == Some(offset) {
        offset + 1
    } else {
        offset
    };
    let layout = render::theme_picker_layout(
        &app.appearance.theme,
        area,
        len,
        state.hint_state(app.appearance.chrome_override, app.appearance.color_mode),
    );

    let row = layout.list.y + (target - offset) as u16;
    assert!(apply_hover(&mut app, layout.list.x + 1, row, area));
    // Like every other dialog, hover only highlights the row — it neither
    // moves the selection nor previews the theme (that's click's job).
    assert_eq!(app.hover, HoverTarget::DialogRow(target));
    assert_eq!(app.theme_picker_state().unwrap().selected_index(), initial);
}

/// The first click on a theme row previews it; a click on the already-selected
/// row confirms and closes the picker.
#[test]
fn theme_picker_click_previews_then_confirms() {
    use crate::tui::ui::{DialogId, InteractionKind};

    let mut app = app_with_journals(&["work"]);
    app.open_theme_picker();
    let area = Rect::new(0, 0, 90, 30);
    let state = app.theme_picker_state().expect("picker open");
    let initial = state.selected_index();
    // A visible, valid, non-selected row to preview then confirm.
    let target = state
        .entries
        .iter()
        .enumerate()
        .position(|(index, entry)| Some(index) != initial && entry.theme.is_some())
        .expect("a valid unselected theme");

    // Re-locate the row each click: previewing a theme can swap the chrome and
    // shift the dialog, so a cached screen position would go stale.
    let click_target = |app: &mut AppModel| {
        let (_, view) = render_view(app, area.width, area.height);
        let (col, row) = find_interaction(&view, area.width, area.height, |kind| {
            matches!(
                kind,
                InteractionKind::DialogRow {
                    dialog: DialogId::ThemePicker,
                    index,
                } if *index == target
            )
        })
        .expect("target theme row registered");
        mouse_in_area(app, mouse(down(), col, row), area.width, area.height);
    };

    // First click previews the row without closing the picker.
    click_target(&mut app);
    assert_eq!(
        app.theme_picker_state().unwrap().selected_index(),
        Some(target),
        "first click only previews"
    );

    // A second click on the now-selected row confirms and closes the picker.
    click_target(&mut app);
    assert!(app.theme_picker_state().is_none(), "second click confirms");
}

#[test]
fn settings_dialog_hover_targets_its_rows() {
    use crate::tui::ui::{DialogId, InteractionKind};

    let mut app = app_with_journals(&["work"]);
    app.open_settings();
    let area = Rect::new(0, 0, 64, 24);

    // Item 1 is the Theme row (item 0 is the Appearance sub-header, which is
    // inert). Find it through the regions render registered — the same ones the
    // click path resolves against.
    let (_, view) = render_view(&mut app, area.width, area.height);
    let point = find_interaction(&view, area.width, area.height, |kind| {
        matches!(
            kind,
            InteractionKind::DialogRow {
                dialog: DialogId::Settings,
                index: 1,
            }
        )
    })
    .expect("settings dialog has a hoverable row");
    assert!(apply_hover(&mut app, point.0, point.1, area));
    assert_eq!(app.hover, HoverTarget::DialogRow(1));
}

/// A category sub-header registers no clickable row, so it can't be hovered.
#[test]
fn settings_dialog_header_is_not_hoverable() {
    use crate::tui::ui::{DialogId, InteractionKind};

    let mut app = app_with_journals(&["work"]);
    app.open_settings();
    let area = Rect::new(0, 0, 64, 24);

    let (_, view) = render_view(&mut app, area.width, area.height);
    // Item 0 is the Appearance header; no DialogRow is registered for it.
    assert!(
        find_interaction(&view, area.width, area.height, |kind| matches!(
            kind,
            InteractionKind::DialogRow {
                dialog: DialogId::Settings,
                index: 0,
            }
        ))
        .is_none()
    );
}

#[test]
fn editor_discard_prompt_hover_targets_the_buttons() {
    let mut app = app_with_entries(1);
    app.select_entry_index(0);
    app.open_editor_for_selected().unwrap();
    app.editor.as_mut().unwrap().prompt = crate::tui::editor_state::EditorPrompt::ConfirmDiscard {
        discard_selected: false,
    };
    let area = Rect::new(0, 0, 120, 20);

    // Probe every cell until both buttons are found through the real regions.
    let mut saw = (false, false);
    for row in 0..area.height {
        for col in 0..area.width {
            apply_hover(&mut app, col, row, area);
            match app.hover {
                HoverTarget::ConfirmButton(true) => saw.0 = true,
                HoverTarget::ConfirmButton(false) => saw.1 = true,
                _ => {}
            }
        }
    }
    assert!(saw.0 && saw.1, "both discard buttons hoverable: {saw:?}");
}

// ── Menu clicks through the interaction map ───────────────────────────────────

/// The action a click at the given dialog row / close region translates to,
/// resolved through the regions render registered.
fn menu_click_action(
    app: &mut AppModel,
    area: Rect,
    predicate: impl Fn(&crate::tui::ui::InteractionKind) -> bool,
) -> Option<Action> {
    let (_, view) = render_view(app, area.width, area.height);
    let (col, row) =
        find_interaction(&view, area.width, area.height, predicate).expect("region registered");
    mouse::mouse_to_action(app, mouse(down(), col, row), area, &view, false)
}

#[test]
fn settings_dialog_click_activates_row_and_close_through_the_regions() {
    use crate::tui::ui::{DialogId, InteractionKind};

    let mut app = app_with_journals(&["work"]);
    app.open_settings();
    let area = Rect::new(0, 0, 64, 24);

    // Item 1 is the Theme row; clicking it selects then activates it.
    let row_action = menu_click_action(&mut app, area, |kind| {
        matches!(
            kind,
            InteractionKind::DialogRow {
                dialog: DialogId::Settings,
                index: 1,
            }
        )
    });
    assert_eq!(row_action, Some(Action::Settings(SettingsAction::Click(1))));

    // The close affordance is the hint bar's "close esc" chip.
    let close_action = menu_click_action(&mut app, area, |kind| {
        matches!(
            kind,
            InteractionKind::Hint(crate::tui::render::HintId::CancelOverlay)
        )
    });
    assert_eq!(close_action, Some(Action::Overlay(OverlayAction::Cancel)));

    // Dispatching the Theme row click opens the picker.
    let (_, view) = render_view(&mut app, area.width, area.height);
    let (col, row) = find_interaction(&view, area.width, area.height, |kind| {
        matches!(
            kind,
            InteractionKind::DialogRow {
                dialog: DialogId::Settings,
                index: 1,
            }
        )
    })
    .unwrap();
    mouse_in_area(&mut app, mouse(down(), col, row), area.width, area.height);
    assert!(app.theme_picker_state().is_some());
}

#[test]
fn settings_dialog_click_toggles_a_bool_row() {
    use crate::tui::ui::{DialogId, InteractionKind};

    let mut app = app_with_journals(&["work"]);
    app.open_settings();
    let area = Rect::new(0, 0, 72, 24);
    let before = app.services.config.ui.layout.reader.body_center_vertically;

    // Item 5 is "Center body vertically", unselected (the dialog seeds on Theme).
    let (_, view) = render_view(&mut app, area.width, area.height);
    let (col, row) = find_interaction(&view, area.width, area.height, |kind| {
        matches!(
            kind,
            InteractionKind::DialogRow {
                dialog: DialogId::Settings,
                index: 5,
            }
        )
    })
    .expect("setting row registered");
    mouse_in_area(&mut app, mouse(down(), col, row), area.width, area.height);
    assert_eq!(
        app.services.config.ui.layout.reader.body_center_vertically, before,
        "first click only selects"
    );

    // A second click on the now-selected row toggles it.
    mouse_in_area(&mut app, mouse(down(), col, row), area.width, area.height);
    assert_eq!(
        app.services.config.ui.layout.reader.body_center_vertically, !before,
        "second click toggles"
    );
}

#[test]
fn metadata_menu_click_maps_every_row_to_its_action() {
    use crate::tui::ui::{DialogId, InteractionKind};

    let mut app = app_with_entries(1);
    app.select_entry_index(0);
    app.open_metadata_menu();
    assert!(matches!(app.overlay, Overlay::MetadataMenu));
    let area = Rect::new(0, 0, 80, 24);

    let expected: [Action; 6] = [
        Action::Metadata(MetadataAction::BeginEdit(MetadataKind::Tags)),
        Action::Metadata(MetadataAction::BeginEdit(MetadataKind::People)),
        Action::Metadata(MetadataAction::BeginEdit(MetadataKind::Activities)),
        Action::Metadata(MetadataAction::BeginFeelings),
        Action::Metadata(MetadataAction::BeginMood),
        Action::Location(LocationAction::BeginEdit),
    ];
    for (index, expected) in expected.into_iter().enumerate() {
        let action = menu_click_action(&mut app, area, |kind| {
            matches!(
                kind,
                InteractionKind::DialogRow {
                    dialog: DialogId::MetadataMenu,
                    index: i,
                } if *i == index
            )
        });
        assert_eq!(action, Some(expected), "row {index}");
    }

    let close_action = menu_click_action(&mut app, area, |kind| {
        matches!(
            kind,
            InteractionKind::Hint(crate::tui::render::HintId::CancelOverlay)
        )
    });
    assert_eq!(close_action, Some(Action::Overlay(OverlayAction::Cancel)));
}

/// The chooser closes on Esc or a valid metadata key, but ignores everything
/// else rather than dismissing on any keypress.
#[test]
fn metadata_menu_closes_on_esc_or_valid_key_only() {
    let mut app = app_with_entries(1);
    app.select_entry_index(0);
    app.open_metadata_menu();

    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Esc), true),
        Some(Action::Overlay(OverlayAction::Cancel))
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('t')), true),
        Some(Action::Metadata(MetadataAction::BeginEdit(
            MetadataKind::Tags
        )))
    );
    // An unmapped key is inert — the popup stays open.
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('z')), true),
        None
    );
}

#[test]
fn editor_double_click_maps_to_select_word() {
    let mut app = app_with_entries(1);
    app.select_entry_index(0);
    app.open_editor_for_selected().unwrap();
    let area = Rect::new(0, 0, 80, 24);
    let (_, view) = render_view(&mut app, area.width, area.height);

    // A central cell in the text body, away from the border and footer.
    let (col, row) = (10, 6);
    let single = mouse::mouse_to_action(&app, mouse(down(), col, row), area, &view, false);
    assert_eq!(
        single,
        Some(Action::Editor(EditorAction::StartSelection { col, row }))
    );
    let double = mouse::mouse_to_action(&app, mouse(down(), col, row), area, &view, true);
    assert_eq!(
        double,
        Some(Action::Editor(EditorAction::SelectWord { col, row }))
    );
}

#[test]
fn editor_metadata_menu_click_maps_rows_and_close() {
    use crate::tui::ui::{DialogId, InteractionKind};

    let mut app = app_with_entries(1);
    app.select_entry_index(0);
    app.open_editor_for_selected().unwrap();
    app.editor.as_mut().unwrap().prompt = crate::tui::editor_state::EditorPrompt::MetadataMenu;
    let area = Rect::new(0, 0, 80, 24);

    let row_action = menu_click_action(&mut app, area, |kind| {
        matches!(
            kind,
            InteractionKind::DialogRow {
                dialog: DialogId::EditorMetadataMenu,
                index: 0,
            }
        )
    });
    assert_eq!(
        row_action,
        Some(Action::Metadata(MetadataAction::BeginEdit(
            MetadataKind::Tags
        )))
    );

    // The editor's menu closes back to the editor, not the overlay layer: its
    // "close esc" chip ends the prompt.
    let close_action = menu_click_action(&mut app, area, |kind| {
        matches!(
            kind,
            InteractionKind::Hint(crate::tui::render::HintId::CancelOverlay)
        )
    });
    assert_eq!(
        close_action,
        Some(Action::Editor(EditorAction::ClosePrompt))
    );
}

// ── Toast interaction ─────────────────────────────────────────────────────────

#[test]
fn clicking_a_toast_dismisses_it() {
    let mut app = app_with_journals(&["work"]);
    app.toast(crate::tui::state::ToastVariant::Info, "First");
    app.toast(crate::tui::state::ToastVariant::Error, "Second");
    let area = Rect::new(0, 0, 120, 30);
    let rects = render::toast_rects(&app, area);
    assert_eq!(rects.len(), 2);

    // Click the second toast: only it disappears.
    let target = rects[1];
    mouse_in_area(&mut app, mouse(down(), target.x + 1, target.y + 1), 120, 30);
    let remaining: Vec<_> = app
        .toasts
        .items()
        .iter()
        .map(|toast| toast.message.clone())
        .collect();
    assert_eq!(remaining, vec!["First".to_string()]);

    // A click outside any toast is not swallowed by the dismiss probe.
    mouse_in_area(&mut app, mouse(down(), 0, area.height - 1), 120, 30);
    assert_eq!(app.toasts.items().len(), 1);
}

#[test]
fn hovering_a_toast_targets_it_over_everything() {
    let mut app = app_with_journals(&["work"]);
    app.open_theme_picker();
    app.toast(crate::tui::state::ToastVariant::Info, "Saved");
    let area = Rect::new(0, 0, 120, 30);
    let rect = render::toast_rects(&app, area)[0];

    assert!(apply_hover(&mut app, rect.x + 1, rect.y + 1, area));
    // Even with the picker open, the topmost toast wins the probe.
    assert_eq!(app.hover, HoverTarget::Toast(0));
}

#[test]
fn dialog_list_hover_targets_rows_without_selecting() {
    let mut app = app_with_entries(1);
    app.begin_edit_tags();
    set_tag_dialog_items(&mut app, 5);
    let area = Rect::new(0, 0, 120, 20);
    let layout = render::metadata_dialog_layout(&app.appearance.theme, area, 5);

    // The third row: hover targets it, but selection and toggles stay put.
    assert!(apply_hover(
        &mut app,
        layout.list.x,
        layout.list.y + 2,
        area
    ));
    assert_eq!(app.hover, HoverTarget::DialogRow(2));
    let state = app.edit_metadata_state().unwrap();
    assert_eq!(state.selected_index(), Some(0));
    assert!(state.selected.is_empty());
}

#[test]
fn confirm_delete_hover_targets_the_buttons() {
    let mut app = app_with_entries(1);
    let ctx = crate::tui::state::DeleteContext::Entry { has_body: true };
    app.overlay = crate::tui::state::Overlay::ConfirmDelete(
        crate::tui::state::DeleteContext::Entry { has_body: true },
        false,
    );
    let area = Rect::new(0, 0, 120, 20);
    let inner = render::confirm_delete_inner(&app.appearance.theme, area, &ctx);

    // Probe every cell of the buttons row until each button is found.
    let mut saw = (false, false);
    for col in inner.x..inner.x + inner.width {
        for row in inner.y..inner.y + inner.height {
            apply_hover(&mut app, col, row, area);
            match app.hover {
                HoverTarget::ConfirmButton(true) => saw.0 = true,
                HoverTarget::ConfirmButton(false) => saw.1 = true,
                _ => {}
            }
        }
    }
    assert!(saw.0 && saw.1, "both confirm buttons hoverable: {saw:?}");
}

#[test]
fn confirm_delete_enter_commits_the_selected_button() {
    let mut app = app_with_entries(1);
    app.begin_confirm_delete();

    // Safe default: Cancel is selected, so a bare Enter cancels rather than deletes.
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Enter), true),
        Some(Action::Overlay(OverlayAction::Cancel))
    );
    // The y/n shortcuts still fire directly, whatever the selection.
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('y')), true),
        Some(Action::Browser(BrowserAction::ConfirmDelete))
    );
    // Left picks the destructive button, Right the safe one.
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Left), true),
        Some(Action::Overlay(OverlayAction::ConfirmSelect(true)))
    );

    // With Delete selected, Enter commits the delete.
    app.overlay = crate::tui::state::Overlay::ConfirmDelete(
        crate::tui::state::DeleteContext::Entry { has_body: true },
        true,
    );
    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Enter), true),
        Some(Action::Browser(BrowserAction::ConfirmDelete))
    );
}

#[test]
fn theme_picker_cycles_chrome_and_cancel_restores_it() {
    use crate::tui::theme::ChromeStyle;
    let mut app = app_with_journals(&["work"]);
    app.open_theme_picker();
    assert_eq!(app.appearance.chrome_override, None);

    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('b')), true),
        Some(Action::Settings(SettingsAction::ThemePickerCycleChrome))
    );

    // auto → flat → bordered → auto, previewing live.
    app.theme_picker_cycle_chrome();
    assert_eq!(app.appearance.chrome_override, Some(ChromeStyle::Flat));
    app.theme_picker_cycle_chrome();
    assert_eq!(app.appearance.chrome_override, Some(ChromeStyle::Bordered));

    // Cancel restores the override from open time along with the theme.
    app.theme_picker_cancel();
    assert_eq!(app.appearance.chrome_override, None);
}

#[test]
fn theme_picker_confirm_persists_the_chrome_override() {
    use crate::tui::theme::ChromeStyle;
    let mut app = app_with_journals(&["work"]);
    app.open_theme_picker();
    app.theme_picker_cycle_chrome();
    app.theme_picker_confirm();
    assert_eq!(
        app.services.config.ui.chrome,
        crate::config::ChromeMode::Flat
    );
    assert_eq!(app.appearance.chrome_override, Some(ChromeStyle::Flat));
    // The saved config round-trips the setting.
    let loaded = crate::config::load_config(&app.services.config_path).unwrap();
    assert_eq!(loaded.ui.chrome, crate::config::ChromeMode::Flat);
}

#[test]
fn theme_picker_cycles_color_mode_and_cancel_restores_it() {
    use crate::config::ColorMode;
    use crate::tui::theme::Mode;
    let mut app = app_with_journals(&["work"]);
    app.open_theme_picker();
    assert_eq!(app.appearance.color_mode, ColorMode::Auto);

    assert_eq!(
        keyboard::key_to_action(&app, key(KeyCode::Char('m')), true),
        Some(Action::Settings(SettingsAction::ThemePickerCycleMode))
    );

    // auto → dark → light → auto, previewing live; the resolved mode follows
    // (auto falls back to dark with no detected terminal background).
    app.theme_picker_cycle_mode();
    assert_eq!(app.appearance.color_mode, ColorMode::Dark);
    app.theme_picker_cycle_mode();
    assert_eq!(app.appearance.color_mode, ColorMode::Light);
    assert_eq!(app.appearance.mode(), Mode::Light);

    // A mode change re-resolves the picker rows against the new variant.
    let journal_light = app
        .theme_picker_state()
        .and_then(|state| state.entries.iter().find(|entry| entry.name == "journal"))
        .and_then(|entry| entry.theme.clone())
        .expect("bundled journal theme resolves");
    assert_eq!(
        journal_light.base_bg(),
        ratatui::style::Color::Rgb(0xfc, 0xfc, 0xfc),
        "journal rows must re-resolve to the light variant"
    );

    // Cancel restores the mode from open time along with the theme.
    app.theme_picker_cancel();
    assert_eq!(app.appearance.color_mode, ColorMode::Auto);
}

#[test]
fn theme_picker_confirm_persists_the_color_mode() {
    use crate::config::ColorMode;
    let mut app = app_with_journals(&["work"]);
    app.open_theme_picker();
    app.theme_picker_cycle_mode();
    app.theme_picker_confirm();
    assert_eq!(app.services.config.ui.color_mode, ColorMode::Dark);
    // The saved config round-trips the setting.
    let loaded = crate::config::load_config(&app.services.config_path).unwrap();
    assert_eq!(loaded.ui.color_mode, ColorMode::Dark);
}

#[test]
fn theme_picker_hides_the_mode_switch_on_mode_agnostic_themes() {
    use crate::config::ColorMode;
    let mut app = app_with_journals(&["work"]);
    app.open_theme_picker();

    // classic resolves identically in both modes, so the switch would be a
    // no-op there; blossom has real variants.
    let position = |app: &crate::tui::app::AppModel, name: &str| {
        app.theme_picker_state()
            .unwrap()
            .entries
            .iter()
            .position(|entry| entry.name == name)
            .unwrap_or_else(|| panic!("bundled theme '{name}' listed"))
    };
    let classic = position(&app, "classic");
    app.theme_picker_select(classic);
    let state = app.theme_picker_state().unwrap();
    assert!(!state.mode_switchable());
    assert!(
        render::theme_picker_hints(
            state.hint_state(app.appearance.chrome_override, app.appearance.color_mode)
        )
        .iter()
        .all(|hint| hint.id != render::HintId::ThemePickerMode),
        "mode hint should be hidden on classic"
    );
    // The key is a no-op while the hint is hidden.
    app.theme_picker_cycle_mode();
    assert_eq!(app.appearance.color_mode, ColorMode::Auto);

    // A variant theme shows the switch again.
    let blossom = position(&app, "blossom");
    app.theme_picker_select(blossom);
    assert!(app.theme_picker_state().unwrap().mode_switchable());

    app.theme_picker_cancel();
}

// ── Unified wheel routing ─────────────────────────────────────────────────────

// Every wheel event — a single notch or a coalesced burst — flows through the one
// `wheel_to_action` router, so overlays and the editor collapse a reversed momentum
// burst to a single net step exactly like the main panels do.

#[test]
fn wheel_over_help_overlay_routes_the_net_delta() {
    let mut app = app_with_entries(1);
    app.open_help();
    let view = crate::tui::ui::ViewState::default();
    let area = Rect::new(0, 0, 80, 20);

    // A five-up / two-down burst nets -3 and is applied once, not replayed notch
    // by notch behind its own momentum tail.
    assert_eq!(
        mouse::wheel_to_action(&app, mouse(MouseEventKind::ScrollUp, 5, 5), -3, area, &view),
        Some(Action::Overlay(OverlayAction::HelpScroll(-3)))
    );
}

#[test]
fn wheel_over_dialog_list_routes_the_net_delta_to_the_list() {
    use crate::tui::ui::{DialogId, InteractionKind};

    let mut app = app_with_journals(&["work"]);
    app.open_theme_picker();
    let area = Rect::new(0, 0, 90, 30);
    let (_, view) = render_view(&mut app, area.width, area.height);
    let (col, row) = find_interaction(&view, area.width, area.height, |kind| {
        matches!(
            kind,
            InteractionKind::DialogRow {
                dialog: DialogId::ThemePicker,
                ..
            }
        )
    })
    .expect("theme picker row registered");

    let action = mouse::wheel_to_action(
        &app,
        mouse(MouseEventKind::ScrollDown, col, row),
        4,
        area,
        &view,
    );
    assert!(
        matches!(
            action,
            Some(Action::Mouse(action::MouseAction::DialogScroll {
                target: action::DialogListTarget::ThemePicker,
                delta: 4,
                ..
            }))
        ),
        "wheel over the list scrolls it by the net delta: {action:?}"
    );
}

#[test]
fn wheel_routes_by_editor_prompt_state() {
    use crate::tui::editor_state::EditorPrompt;

    let mut app = app_with_entries(1);
    app.select_entry_index(0);
    app.open_editor_for_selected().unwrap();
    let view = crate::tui::ui::ViewState::default();
    let area = Rect::new(0, 0, 80, 24);
    let wheel = mouse(MouseEventKind::ScrollDown, 10, 6);

    // Editor body: the net delta scrolls the text.
    assert_eq!(
        mouse::wheel_to_action(&app, wheel, 3, area, &view),
        Some(Action::Editor(EditorAction::Scroll(3)))
    );

    // The Help prompt scrolls the cheatsheet, still by the net delta.
    app.editor.as_mut().unwrap().prompt = EditorPrompt::Help { scroll: 0 };
    assert_eq!(
        mouse::wheel_to_action(&app, wheel, 3, area, &view),
        Some(Action::Editor(EditorAction::ScrollHelp(3)))
    );

    // Any other modal prompt swallows the wheel rather than scrolling the body
    // behind it.
    app.editor.as_mut().unwrap().prompt = EditorPrompt::MetadataMenu;
    assert_eq!(mouse::wheel_to_action(&app, wheel, 3, area, &view), None);
}

#[test]
fn wheel_over_confirm_dialog_is_a_no_op() {
    let mut app = app_with_entries(1);
    app.begin_confirm_delete();
    let view = crate::tui::ui::ViewState::default();
    let area = Rect::new(0, 0, 80, 20);

    // A confirm dialog has nothing to scroll, so the wheel is dropped on both the
    // coalesced and per-event paths.
    assert_eq!(
        mouse::wheel_to_action(
            &app,
            mouse(MouseEventKind::ScrollDown, 5, 5),
            2,
            area,
            &view
        ),
        None
    );
}
