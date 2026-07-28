//! Dialog sizing and contents: list widths, hint rows, feelings, confirm
//! delete, edit tags, and the filter browser.

use super::*;

fn render_edit_tags_dialog_text(state: EditMetadataState, width: u16, height: u16) -> String {
    render_edit_tags_dialog_text_with_theme(&theme::Theme::terminal_default(), state, width, height)
}

fn render_confirm_delete_rows(width: u16, height: u16) -> Vec<String> {
    render_to_rows(width, height, |frame| {
        dialogs::draw_confirm_delete(
            &theme::Theme::terminal_default(),
            frame,
            &crate::tui::state::DeleteContext::Entry { has_body: true },
            false,
            None,
        )
    })
}

#[test]
fn list_dialogs_keep_preferred_width_until_they_hit_edges() {
    let wide_tags = metadata_dialog_layout(
        &crate::tui::theme::Theme::terminal_default(),
        Rect::new(0, 0, 120, 30),
        20,
    );
    assert_eq!(wide_tags.area.width, 44);
    assert_eq!(wide_tags.list.height, 14);

    let narrow_tags = metadata_dialog_layout(
        &crate::tui::theme::Theme::terminal_default(),
        Rect::new(0, 0, 40, 30),
        20,
    );
    assert_eq!(narrow_tags.area.x, 0);
    assert_eq!(narrow_tags.area.width, 40);

    // 15 group headers + 155 feelings = 170 rows; the list caps at its max visible rows.
    let wide_feelings = feelings_dialog_layout(
        &crate::tui::theme::Theme::terminal_default(),
        Rect::new(0, 0, 120, 30),
        170,
        &[],
    );
    assert_eq!(wide_feelings.area.width, 44);
    assert_eq!(wide_feelings.list.height, 16);

    let wide_mood = mood_dialog_layout(
        &crate::tui::theme::Theme::terminal_default(),
        Rect::new(0, 0, 120, 30),
    );
    assert_eq!(wide_mood.area.width, 90);

    let narrow_mood = mood_dialog_layout(
        &crate::tui::theme::Theme::terminal_default(),
        Rect::new(0, 0, 80, 30),
    );
    assert_eq!(narrow_mood.area.x, 0);
    assert_eq!(narrow_mood.area.width, 80);
}

/// The mood dialog is 90 columns wide, so its five hints fit on one row. It used
/// to size its hint block against a 44-column probe and reserve three, leaving
/// two dead rows under the bar.
#[test]
fn mood_dialog_reserves_only_the_hint_rows_it_draws() {
    for theme in [
        theme::Theme::terminal_default(),
        theme::test_flat_theme().with_chrome_override(Some(crate::tui::theme::ChromeStyle::Flat)),
    ] {
        let layout = mood_dialog_layout(&theme, Rect::new(0, 0, 120, 30));
        assert_eq!(
            layout.hints.height, 1,
            "five hints fit one row at 90 columns"
        );
        assert_eq!(
            layout.hints.y + layout.hints.height,
            layout.inner.y + layout.inner.height
        );
    }
}

/// The picker's chrome/mode hints carry live labels, and the wider ones ("chrome:
/// bordered", "mode: light") can push the grid onto more rows than the narrow
/// defaults. Sizing reads the same labels the draw does, so nothing is clipped.
#[test]
fn theme_picker_reserves_hint_rows_for_the_live_labels() {
    use crate::config::ColorMode;
    use crate::tui::theme::ChromeStyle;

    let mut app = app_with_journals(&["work"]);
    app.open_theme_picker();
    let state = app.theme_picker_state().unwrap();
    let len = state.entries.len();
    let active = app.appearance.theme.clone();

    let narrow = state.hint_state(None, ColorMode::Auto);
    let widest = state.hint_state(Some(ChromeStyle::Bordered), ColorMode::Light);
    let area = Rect::new(0, 0, 90, 30);

    for inputs in [narrow, widest] {
        let layout = theme_picker_layout(&active, area, len, inputs);
        let drawn = footer::hint_height(&theme_picker_hints(inputs), layout.hints.width);
        assert_eq!(
            layout.hints.height, drawn,
            "the reserved block matches the rows the hints wrap onto"
        );
    }
}

