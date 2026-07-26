use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
};

use unicode_width::UnicodeWidthStr;

use crate::tui::entry_rows::wrap_text;
use crate::tui::features::{
    feelings::{EditFeelingState, FeelingRow},
    location::{EditLocationFocus, EditLocationState, LocationResolveStatus},
    metadata::{EditMetadataFocus, EditMetadataState},
};
use crate::tui::state::{DeleteContext, EditMoodState, HoverTarget, ListNav, ThemePickerState};
use crate::tui::surface::{metadata_value_rows, surface_outer_width};
use crate::tui::text_input::TextInput;
use crate::tui::theme::Theme;

use super::{
    chrome::{
        centered_rect_fixed_size, dot_leader_line, flat_chrome, render_dialog_list_scrollbar,
    },
    footer::{Hint, HintId, hint_height},
    frames::{
        dialog_content_full, dialog_frame_rows, dialog_hints_rect, dialog_inner,
        dialog_list_gutter, dialog_list_width, dialog_row, draw_dialog_frame,
        draw_dialog_frame_wide, render_confirm_buttons, render_hint_line, render_separator,
    },
    list_state_for_render,
    metadata::MoodBar,
};
use std::time::Instant;

mod theme_picker;
pub(super) use theme_picker::draw_theme_picker;
pub(crate) use theme_picker::{theme_picker_hints, theme_picker_layout};
mod filter;
pub(super) use filter::draw_filter_dialog;
pub(crate) use filter::{filter_dialog_hints, filter_dialog_layout};
mod settings;
pub(super) use settings::draw_settings_dialog;
pub(crate) use settings::{settings_dialog_hints, settings_dialog_layout};
mod overlays;
pub(crate) use overlays::{confirm_delete_inner, new_journal_field_rect};
pub(super) use overlays::{
    draw_confirm_delete, draw_edit_feelings_dialog, draw_edit_location_dialog,
    draw_edit_metadata_dialog, draw_edit_mood_dialog, draw_fetching_environment,
    draw_new_journal_input,
};

// ── Hint text constants and helpers ──────────────────────────────────────────

const FEELINGS_DIALOG_LIST_HINTS: [Hint; 6] = [
    Hint::new("open", "→", HintId::FeelingsExpand),
    Hint::new("close", "←", HintId::FeelingsCollapse),
    Hint::new("toggle", "space", HintId::FeelingsToggle),
    Hint::new("search", "tab", HintId::FeelingsSwitchFocus),
    Hint::new("save", "enter", HintId::FeelingsSave),
    Hint::new("cancel", "esc", HintId::CancelOverlay),
];

const FEELINGS_DIALOG_INPUT_HINTS: [Hint; 4] = [
    Hint::new("list", "tab", HintId::FeelingsSwitchFocus),
    Hint::new("select all", "^a", HintId::InputSelectAll),
    Hint::new("save", "enter", HintId::FeelingsSave),
    Hint::new("cancel", "esc", HintId::CancelOverlay),
];

const SELECTED_LABEL: &str = "Selected: ";

/// Wrap the picked feelings into display rows, reusing the entry view's
/// metadata-row layout: the "Selected: " label reserves the first row's leading
/// width, values are separated by " | ", and each row is a list of indices into
/// `selected`. Empty when nothing is picked (rendered as "Selected: none").
fn feelings_selected_rows(selected: &[String], width: u16) -> Vec<Vec<usize>> {
    metadata_value_rows(SELECTED_LABEL.len() as u16, width, selected)
}

/// Number of lines the "Selected: …" footer occupies once wrapped — at least one
/// (the "Selected: none" line when nothing is picked). Used both to size the dialog
/// and to render it, so the reserved height always matches the drawn lines.
fn feelings_selected_line_count(theme: &Theme, frame_area: Rect, selected: &[String]) -> usize {
    let area = centered_rect_fixed_size(LIST_DIALOG_WIDTH, 1, frame_area);
    feelings_selected_rows(selected, dialog_content_full(theme, area).width)
        .len()
        .max(1)
}

