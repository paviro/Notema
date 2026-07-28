//! Overlay menus and their tables: editor shortcuts, the help cheatsheet, the
//! settings dialog, and the theme picker.

use super::*;

/// The shortcut overlay lists every group's bindings as a centered table with
/// vertical rules between the columns.
#[test]
fn editor_shortcuts_list_grouped_bindings() {
    let text = render_to_text(90, 44, |frame| {
        super::menus::draw_editor_shortcuts(
            &theme::Theme::terminal_default(),
            frame,
            crate::tui::state::HoverTarget::None,
            &mut 0,
        )
    });
    assert!(text.contains("Editor Shortcuts"));
    // A group header, plus bindings from each section.
    assert!(text.contains("File"));
    assert!(text.contains("Save"));
    assert!(text.contains("Select all"));
    assert!(text.contains("Cut to line end"));
    assert!(text.contains("Paragraph"));
    // The emacs bindings the textarea honors are listed too.
    assert!(text.contains("Emacs"));
    assert!(text.contains("Delete to line start"));
    // The columns are split by a vertical rule.
    assert!(text.contains('│'));
}

#[test]
fn editor_metadata_menu_registers_row_and_close_regions() {
    use crate::tui::ui::{DialogId, InteractionKind};

    let mut app = app_with_entries(1);
    app.select_entry_index(0);
    app.open_editor_for_selected().unwrap();
    app.editor.as_mut().unwrap().prompt = crate::tui::editor_state::EditorPrompt::MetadataMenu;
    let area = Rect::new(0, 0, 64, 30);
    let mut view = crate::tui::ui::ViewState::default();
    render_backend(area.width, area.height, |frame| {
        draw_app(frame, &mut app, &mut view)
    });

    // Every menu row (tags/people/activities/feelings/mood/location) and the
    // close footer must be resolvable through the interaction map — this is
    // what the mouse click path dispatches from.
    let mut seen = [false; 6];
    let mut close = false;
    for row in 0..area.height {
        for col in 0..area.width {
            match view.interactions.hit(col, row) {
                Some(InteractionKind::DialogRow {
                    dialog: DialogId::EditorMetadataMenu,
                    index,
                }) if *index < 6 => seen[*index] = true,
                Some(InteractionKind::Hint(crate::tui::render::HintId::CancelOverlay)) => {
                    close = true
                }
                _ => {}
            }
        }
    }
    assert_eq!(seen, [true; 6], "all six menu rows registered");
    assert!(close, "close footer registered");
}

/// The hint grid spreads its leftover width across the gaps, so laying the
/// hit-test out at a different origin or width than the draw shifts every chip.
/// The first chip is where that shows: its leftmost cell has to be live.
#[test]
fn dialog_hint_regions_start_where_the_chips_are_drawn() {
    use crate::tui::ui::InteractionKind;

    let mut app = app_with_entries(3);
    app.begin_filter();
    let area = Rect::new(0, 0, 80, 30);
    let mut view = crate::tui::ui::ViewState::default();
    let backend = render_backend(area.width, area.height, |frame| {
        draw_app(frame, &mut app, &mut view)
    });

    let state = app.filter_state().unwrap();
    let hints = dialogs::filter_dialog_layout(&app.appearance.theme, area, state).hints;
    let buffer = backend.buffer();

    // The key chip is the only reversed run on the row, and its leading pad is a
    // styled space — so the style, not the glyph, marks where the chip starts.
    let chip_style = footer::key_chip_style(&app.appearance.theme);
    let mut checked = 0;
    for y in hints.y..hints.y + hints.height {
        let Some(x) = (hints.x..hints.x + hints.width)
            .find(|x| buffer[(*x, y)].modifier.contains(chip_style.add_modifier))
        else {
            continue;
        };
        assert!(
            matches!(view.interactions.hit(x, y), Some(InteractionKind::Hint(_))),
            "the chip drawn at ({x}, {y}) is not clickable at its first cell"
        );
        checked += 1;
    }
    assert!(checked > 0, "no hint chips were found to check");
}