#[test]
fn feelings_dialog_folds_groups_and_marks_disclosure() {
    use crate::tui::features::feelings::EditFeelingState;
    use notema_domain::{Feeling, FeelingGroup};

    static GROUPS: &[FeelingGroup] = &[
        FeelingGroup {
            name: "Peaceful",
            feelings: &[
                Feeling {
                    name: "calm",
                    search_aliases: &[],
                },
                Feeling {
                    name: "content",
                    search_aliases: &[],
                },
            ],
        },
        FeelingGroup {
            name: "Joyful",
            feelings: &[Feeling {
                name: "happy",
                search_aliases: &[],
            }],
        },
    ];
    let mut state = EditFeelingState::new(GROUPS, vec!["calm".into()]);
    state.expanded[1] = true;
    let rows = render_to_rows(60, 24, |frame| {
        dialogs::draw_edit_feelings_dialog(
            &theme::Theme::terminal_default(),
            frame,
            &mut state,
            crate::tui::state::HoverTarget::None,
        )
    });

    // Collapsed group: header keeps its stored casing (no all-caps), carries a
    // trailing ▸ and a selected count, and its feelings are hidden.
    let collapsed = rows.iter().find(|row| row.contains("Peaceful")).unwrap();
    assert!(
        !collapsed.contains('['),
        "header must not render a checkbox"
    );
    assert!(collapsed.contains('▸'), "collapsed header shows ▸");
    // The disclosure now trails the name; the selected count is pinned to the
    // right edge, past the arrow.
    let arrow = collapsed.find('▸').unwrap();
    let count = collapsed.rfind('1').unwrap();
    assert!(
        count > arrow,
        "collapsed header shows the selected count after the disclosure"
    );
    // "calm" appears in the selected summary; it must NOT appear as a list row.
    assert!(!rows.iter().any(|row| row.contains("[x] calm")));

    // Expanded group: ▾ marker and its feelings render with checkboxes.
    let expanded = rows.iter().find(|row| row.contains("Joyful")).unwrap();
    assert!(expanded.contains('▾'), "expanded header shows ▾");
    assert!(rows.iter().any(|row| row.contains("[ ] happy")));

    // The selected-feelings summary lists picks from any group.
    assert!(rows.iter().any(|row| row.contains("Selected: calm")));
}

#[test]
fn feelings_dialog_shows_no_matches_when_filter_is_empty() {
    use crate::tui::features::feelings::EditFeelingState;
    use notema_domain::{Feeling, FeelingGroup};

    static GROUPS: &[FeelingGroup] = &[FeelingGroup {
        name: "Peaceful",
        feelings: &[Feeling {
            name: "calm",
            search_aliases: &["composed"],
        }],
    }];
    let mut state = EditFeelingState::new(GROUPS, Vec::new());
    // A query matching neither the feeling nor its alias collapses the list.
    state.input = "zzz-nope".into();
    state.rebuild_filter();

    let rows = render_to_rows(60, 24, |frame| {
        dialogs::draw_edit_feelings_dialog(
            &theme::Theme::terminal_default(),
            frame,
            &mut state,
            crate::tui::state::HoverTarget::None,
        )
    });
    assert!(
        rows.iter().any(|row| row.contains("(no matches)")),
        "an empty filter must still surface the no-matches line"
    );
}

#[test]
fn confirm_delete_shows_message_then_buttons() {
    let rows = render_confirm_delete_rows(80, 20);
    let title_row = rows
        .iter()
        .position(|row| row.contains("Confirm Delete"))
        .unwrap();
    let message_row = rows
        .iter()
        .position(|row| row.contains("Move entry to trash?"))
        .unwrap();
    let button_row = rows
        .iter()
        .position(|row| row.contains("Delete") && row.contains("Cancel"))
        .unwrap();

    // Message sits just below the border/title; the buttons follow, below it.
    assert_eq!(message_row, title_row + 1);
    assert!(button_row > message_row);
}