const MOOD_DIALOG_HINTS: [Hint; 5] = [
    Hint::new("decrease", "←", HintId::MoodDecrease),
    Hint::new("increase", "→", HintId::MoodIncrease),
    Hint::new("save", "enter", HintId::MoodSave),
    Hint::new("clear", "del", HintId::MoodClear),
    Hint::new("cancel", "esc", HintId::CancelOverlay),
];

const METADATA_DIALOG_LIST_HINTS: [Hint; 4] = [
    Hint::new("toggle", "space", HintId::MetadataToggle),
    Hint::new("input", "tab", HintId::MetadataSwitchFocus),
    Hint::new("save", "enter", HintId::MetadataSave),
    Hint::new("cancel", "esc", HintId::CancelOverlay),
];

const METADATA_DIALOG_INPUT_EMPTY_HINTS: [Hint; 3] = [
    Hint::new("save", "enter", HintId::MetadataSave),
    Hint::new("list", "tab", HintId::MetadataSwitchFocus),
    Hint::new("cancel", "esc", HintId::CancelOverlay),
];

const METADATA_DIALOG_INPUT_VALUE_HINTS: [Hint; 4] = [
    Hint::new("add", "enter", HintId::MetadataAddFromInput),
    Hint::new("list", "tab", HintId::MetadataSwitchFocus),
    Hint::new("select all", "^a", HintId::InputSelectAll),
    Hint::new("cancel", "esc", HintId::CancelOverlay),
];

const LOCATION_DIALOG_LIST_HINTS: [Hint; 4] = [
    Hint::new("pick", "enter", HintId::LocationSelectRow),
    Hint::new("edit", "tab", HintId::LocationSwitchFocus),
    Hint::new("clear", "del", HintId::LocationClear),
    Hint::new("cancel", "esc", HintId::CancelOverlay),
];

const LOCATION_DIALOG_QUERY_HINTS: [Hint; 5] = [
    Hint::new("look up", "enter", HintId::LocationResolve),
    Hint::new("locate", "^l", HintId::LocationGrabDevice),
    Hint::new("next", "tab", HintId::LocationSwitchFocus),
    Hint::new("select all", "^a", HintId::InputSelectAll),
    Hint::new("cancel", "esc", HintId::CancelOverlay),
];

/// Query-field hints once the query is resolved: Enter now saves.
const LOCATION_DIALOG_QUERY_RESOLVED_HINTS: [Hint; 5] = [
    Hint::new("save", "enter", HintId::LocationSave),
    Hint::new("locate", "^l", HintId::LocationGrabDevice),
    Hint::new("next", "tab", HintId::LocationSwitchFocus),
    Hint::new("select all", "^a", HintId::InputSelectAll),
    Hint::new("cancel", "esc", HintId::CancelOverlay),
];

const LOCATION_DIALOG_NAME_HINTS: [Hint; 5] = [
    Hint::new("save", "enter", HintId::LocationSave),
    Hint::new("locate", "^l", HintId::LocationGrabDevice),
    Hint::new("next", "tab", HintId::LocationSwitchFocus),
    Hint::new("select all", "^a", HintId::InputSelectAll),
    Hint::new("cancel", "esc", HintId::CancelOverlay),
];

const LIST_DIALOG_WIDTH: u16 = 44;
const LOCATION_DIALOG_WIDTH: u16 = 66;
const LOCATION_DIALOG_MAX_VISIBLE_ROWS: u16 = 8;
/// Cap the lines a single (wrapped) list row may occupy.
const LOCATION_LIST_MAX_ITEM_LINES: usize = 3;