#[test]
fn help_cheatsheet_lists_grouped_bindings() {
    use crate::tui::state::HelpTab;

    let text = render_to_text(72, 44, |frame| {
        menus::draw_help(
            &theme::Theme::terminal_default(),
            frame,
            HelpTab::Shortcuts,
            crate::tui::state::HoverTarget::None,
            &mut 0,
        )
    });
    assert!(text.contains("Help"));
    // A bare metadata key the footer does not advertise, plus a grouped label.
    assert!(text.contains("Tags"));
    assert!(text.contains("Metadata"));
    // The global bindings the trimmed footer dropped still live here.
    assert!(text.contains("Settings"));
    assert!(text.contains("Quit"));
    assert!(text.contains("This help"));
    // The cheatsheet is the only place the two reload gestures are told apart —
    // neither is in the footer.
    assert!(text.contains("Refresh"));
    assert!(text.contains("Rebuild cache"));

    // The search prefixes are documented on the Search tab, nowhere else. Wide
    // enough for the two-column grid, so the whole tab sits above the fold —
    // at 72 columns it is one column and the last rows need scrolling.
    let search = render_to_text(100, 44, |frame| {
        menus::draw_help(
            &theme::Theme::terminal_default(),
            frame,
            HelpTab::Search,
            crate::tui::state::HoverTarget::None,
            &mut 0,
        )
    });
    assert!(search.contains("date:"));
    assert!(search.contains("mood:"));
    // Chaining, value operators, and quoting are documented here too.
    assert!(search.contains("Combine"));
    assert!(search.contains("Every filter matches"));
    assert!(search.contains("Any value matches"));
    assert!(search.contains("Exactly x, not xy"));
}

/// On a terminal too short to show every row, the cheatsheet scrolls rather than
/// clipping: the footer stays pinned and a binding below the fold is reachable.
#[test]
fn help_cheatsheet_scrolls_when_the_terminal_is_short() {
    // "This help" (the `?` binding) sits low in its column, so it is below the
    // fold at scroll 0 on a short terminal but reachable once scrolled.
    let top = render_to_text(72, 14, |frame| {
        menus::draw_help(
            &theme::Theme::terminal_default(),
            frame,
            crate::tui::state::HelpTab::Shortcuts,
            crate::tui::state::HoverTarget::None,
            &mut 0,
        )
    });
    let bottom = render_to_text(72, 14, |frame| {
        menus::draw_help(
            &theme::Theme::terminal_default(),
            frame,
            crate::tui::state::HelpTab::Shortcuts,
            crate::tui::state::HoverTarget::None,
            &mut 9999,
        )
    });

    // The footer is pinned in both — the box never clips it away.
    assert!(top.contains("switch"));
    assert!(bottom.contains("switch"));

    // Scrolling actually moves the viewport and brings the hidden row into view.
    assert!(
        !top.contains("This help"),
        "row should be below the fold:\n{top}"
    );
    assert!(
        bottom.contains("This help"),
        "scrolled view should reveal it:\n{bottom}"
    );
}

#[test]
fn balanced_splits_minimizes_the_tallest_column() {
    use super::menus::{balanced_splits, column_span};

    // Brute-force the minimum tallest-column span over every contiguous 3-way cut
    // and confirm the splitter matches it, with well-formed boundaries.
    let sizes = [6usize, 5, 7, 9, 4, 9];
    let bounds = balanced_splits(&sizes, 3);
    assert_eq!(bounds.len(), 3, "one boundary per column");
    assert_eq!(
        *bounds.last().unwrap(),
        sizes.len(),
        "last boundary spans all"
    );
    assert!(
        bounds.windows(2).all(|w| w[0] < w[1]) && bounds[0] > 0,
        "boundaries strictly increasing, so no column is empty: {bounds:?}"
    );

    let mut brute = usize::MAX;
    for a in 1..sizes.len() - 1 {
        for b in a + 1..sizes.len() {
            brute = brute.min(column_span(&sizes, &[a, b, sizes.len()]));
        }
    }
    assert_eq!(
        column_span(&sizes, &bounds),
        brute,
        "picks the minimax split"
    );

    // Edge cases: one column is the whole list; more columns than sections gives
    // one section each; empty input is a single empty column.
    assert_eq!(balanced_splits(&sizes, 1), vec![sizes.len()]);
    assert_eq!(balanced_splits(&[3, 4], 5), vec![1, 2]);
    assert_eq!(balanced_splits(&[], 3), vec![0]);
}

