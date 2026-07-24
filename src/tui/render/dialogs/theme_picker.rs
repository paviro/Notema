use super::*;

/// The picker's title, naming the scope being edited so the global-vs-journal
/// choice is unambiguous.
fn theme_picker_title(state: &ThemePickerState) -> String {
    use crate::tui::state::ThemePickerScope;
    match (&state.journal, state.scope) {
        (Some(journal), ThemePickerScope::Journal) => {
            format!("Theme · {}", notema_storage::journal_display_name(journal))
        }
        _ => "Theme · global".to_string(),
    }
}

/// The picker's hint row, with the chrome and mode hints' labels reflecting
/// the live `[ui]` settings so cycling them reads back immediately. The mode
/// hint only shows when the highlighted theme has dark/light variants.
pub(crate) fn theme_picker_hints(
    inputs: crate::tui::state::PickerHints,
    chrome_override: Option<crate::tui::theme::ChromeStyle>,
    color_mode: crate::config::ColorMode,
) -> Vec<Hint> {
    use crate::config::ColorMode;
    use crate::tui::theme::ChromeStyle;
    let chrome = match chrome_override {
        None => Hint::new("chrome: default", "b", HintId::ThemePickerChrome),
        Some(ChromeStyle::Flat) => Hint::new("chrome: flat", "b", HintId::ThemePickerChrome),
        Some(ChromeStyle::Bordered) => {
            Hint::new("chrome: bordered", "b", HintId::ThemePickerChrome)
        }
    };
    let mut hints = vec![
        Hint::new("apply", "enter", HintId::ThemePickerApply),
        chrome,
    ];
    if inputs.mode_switchable {
        hints.push(match color_mode {
            ColorMode::Auto => Hint::new("mode: auto", "m", HintId::ThemePickerMode),
            ColorMode::Dark => Hint::new("mode: dark", "m", HintId::ThemePickerMode),
            ColorMode::Light => Hint::new("mode: light", "m", HintId::ThemePickerMode),
        });
    }
    // Scope toggle only when there's a journal in context.
    if inputs.has_journal {
        hints.push(Hint::new("scope", "tab", HintId::ThemePickerScope));
    }
    hints.push(Hint::new("revert", "esc", HintId::ThemePickerRevert));
    hints
}

fn theme_picker_hint_height(
    theme: &Theme,
    frame_area: Rect,
    inputs: crate::tui::state::PickerHints,
) -> u16 {
    hint_height(
        &theme_picker_hints(inputs, None, crate::config::ColorMode::Auto),
        dialog_hint_width(theme, frame_area, LIST_DIALOG_WIDTH),
    )
}

fn theme_picker_area(
    theme: &Theme,
    frame_area: Rect,
    len: usize,
    inputs: crate::tui::state::PickerHints,
) -> Rect {
    let hint_height = theme_picker_hint_height(theme, frame_area, inputs);
    let visible = (len as u16).clamp(1, THEME_PICKER_MAX_VISIBLE_ROWS);
    // The frame + the list + a blank spacer + the hint block.
    let h = (dialog_frame_rows(theme) + 1 + visible + hint_height)
        .min(frame_area.height.saturating_sub(2));
    super::centered_rect_fixed_size(LIST_DIALOG_WIDTH, h, frame_area)
}

#[derive(Clone, Copy)]
pub(crate) struct ThemePickerLayout {
    pub(crate) area: Rect,
    pub(crate) list: Rect,
    pub(crate) hints: Rect,
}

/// The theme picker's geometry, shared by the draw and the mouse hit-tests so
/// the click map can't drift from the pixels.
pub(crate) fn theme_picker_layout(
    theme: &Theme,
    frame_area: Rect,
    len: usize,
    inputs: crate::tui::state::PickerHints,
) -> ThemePickerLayout {
    let area = theme_picker_area(theme, frame_area, len, inputs);
    let inner = dialog_content_full(theme, area);
    let hint_height = theme_picker_hint_height(theme, frame_area, inputs);
    // A blank spacer row separates the list from the hint block.
    let list_height = inner.height.saturating_sub(1 + hint_height);
    let list = Rect {
        x: inner.x,
        y: inner.y,
        width: dialog_list_width(theme, inner.width, len, list_height),
        height: list_height,
    };
    let hints = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(hint_height),
        width: inner.width,
        height: hint_height,
    };

    ThemePickerLayout { area, list, hints }
}

pub(crate) fn draw_theme_picker(
    theme: &Theme,
    chrome_override: Option<crate::tui::theme::ChromeStyle>,
    color_mode: crate::config::ColorMode,
    frame: &mut Frame<'_>,
    state: &mut ThemePickerState,
    hover: HoverTarget,
) {
    let hovered_row = hovered_dialog_row(hover);
    let hint_inputs = state.hint_state();
    let layout = theme_picker_layout(theme, frame.area(), state.entries.len(), hint_inputs);

    state.normalize_list_state();
    let len = state.entries.len();
    let max_visible = layout.list.height;
    let max_offset = len.saturating_sub(max_visible as usize);
    let scroll = state.offset().min(max_offset);
    state.list.set_offset(scroll);

    let items: Vec<ListItem<'_>> = if state.entries.is_empty() {
        vec![ListItem::new(Line::from("(no themes found)"))]
    } else {
        state
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                // Annotate which row is the global default and which is this
                // journal's own theme, so the scope you're editing is legible.
                let mut tags: Vec<&str> = Vec::new();
                if entry.name == state.previous_name {
                    tags.push("global");
                }
                if state
                    .journal_theme
                    .as_ref()
                    .map(|theme| theme.name.as_str())
                    == Some(entry.name.as_str())
                {
                    tags.push("this journal");
                }
                let suffix = if tags.is_empty() {
                    String::new()
                } else {
                    format!("  ({})", tags.join(", "))
                };
                let item = match entry.theme {
                    Some(_) => ListItem::new(Line::from(format!("  {}{suffix}", entry.name))),
                    None => ListItem::new(Line::from(Span::styled(
                        format!("  {} (broken){suffix}", entry.name),
                        theme.error(),
                    ))),
                };
                // The selection highlight patches over the hover lift, so the
                // hovered-and-selected row still reads as selected.
                if Some(index) == hovered_row && Some(index) != state.selected_index() {
                    item.style(theme.hover())
                } else {
                    item
                }
            })
            .collect()
    };

    draw_dialog_frame_wide(theme, frame, layout.area, &theme_picker_title(state), true);
    let list = List::new(items).highlight_style(theme.selection());
    let mut render_state =
        list_state_for_render(state.selected_index(), scroll, layout.list.height, len > 0);
    frame.render_stateful_widget(list, layout.list, &mut render_state);
    render_hint_line(
        theme,
        frame,
        &theme_picker_hints(hint_inputs, chrome_override, color_mode),
        layout.hints,
        hover,
    );
    render_dialog_list_scrollbar(theme, frame, layout.list, len, scroll, true);
}