/// Wrap a list label into its display lines (at least one).
fn location_row_lines(label: &str, list_width: u16) -> Vec<String> {
    let lines = wrap_text(
        label,
        list_width.max(1) as usize,
        LOCATION_LIST_MAX_ITEM_LINES,
    );
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

/// Total rows the list occupies once every label is wrapped — what the dialog is
/// sized to, so multi-line rows aren't clipped. Measured at the narrowest the
/// list can be — the width it takes once a scrollbar is drawn beside it. Whether
/// that bar appears depends on this very count, so the narrow width is the only
/// one that can't under-count: without a bar the list is wider and wraps less.
fn location_list_rows(theme: &Theme, frame_area: Rect, labels: &[String]) -> usize {
    let area = centered_rect_fixed_size(LOCATION_DIALOG_WIDTH, 1, frame_area);
    let list_width = dialog_content_full(theme, area)
        .width
        .saturating_sub(dialog_list_gutter(theme));
    labels
        .iter()
        .map(|label| location_row_lines(label, list_width).len())
        .sum::<usize>()
        .max(1)
}

/// Map a click at `row` within the list `Rect` to a label index, accounting for
/// rows that wrap onto continuation lines. `offset` is the index of the first
/// visible label. `None` when the click lands past the last rendered row.
pub(crate) fn location_list_row_at(
    list: Rect,
    labels: &[String],
    offset: usize,
    row: u16,
) -> Option<usize> {
    let relative = row.checked_sub(list.y)? as usize;
    if relative >= list.height as usize {
        return None;
    }
    let mut line = 0usize;
    for (index, label) in labels.iter().enumerate().skip(offset) {
        line += location_row_lines(label, list.width).len();
        if relative < line {
            return Some(index);
        }
    }
    None
}
const MOOD_DIALOG_WIDTH: u16 = 90;
const CONFIRM_DIALOG_WIDTH: u16 = 42;
const NEW_JOURNAL_DIALOG_WIDTH: u16 = 56;
const METADATA_DIALOG_MAX_VISIBLE_ROWS: u16 = 14;
const FEELINGS_DIALOG_MAX_VISIBLE_ROWS: u16 = 16;
const THEME_PICKER_MAX_VISIBLE_ROWS: u16 = 14;

pub(crate) fn feelings_dialog_hints(focus: EditMetadataFocus) -> &'static [Hint] {
    match focus {
        EditMetadataFocus::List => &FEELINGS_DIALOG_LIST_HINTS,
        EditMetadataFocus::Input => &FEELINGS_DIALOG_INPUT_HINTS,
    }
}

pub(crate) fn mood_dialog_hints() -> &'static [Hint] {
    &MOOD_DIALOG_HINTS
}

pub(crate) fn location_dialog_hints(
    focus: EditLocationFocus,
    query_looked_up: bool,
) -> &'static [Hint] {
    match focus {
        EditLocationFocus::Query if query_looked_up => &LOCATION_DIALOG_QUERY_RESOLVED_HINTS,
        EditLocationFocus::Query => &LOCATION_DIALOG_QUERY_HINTS,
        EditLocationFocus::Name => &LOCATION_DIALOG_NAME_HINTS,
        EditLocationFocus::List => &LOCATION_DIALOG_LIST_HINTS,
    }
}

pub(crate) fn metadata_dialog_hints(
    focus: EditMetadataFocus,
    input_is_empty: bool,
) -> &'static [Hint] {
    match (focus, input_is_empty) {
        (EditMetadataFocus::List, _) => &METADATA_DIALOG_LIST_HINTS,
        (EditMetadataFocus::Input, true) => &METADATA_DIALOG_INPUT_EMPTY_HINTS,
        (EditMetadataFocus::Input, false) => &METADATA_DIALOG_INPUT_VALUE_HINTS,
    }
}

// ── Dialog area helpers (re-used by the mouse handler for hit-testing) ───────

/// Rows inside the border that are not the list: the title, both list
/// separators, the search input, and the blank spacer above the hints. Sizing
/// the dialog and placing the list both derive from this, so they can't drift.
const METADATA_DIALOG_CHROME: u16 = 5;