#[test]
fn edit_tags_dialog_keeps_help_visible_below_spacer() {
    let all_values: Vec<(String, usize)> = (0..20)
        .map(|index| (format!("tag-{index:02}"), index))
        .collect();
    let filtered: Vec<usize> = (0..all_values.len()).collect();
    let active_len = all_values.len();
    let rendered = render_edit_tags_dialog_text(
        EditMetadataState::new(
            MetadataKind::Tags,
            all_values,
            filtered,
            Vec::new(),
            active_len,
        ),
        200,
        20,
    );

    // The count is pinned to the right edge on a dot leader, not inline. (The
    // selected row blanks its leader, so assert against a non-selected row.)
    assert!(rendered.contains("[ ] tag-00"));
    assert!(rendered.contains(". 1"));
    assert!(rendered.contains("space  toggle"));
    assert!(rendered.contains("tab  input"));
    assert!(rendered.contains("enter  save"));
    assert!(rendered.contains("esc  cancel"));
}

#[test]
fn edit_tags_dialog_keeps_list_gutter_when_selection_is_scrolled_out() {
    let all_values: Vec<(String, usize)> = (0..20)
        .map(|index| (format!("tag-{index:02}"), index))
        .collect();
    let filtered: Vec<usize> = (0..all_values.len()).collect();
    let active_len = all_values.len();
    let mut state = EditMetadataState::new(
        MetadataKind::Tags,
        all_values,
        filtered,
        Vec::new(),
        active_len,
    );
    state.list.set_offset(5);

    let rendered = render_edit_tags_dialog_text(state, 200, 20);

    // The list gutter (leading space) is preserved; the count moves to the
    // right edge on a dot leader.
    assert!(rendered.contains(" [ ] tag-05"));
    assert!(rendered.contains(". 5"));
}

#[test]
fn edit_tags_dialog_counts_no_matches_row_when_sizing() {
    let mut state = EditMetadataState::new(
        MetadataKind::Tags,
        vec![("work".to_string(), 1)],
        Vec::new(),
        Vec::new(),
        1,
    );
    state.input = "missing".into();
    state.focus = EditMetadataFocus::Input;
    let rendered = render_edit_tags_dialog_text(state, 200, 12);

    assert!(rendered.contains(" (no matches)"));
    assert!(rendered.contains("enter  add"));
    assert!(rendered.contains("tab  list"));
    assert!(rendered.contains("esc  cancel"));
}

#[test]
fn edit_metadata_input_hint_saves_when_empty_and_adds_when_not_empty() {
    let mut empty =
        EditMetadataState::new(MetadataKind::People, Vec::new(), Vec::new(), Vec::new(), 0);
    empty.focus = EditMetadataFocus::Input;
    let rendered_empty = render_edit_tags_dialog_text(empty, 200, 12);
    assert!(rendered_empty.contains("enter  save"));
    assert!(rendered_empty.contains("tab  list"));
    assert!(rendered_empty.contains("esc  cancel"));

    let mut with_value =
        EditMetadataState::new(MetadataKind::People, Vec::new(), Vec::new(), Vec::new(), 0);
    with_value.focus = EditMetadataFocus::Input;
    with_value.input = "alex".into();
    let rendered_value = render_edit_tags_dialog_text(with_value, 200, 12);
    assert!(rendered_value.contains("enter  add"));
    assert!(rendered_value.contains("tab  list"));
    assert!(rendered_value.contains("esc  cancel"));
}

#[test]
fn filter_dialog_renders_across_sizes() {
    // A comfortable size shows the framed title and the populated tabs.
    let mut app = app_with_metadata_entry();
    app.nav.focus = Focus::Journals;
    app.begin_filter();
    let text = render_text(app, 90, 24);
    assert!(text.contains("Filter —"), "missing title: {text}");
    // The dialog is sized to the full tab strip, so every full label fits — even
    // the longest ("Locations") — without collapsing to shorter labels.
    assert!(text.contains("Tags"), "missing a tab label: {text}");
    assert!(
        text.contains("Locations"),
        "full tab labels should fit: {text}"
    );

    // Tiny terminals must still render without panicking on the layout math.
    for (w, h) in [(40u16, 12u16), (20, 8), (12, 6)] {
        let mut app = app_with_metadata_entry();
        app.nav.focus = Focus::Journals;
        app.begin_filter();
        let _ = render_text(app, w, h);
    }
}
