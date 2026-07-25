//! The settings dialog: one scrollable list with the categories as sub-headers
//! and their settings toggled/adjusted in place. Layout and hit-testing derive
//! from the same geometry so drawing and clicking never drift. The highlighted
//! setting's description sits inside the frame, above the hint bar.

use super::*;

use crate::config::Config;
use crate::tui::entry_rows::{DividerAlign, section_divider, wrap_text};
use crate::tui::features::settings::{SettingCategory, SettingRow};
use crate::tui::state::{SettingsItem, SettingsState};

const SETTINGS_DIALOG_WIDTH: u16 = 52;
const SETTINGS_MAX_VISIBLE_ROWS: u16 = 16;
/// A blank row between the list and the description block.
const SETTINGS_DESC_SPACER: u16 = 1;
/// A blank row between the description block and the hint bar.
const SETTINGS_HINTS_SPACER: u16 = 1;
/// Cap on the rows the description block reserves, so a very long description
/// can't dominate the dialog.
const SETTINGS_DESC_MAX_ROWS: usize = 4;

const SETTINGS_DIALOG_HINTS: [Hint; 3] = [
    Hint::new("toggle", "enter", HintId::SettingsActivate),
    Hint::new("adjust", "←→", HintId::SettingsAdjust),
    Hint::new("close", "esc", HintId::CancelOverlay),
];

pub(crate) fn settings_dialog_hints() -> &'static [Hint] {
    &SETTINGS_DIALOG_HINTS
}

fn settings_dialog_hint_height(theme: &Theme, frame_area: Rect) -> u16 {
    hint_height(
        &SETTINGS_DIALOG_HINTS,
        dialog_hint_width(theme, frame_area, SETTINGS_DIALOG_WIDTH),
    )
}

/// Rows reserved for the highlighted setting's wrapped description: the tallest
/// description across every setting, so the block never resizes as the cursor
/// moves (and short ones simply leave blank rows).
fn settings_description_rows(theme: &Theme, frame_area: Rect) -> u16 {
    let probe = centered_rect_fixed_size(SETTINGS_DIALOG_WIDTH, 1, frame_area);
    let width = dialog_content_full(theme, probe).width.max(1) as usize;
    SettingCategory::ALL
        .iter()
        .flat_map(|category| category.rows().iter())
        .map(|row| wrap_text(row.description(), width, SETTINGS_DESC_MAX_ROWS).len())
        .max()
        .unwrap_or(1)
        .clamp(1, SETTINGS_DESC_MAX_ROWS) as u16
}

fn settings_dialog_area(
    theme: &Theme,
    frame_area: Rect,
    rows: usize,
    desc_rows: u16,
    hint_height: u16,
) -> Rect {
    let visible = (rows as u16).clamp(1, SETTINGS_MAX_VISIBLE_ROWS);
    let h = (dialog_frame_rows(theme)
        + visible
        + SETTINGS_DESC_SPACER
        + desc_rows
        + SETTINGS_HINTS_SPACER
        + hint_height)
        .min(frame_area.height.saturating_sub(2));
    centered_rect_fixed_size(SETTINGS_DIALOG_WIDTH, h, frame_area)
}

#[derive(Clone, Copy)]
pub(crate) struct SettingsDialogLayout {
    pub(crate) area: Rect,
    pub(crate) list: Rect,
    pub(crate) description: Rect,
    pub(crate) hints: Rect,
}

pub(crate) fn settings_dialog_layout(
    theme: &Theme,
    frame_area: Rect,
    state: &SettingsState,
) -> SettingsDialogLayout {
    let rows = state.items.len();
    let hint_height = settings_dialog_hint_height(theme, frame_area);
    let desc_rows = settings_description_rows(theme, frame_area);
    let area = settings_dialog_area(theme, frame_area, rows, desc_rows, hint_height);
    let inner = dialog_content_full(theme, area);
    let list_height = inner
        .height
        .saturating_sub(SETTINGS_DESC_SPACER + desc_rows + SETTINGS_HINTS_SPACER + hint_height);
    let list = Rect {
        x: inner.x,
        y: inner.y,
        width: dialog_list_width(theme, inner.width, rows, list_height),
        height: list_height,
    };
    let description = Rect {
        x: inner.x,
        y: inner.y + list_height + SETTINGS_DESC_SPACER,
        width: inner.width,
        height: desc_rows,
    };
    let hints = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(hint_height),
        width: inner.width,
        height: hint_height,
    };

    SettingsDialogLayout {
        area,
        list,
        description,
        hints,
    }
}

pub(in crate::tui::render) fn draw_settings_dialog(
    theme: &Theme,
    frame: &mut Frame<'_>,
    state: &mut SettingsState,
    config: &Config,
    hover: HoverTarget,
) {
    let layout = settings_dialog_layout(theme, frame.area(), state);

    state.normalize_list_state();
    let rows_len = state.items.len();
    let max_visible = layout.list.height;
    let max_offset = rows_len.saturating_sub(max_visible as usize);
    let scroll = state.offset().min(max_offset);
    state.list.set_offset(scroll);

    draw_dialog_frame_wide(theme, frame, layout.area, "Settings", true);

    let hovered_row = hovered_dialog_row(hover);
    let selected = state.selected_index();
    let items: Vec<ListItem<'_>> = state
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| match item {
            SettingsItem::Spacer => ListItem::new(Line::default()),
            // Headers are never selected; the same labelled divider the entry
            // list uses for its month/archive sections groups the rows beneath.
            SettingsItem::Header(category) => ListItem::new(section_divider(
                theme,
                layout.list.width as usize,
                category.title(),
                DividerAlign::Left,
            )),
            SettingsItem::Row {
                category,
                index: row_index,
            } => {
                let row = &category.rows()[*row_index];
                // Indent the row a step under its header; the Theme row trails
                // the themable disclosure glyph, since it opens the picker.
                let label = match row {
                    SettingRow::Theme => {
                        format!("  {} {}", row.label(), theme.glyphs().collapsed)
                    }
                    _ => format!("  {}", row.label()),
                };
                let item = ListItem::new(dot_leader_line(
                    theme,
                    Span::raw(label),
                    Span::styled(row.value(config), theme.muted()),
                    layout.list.width,
                    Some(index) == selected,
                ));
                if Some(index) == hovered_row && Some(index) != selected {
                    item.style(theme.hover())
                } else {
                    item
                }
            }
        })
        .collect();

    let list = List::new(items).highlight_style(theme.selection());
    let mut render_state = list_state_for_render(selected, scroll, layout.list.height, true);
    frame.render_stateful_widget(list, layout.list, &mut render_state);

    // The highlighted setting's description, wrapped to the reserved block.
    let description = state
        .selected_row()
        .map(|(_, row)| row.description())
        .unwrap_or_default();
    let desc_lines: Vec<Line<'_>> = wrap_text(
        description,
        layout.description.width as usize,
        layout.description.height as usize,
    )
    .into_iter()
    .map(|line| Line::from(Span::styled(line, theme.muted())))
    .collect();
    frame.render_widget(Paragraph::new(desc_lines), layout.description);

    render_hint_line(theme, frame, &SETTINGS_DIALOG_HINTS, layout.hints, hover);
    render_dialog_list_scrollbar(theme, frame, layout.list, rows_len, scroll, true);
}