/// The list's first row in the metadata and feelings dialogs, which share the
/// same title-plus-separator framing above it.
const LIST_DIALOG_LIST_TOP: u16 = 2;

pub(crate) fn metadata_dialog_area(theme: &Theme, frame_area: Rect, filtered_len: usize) -> Rect {
    let fixed = METADATA_DIALOG_CHROME + dialog_frame_rows(theme);
    let hint_height = tag_dialog_hint_height(theme, frame_area);
    let visible = (filtered_len as u16).clamp(1, METADATA_DIALOG_MAX_VISIBLE_ROWS);
    let h = (fixed + hint_height + visible).min(frame_area.height.saturating_sub(2));
    super::centered_rect_fixed_size(LIST_DIALOG_WIDTH, h, frame_area)
}

/// Height of every row inside the dialog border that is *not* the list: the
/// title, both list separators, the search input, the selected summary and the
/// two blank spacers around it, and the hint block. Sizing the dialog and placing
/// the list both derive from this one value so they can't drift apart.
fn feelings_dialog_chrome_height(theme: &Theme, frame_area: Rect, selected_lines: usize) -> u16 {
    // title + two separators + search input + spacer + summary + spacer + hints
    1 + 2 + 1 + 1 + selected_lines as u16 + 1 + feelings_dialog_hint_height(theme, frame_area)
}

pub(crate) fn feelings_dialog_area(
    theme: &Theme,
    frame_area: Rect,
    all_len: usize,
    selected_lines: usize,
) -> Rect {
    // Clamp to at least one row so the "(no matches)" line has somewhere to render
    // when a filter matches nothing, matching the metadata dialog.
    let visible = (all_len as u16).clamp(1, FEELINGS_DIALOG_MAX_VISIBLE_ROWS);
    let h = (dialog_frame_rows(theme)
        + feelings_dialog_chrome_height(theme, frame_area, selected_lines)
        + visible)
        .min(frame_area.height.saturating_sub(2));
    super::centered_rect_fixed_size(LIST_DIALOG_WIDTH, h, frame_area)
}

pub(crate) fn mood_dialog_area(theme: &Theme, frame_area: Rect) -> Rect {
    let h = 5 + dialog_frame_rows(theme) + mood_dialog_hint_height(theme, frame_area);
    super::centered_rect_fixed_size(
        MOOD_DIALOG_WIDTH,
        h.min(frame_area.height.saturating_sub(2)),
        frame_area,
    )
}

/// The width a dialog of outer `width` lays its hint grid out in, for sizing the
/// block before the dialog's own rect exists. Must match where the hints are
/// finally drawn: probe narrow and the block reserves rows the draw never fills,
/// probe wide and the last chips are clipped away.
pub(super) fn dialog_hint_width(theme: &Theme, frame_area: Rect, width: u16) -> u16 {
    let area = super::centered_rect_fixed_size(width, 1, frame_area);
    dialog_content_full(theme, area).width
}

fn tag_dialog_hint_height(theme: &Theme, frame_area: Rect) -> u16 {
    let width = dialog_hint_width(theme, frame_area, LIST_DIALOG_WIDTH);
    hint_height(&METADATA_DIALOG_LIST_HINTS, width)
        .max(hint_height(&METADATA_DIALOG_INPUT_EMPTY_HINTS, width))
        .max(hint_height(&METADATA_DIALOG_INPUT_VALUE_HINTS, width))
}

fn feelings_dialog_hint_height(theme: &Theme, frame_area: Rect) -> u16 {
    // Reserve the taller of the two focus states so the layout stays put as the
    // user tabs between the list and the search input.
    let width = dialog_hint_width(theme, frame_area, LIST_DIALOG_WIDTH);
    hint_height(&FEELINGS_DIALOG_LIST_HINTS, width)
        .max(hint_height(&FEELINGS_DIALOG_INPUT_HINTS, width))
}