#[test]
fn settings_dialog_lists_categories_rows_and_description() {
    use crate::tui::ui::{DialogId, InteractionKind};

    let mut app = app_with_journals(&["work"]);
    app.open_settings();
    let area = Rect::new(0, 0, 72, 30);
    let mut view = crate::tui::ui::ViewState::default();
    let text = render_to_text(area.width, area.height, |frame| {
        draw_app(frame, &mut app, &mut view)
    });

    // One dialog with the three categories as sub-headers, their settings and
    // values, and the highlighted (Theme) row's description inside the frame.
    assert!(text.contains("Settings"));
    assert!(text.contains("Appearance"));
    assert!(text.contains("Reader"));
    assert!(text.contains("Editor"));
    assert!(text.contains("Theme"));
    assert!(text.contains("Center body vertically"));
    assert!(text.contains("Show link URLs"));
    assert!(text.contains("Choose the color theme"));

    // Every setting row registers a clickable region; the three sub-headers do
    // not, so the count matches the number of settings, not the line count.
    let setting_rows: usize = crate::tui::features::settings::SettingCategory::ALL
        .iter()
        .map(|category| category.rows().len())
        .sum();
    let mut rows = std::collections::BTreeSet::new();
    let mut close_found = false;
    for row in 0..area.height {
        for col in 0..area.width {
            match view.interactions.hit(col, row) {
                Some(InteractionKind::DialogRow {
                    dialog: DialogId::Settings,
                    index,
                }) => {
                    rows.insert(*index);
                }
                // The close affordance is the hint bar's "close esc" chip.
                Some(InteractionKind::Hint(crate::tui::render::HintId::CancelOverlay)) => {
                    close_found = true
                }
                _ => {}
            }
        }
    }
    assert_eq!(rows.len(), setting_rows, "every setting row registered");
    // The Appearance sub-header (item 0) is never a clickable row.
    assert!(!rows.contains(&0), "sub-header is inert");
    assert!(close_found, "close (esc) hint registered");
}

#[test]
fn theme_picker_lists_bundled_themes_with_the_active_row_marked() {
    let mut app = app_with_journals(&["work"]);
    // Pin a configured theme near the top of the sorted list so the active row
    // is on screen without scrolling — keeps this test independent of whatever
    // the default theme happens to be (the list caps at 14 visible rows).
    app.services.config.ui.theme = "blossom".to_string();
    app.open_theme_picker();

    let mut view = crate::tui::ui::ViewState::default();
    let rows = render_to_rows(90, 30, |frame| draw_app(frame, &mut app, &mut view));
    let text = rows.join("\n");

    // Dialog frame with its scope-naming title and hint row. With no per-journal
    // theme on "work", the picker opens in Global scope.
    assert!(
        text.contains("Theme · global"),
        "dialog title missing:\n{text}"
    );
    assert!(text.contains("enter  apply"));
    assert!(text.contains("esc  revert"));
    // The bundled themes at the top of the list render; the configured one is
    // annotated as the global default.
    for name in ["blossom", "classic", "eclipse", "fjord", "grove"] {
        assert!(text.contains(name), "theme '{name}' missing:\n{text}");
    }
    assert!(
        text.contains("blossom  (global)"),
        "global-default annotation missing:\n{text}"
    );
}

#[test]
fn theme_picker_renders_broken_rows_in_the_error_style() {
    let mut app = app_with_journals(&["work"]);
    let themes = crate::tui::theme::themes_dir(&app.services.config_path);
    fs::create_dir_all(&themes).unwrap();
    fs::write(themes.join("busted.toml"), "surfaces = 12\n").unwrap();
    app.open_theme_picker();
    let state = app.theme_picker_state().unwrap();
    let (len, hint_inputs) = (
        state.entries.len(),
        state.hint_state(app.appearance.chrome_override, app.appearance.color_mode),
    );
    let active_theme = app.appearance.theme.clone();

    let backend = render_app(app, 90, 30);
    let buffer = backend.buffer();
    let rows: Vec<String> = buffer
        .content()
        .chunks(90)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect())
        .collect();
    let (y, line) = rows
        .iter()
        .enumerate()
        .find(|(_, line)| line.contains("busted (broken)"))
        .expect("broken row rendered");
    let x = line.find("busted").unwrap() as u16;

    let error_fg = active_theme.error().fg.unwrap();
    assert_eq!(buffer[(x, y as u16)].fg, error_fg);
    // The layout the mouse handler uses matches where the list was drawn.
    let layout = theme_picker_layout(&active_theme, Rect::new(0, 0, 90, 30), len, hint_inputs);
    assert!(point_in_rect(layout.list, x, y as u16));
}