fn mood_dialog_hint_height(theme: &Theme, frame_area: Rect) -> u16 {
    hint_height(
        &MOOD_DIALOG_HINTS,
        dialog_hint_width(theme, frame_area, MOOD_DIALOG_WIDTH),
    )
}

fn location_dialog_hint_height(theme: &Theme, frame_area: Rect) -> u16 {
    // Reserve the tallest focus state so the layout doesn't shift as focus moves.
    let width = dialog_hint_width(theme, frame_area, LOCATION_DIALOG_WIDTH);
    hint_height(&LOCATION_DIALOG_QUERY_HINTS, width)
        .max(hint_height(&LOCATION_DIALOG_QUERY_RESOLVED_HINTS, width))
        .max(hint_height(&LOCATION_DIALOG_NAME_HINTS, width))
        .max(hint_height(&LOCATION_DIALOG_LIST_HINTS, width))
}

/// Fixed rows above the list, mirroring the feelings dialog's framing: a title,
/// a separator, the two inputs, a blank spacer, the status line, a separator, and
/// the list heading.
const LOCATION_DIALOG_CHROME: u16 = 8;
/// A blank row between the list and the hint block, matching the feelings dialog.
const LOCATION_DIALOG_HINTS_SPACER: u16 = 1;

pub(crate) fn location_dialog_area(theme: &Theme, frame_area: Rect, list_rows: usize) -> Rect {
    let hint_height = location_dialog_hint_height(theme, frame_area);
    let visible = (list_rows as u16).clamp(1, LOCATION_DIALOG_MAX_VISIBLE_ROWS);
    let h = (dialog_frame_rows(theme)
        + LOCATION_DIALOG_CHROME
        + LOCATION_DIALOG_HINTS_SPACER
        + hint_height
        + visible)
        .min(frame_area.height.saturating_sub(2));
    super::centered_rect_fixed_size(LOCATION_DIALOG_WIDTH, h, frame_area)
}

#[derive(Clone, Copy)]
pub(crate) struct LocationDialogLayout {
    pub(crate) area: Rect,
    pub(crate) title: Rect,
    pub(crate) title_separator: Rect,
    pub(crate) name: Rect,
    pub(crate) query: Rect,
    pub(crate) status: Rect,
    pub(crate) list_separator: Rect,
    pub(crate) heading: Rect,
    pub(crate) list: Rect,
    pub(crate) hints: Rect,
}

pub(crate) fn location_dialog_layout(
    theme: &Theme,
    frame_area: Rect,
    labels: &[String],
) -> LocationDialogLayout {
    let list_rows = location_list_rows(theme, frame_area, labels);
    let area = location_dialog_area(theme, frame_area, list_rows);
    let inner = dialog_content_full(theme, area);
    let hint_height = location_dialog_hint_height(theme, frame_area);
    // Rows: title(0) sep(1) address(2) name(3) spacer(4) status(5) sep(6) heading(7),
    // then the list, a blank spacer, and the hints.
    let list_height = inner
        .height
        .saturating_sub(LOCATION_DIALOG_CHROME + LOCATION_DIALOG_HINTS_SPACER + hint_height);
    let list = Rect {
        x: inner.x,
        y: inner.y + LOCATION_DIALOG_CHROME,
        width: dialog_list_width(theme, inner.width, labels.len(), list_height),
        height: list_height,
    };

    LocationDialogLayout {
        area,
        title: dialog_row(inner, 0),
        title_separator: dialog_row(inner, 1),
        query: dialog_row(inner, 2),
        name: dialog_row(inner, 3),
        // Row 4 is a blank spacer between the inputs and the status line.
        status: dialog_row(inner, 5),
        list_separator: dialog_row(inner, 6),
        heading: dialog_row(inner, 7),
        list,
        hints: dialog_hints_rect(inner, hint_height),
    }
}

#[derive(Clone, Copy)]
pub(crate) struct MetadataDialogLayout {
    pub(crate) area: Rect,
    pub(crate) inner: Rect,
    pub(crate) list_top_separator: Rect,
    pub(crate) list: Rect,
    pub(crate) list_bottom_separator: Rect,
    pub(crate) input: Rect,
    pub(crate) hints: Rect,
}

pub(crate) fn metadata_dialog_layout(
    theme: &Theme,
    frame_area: Rect,
    filtered_len: usize,
) -> MetadataDialogLayout {
    let area = metadata_dialog_area(theme, frame_area, filtered_len);
    let inner = dialog_content_full(theme, area);
    let hint_height = tag_dialog_hint_height(theme, frame_area);
    let list_height = inner
        .height
        .saturating_sub(METADATA_DIALOG_CHROME + hint_height);
    let list = Rect {
        x: inner.x,
        y: inner.y + LIST_DIALOG_LIST_TOP,
        width: dialog_list_width(theme, inner.width, filtered_len, list_height),
        height: list_height,
    };
    let below_list = LIST_DIALOG_LIST_TOP + list_height;

    MetadataDialogLayout {
        area,
        inner,
        list_top_separator: dialog_row(inner, 1),
        list,
        list_bottom_separator: dialog_row(inner, below_list),
        input: dialog_row(inner, below_list + 1),
        hints: dialog_hints_rect(inner, hint_height),
    }
}

#[derive(Clone, Copy)]
pub(crate) struct FeelingsDialogLayout {
    pub(crate) area: Rect,
    pub(crate) inner: Rect,
    pub(crate) list_top_separator: Rect,
    pub(crate) list: Rect,
    pub(crate) list_bottom_separator: Rect,
    pub(crate) input: Rect,
    pub(crate) selected: Rect,
    pub(crate) hints: Rect,
}

pub(crate) fn feelings_dialog_layout(
    theme: &Theme,
    frame_area: Rect,
    all_len: usize,
    selected: &[String],
) -> FeelingsDialogLayout {
    let selected_lines = feelings_selected_line_count(theme, frame_area, selected);
    let area = feelings_dialog_area(theme, frame_area, all_len, selected_lines);
    let inner = dialog_content_full(theme, area);
    let hint_height = feelings_dialog_hint_height(theme, frame_area);
    let selected_h = selected_lines as u16;
    let chrome = feelings_dialog_chrome_height(theme, frame_area, selected_lines);
    let list_height = inner.height.saturating_sub(chrome);
    let list = Rect {
        x: inner.x,
        y: inner.y + LIST_DIALOG_LIST_TOP,
        width: dialog_list_width(theme, inner.width, all_len, list_height),
        height: list_height,
    };
    let below_list = LIST_DIALOG_LIST_TOP + list_height;
    let input_offset = below_list + 1;
    // A blank spacer line sits between the search input and the summary, and
    // another between the summary and the hints.
    let selected = Rect {
        height: selected_h,
        ..dialog_row(inner, input_offset + 2)
    };

    FeelingsDialogLayout {
        area,
        inner,
        list_top_separator: dialog_row(inner, 1),
        list,
        list_bottom_separator: dialog_row(inner, below_list),
        input: dialog_row(inner, input_offset),
        selected,
        hints: dialog_hints_rect(inner, hint_height),
    }
}

#[derive(Clone, Copy)]
pub(crate) struct MoodDialogLayout {
    pub(crate) area: Rect,
    pub(crate) inner: Rect,
    pub(crate) bar: Rect,
    pub(crate) value: Rect,
    pub(crate) hints: Rect,
}

pub(crate) fn mood_dialog_layout(theme: &Theme, frame_area: Rect) -> MoodDialogLayout {
    let area = mood_dialog_area(theme, frame_area);
    // No list, so no scrollbar gutter to hold back — the rows run the full width
    // the hints are sized against.
    let inner = dialog_content_full(theme, area);
    let hint_height = mood_dialog_hint_height(theme, frame_area);
    let right_w = " Blissful".len() as u16;
    let bar_row = dialog_row(inner, 1);
    let bar_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(10),
            Constraint::Min(3),
            Constraint::Length(right_w),
        ])
        .split(bar_row);
    let value_row = dialog_row(inner, 3);
    let value_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(10),
            Constraint::Min(3),
            Constraint::Length(right_w),
        ])
        .split(value_row);

    MoodDialogLayout {
        area,
        inner,
        bar: bar_chunks[1],
        value: value_chunks[1],
        hints: dialog_hints_rect(inner, hint_height),
    }
}

// ── Shared render helpers ─────────────────────────────────────────────────────

/// Render a single-line search/filter input styled as a form field: a normal
/// label followed by an underlined textarea spanning the rest of the row, so it
/// reads as an editable field whether or not text has been entered. The active
/// field is marked by the `>` prefix and the native bar cursor at the caret.
/// (No whole-field reversal: a reversed text selection would vanish inside it.)
fn render_search_field(
    theme: &Theme,
    frame: &mut Frame<'_>,
    rect: Rect,
    label: &str,
    value: &mut TextInput,
    focused: bool,
    hover: HoverTarget,
) {
    let field = input_field_rect(rect, label);
    let hovered = hovered_field(hover, field);
    // Flat chrome marks the active field with an accent stripe, bordered with a
    // `>` caret; both are one column wide so the field math is shared.
    let (marker, marker_style) = if focused {
        if flat_chrome(theme) {
            (theme.glyphs().focus_stripe, theme.primary())
        } else {
            ('>', Style::default())
        }
    } else {
        (' ', Style::default())
    };
    let prefix_w = unicode_width::UnicodeWidthChar::width(marker).unwrap_or(1) as u16
        + UnicodeWidthStr::width(label) as u16;
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(marker.to_string(), marker_style),
            Span::raw(label.to_string()),
        ])),
        rect,
    );

    debug_assert_eq!(field.x, rect.x + prefix_w);
    value.render_in(theme, frame, field, focused, hovered);
}

pub(crate) fn input_field_rect(rect: Rect, label: &str) -> Rect {
    let prefix_w = 1 + UnicodeWidthStr::width(label) as u16;
    Rect {
        x: rect.x + prefix_w,
        width: rect.width.saturating_sub(prefix_w),
        ..rect
    }
}

fn hovered_field(hover: HoverTarget, field: Rect) -> bool {
    matches!(hover, HoverTarget::TextField(rect) if rect == field)
}

fn render_lines_in_area<'a>(
    frame: &mut Frame<'_>,
    lines: impl IntoIterator<Item = Line<'a>>,
    inner: Rect,
) {
    for (y_offset, line) in lines.into_iter().enumerate() {
        let y = inner.y + y_offset as u16;
        if y >= inner.y + inner.height {
            break;
        }
        frame.render_widget(
            Paragraph::new(line),
            Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            },
        );
    }
}

/// The dialog list/menu row a hover targets, if any.
fn hovered_dialog_row(hover: HoverTarget) -> Option<usize> {
    match hover {
        HoverTarget::DialogRow(index) => Some(index),
        _ => None,
    }
}

// ── Dialog draw functions ─────────────────────────────────────────────────────

/// The "Fetching weather and air quality…" modal shown while a save waits on its
/// background context fetch. The ellipsis cycles `.`→`..`→`...` every ~400ms;
/// dropped dots become spaces so the fixed-width box doesn't jitter.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_list_row_at_maps_wrapped_rows() {
        let long = "Some very long place name that keeps going ".repeat(3);
        let labels = vec!["First".to_string(), long.clone(), "Third".to_string()];
        let list_width = LOCATION_DIALOG_WIDTH - 4;
        let l0 = location_row_lines(&labels[0], list_width).len();
        let l1 = location_row_lines(&labels[1], list_width).len();
        assert!(l1 > 1, "long label should wrap onto multiple lines");

        let list = Rect {
            x: 0,
            y: 10,
            width: list_width,
            height: 40,
        };

        // First label's opening line.
        assert_eq!(location_list_row_at(list, &labels, 0, 10), Some(0));
        // Any continuation line of the wrapped label still maps to it.
        let last_of_second = 10 + (l0 + l1 - 1) as u16;
        assert_eq!(
            location_list_row_at(list, &labels, 0, last_of_second),
            Some(1)
        );
        // The third label starts right after the wrapped one.
        let third_start = 10 + (l0 + l1) as u16;
        assert_eq!(location_list_row_at(list, &labels, 0, third_start), Some(2));
        // A click past the last rendered row misses.
        let past = third_start + location_row_lines(&labels[2], list_width).len() as u16;
        assert_eq!(location_list_row_at(list, &labels, 0, past), None);
        // Scrolled: the first visible row is the label at `offset`.
        assert_eq!(location_list_row_at(list, &labels, 1, 10), Some(1));
    }

    /// Both chrome styles. A probe that measures at a different width than the
    /// draw is invisible in bordered chrome, which insets the two rects
    /// identically; only flat holds them apart.
    fn chrome_variants() -> [Theme; 2] {
        use crate::tui::theme::{ChromeStyle, test_flat_theme};
        [
            Theme::terminal_default(),
            test_flat_theme().with_chrome_override(Some(ChromeStyle::Flat)),
        ]
    }

    #[test]
    fn narrow_location_layout_sizes_and_hit_tests_from_its_actual_width() {
        let frame_area = Rect::new(0, 0, 30, 24);
        let labels = vec![
            "A long place name that wraps on a narrow terminal".to_string(),
            "Another place name that also wraps across rows".to_string(),
        ];
        // Bordered chrome has the room to show every wrapped row at once.
        let bordered = location_dialog_layout(&Theme::terminal_default(), frame_area, &labels);
        assert_eq!(
            bordered.list.height as usize,
            location_list_rows(&Theme::terminal_default(), frame_area, &labels)
        );

        for theme in chrome_variants() {
            let layout = location_dialog_layout(&theme, frame_area, &labels);
            let first_height = location_row_lines(&labels[0], layout.list.width).len();
            let total_rows: usize = labels
                .iter()
                .map(|label| location_row_lines(label, layout.list.width).len())
                .sum();

            // The probe the dialog is sized from never comes in under what the
            // list wraps onto at the width it is finally drawn at. When the
            // terminal is too short for all of them the list scrolls; what it
            // must never do is size itself as if the rows didn't wrap.
            assert!(
                location_list_rows(&theme, frame_area, &labels) >= total_rows,
                "the probe undercounts by {} rows",
                total_rows - location_list_rows(&theme, frame_area, &labels)
            );
            assert_eq!(
                location_list_row_at(
                    layout.list,
                    &labels,
                    0,
                    layout.list.y + first_height as u16 - 1,
                ),
                Some(0),
            );
            assert_eq!(
                location_list_row_at(layout.list, &labels, 0, layout.list.y + first_height as u16,),
                Some(1),
            );
        }
    }

    #[test]
    fn feelings_summary_height_uses_the_final_dialog_width() {
        let frame_area = Rect::new(0, 0, 30, 24);
        let selected = vec![
            "overwhelmed".to_string(),
            "appreciative".to_string(),
            "self-conscious".to_string(),
        ];
        for theme in chrome_variants() {
            let layout = feelings_dialog_layout(&theme, frame_area, 4, &selected);
            let rows = feelings_selected_rows(&selected, layout.selected.width);

            assert_eq!(layout.selected.height as usize, rows.len());
            assert!(rows.len() > 1);
        }
    }
}
