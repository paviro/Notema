//! The filter dialog: a tabbed browser over the facets
//! ([`FilterTab`]), each a scrollable list of `label ···· count` rows. Selecting a
//! row launches its search. Layout and hit-testing derive from the same segment
//! math so drawing and clicking never drift.

use ratatui::{
    Frame,
    layout::Rect,
    text::Span,
    widgets::{List, ListItem, Paragraph},
};

use crate::tui::app::SearchScope;
use crate::tui::features::filter::FilterState;
use crate::tui::render::tab_strip::{StripTab, full_strip_width, tab_strip_line};
use crate::tui::state::{FilterTab, HoverTarget, ListNav};
use crate::tui::theme::Theme;

use super::super::chrome::{
    centered_rect_fixed_size, dot_leader_line, render_centered_notice, render_dialog_list_scrollbar,
};
use super::super::footer::{Hint, HintId, hint_height};
use super::super::frames::{
    dialog_content_full, dialog_frame_rows, dialog_hints_rect, dialog_list_width, dialog_row,
    draw_dialog_frame_wide,
};
use super::super::list_state_for_render;

const FILTER_DIALOG_MAX_VISIBLE_ROWS: u16 = 14;
/// Rows above the list inside the border: the tab strip and its separator.
const FILTER_DIALOG_CHROME: u16 = 2;
/// A blank row between the list and the hint block.
const FILTER_DIALOG_HINTS_SPACER: u16 = 1;

const FILTER_DIALOG_HINTS: [Hint; 3] = [
    Hint::new("switch tab", "tab ←→", HintId::FilterNextTab),
    Hint::new("show", "enter", HintId::FilterLaunch),
    Hint::new("cancel", "esc", HintId::CancelOverlay),
];

pub(crate) fn filter_dialog_hints() -> &'static [Hint] {
    &FILTER_DIALOG_HINTS
}

// ── Tab strip (the shared strip on an inner content row) ──────────────────────

impl StripTab for FilterTab {
    fn all() -> &'static [Self] {
        &Self::ALL
    }
    fn title(self) -> &'static str {
        self.title()
    }
    fn short_title(self) -> &'static str {
        self.short_title()
    }
    fn initial(self) -> &'static str {
        self.initial()
    }
}

// ── Layout ────────────────────────────────────────────────────────────────────

/// The dialog's outer width: the tab strip at full labels (its widest row), so the
/// tabs always fit without collapsing. `dialog_content_full` insets the outer width
/// by four columns, so strip width + 4 is the exact fit.
fn filter_dialog_width() -> u16 {
    full_strip_width::<FilterTab>() + 4
}

fn filter_dialog_hint_height(theme: &Theme, frame_area: Rect) -> u16 {
    let width = super::dialog_hint_width(theme, frame_area, filter_dialog_width());
    hint_height(&FILTER_DIALOG_HINTS, width)
}

fn filter_dialog_area(theme: &Theme, frame_area: Rect, rows: usize) -> Rect {
    let hint_height = filter_dialog_hint_height(theme, frame_area);
    let visible = (rows as u16).clamp(1, FILTER_DIALOG_MAX_VISIBLE_ROWS);
    let h = (dialog_frame_rows(theme)
        + FILTER_DIALOG_CHROME
        + FILTER_DIALOG_HINTS_SPACER
        + hint_height
        + visible)
        .min(frame_area.height.saturating_sub(2));
    centered_rect_fixed_size(filter_dialog_width(), h, frame_area)
}

#[derive(Clone, Copy)]
pub(crate) struct FilterDialogLayout {
    pub(crate) area: Rect,
    pub(crate) tabs: Rect,
    pub(crate) separator: Rect,
    pub(crate) list: Rect,
    pub(crate) hints: Rect,
}

pub(crate) fn filter_dialog_layout(
    theme: &Theme,
    frame_area: Rect,
    state: &FilterState,
) -> FilterDialogLayout {
    let area = filter_dialog_area(theme, frame_area, state.current_rows().len());
    // Rows beside no scrollbar (tabs, separator, hints) run flush with the bar's
    // right edge; the list narrows to leave the bar room only when it overflows.
    let inner = dialog_content_full(theme, area);
    let hint_height = filter_dialog_hint_height(theme, frame_area);
    let list_height = inner
        .height
        .saturating_sub(FILTER_DIALOG_CHROME + FILTER_DIALOG_HINTS_SPACER + hint_height);
    let list = Rect {
        x: inner.x,
        y: inner.y + FILTER_DIALOG_CHROME,
        width: dialog_list_width(theme, inner.width, state.current_rows().len(), list_height),
        height: list_height,
    };
    FilterDialogLayout {
        area,
        tabs: dialog_row(inner, 0),
        separator: dialog_row(inner, 1),
        list,
        hints: dialog_hints_rect(inner, hint_height),
    }
}

// ── Draw ──────────────────────────────────────────────────────────────────────

pub(in crate::tui::render) fn draw_filter_dialog(
    theme: &Theme,
    frame: &mut Frame<'_>,
    state: &mut FilterState,
    hover: HoverTarget,
) {
    let layout = filter_dialog_layout(theme, frame.area(), state);
    let scope_label = match &state.scope {
        SearchScope::AllJournals => "All journals".to_string(),
        SearchScope::Journal(name) => name.clone(),
    };

    state.normalize_list_state();
    let rows_len = state.current_rows().len();
    let max_visible = layout.list.height;
    let max_offset = rows_len.saturating_sub(max_visible as usize);
    let scroll = state.offset().min(max_offset);
    state.list.set_offset(scroll);

    draw_dialog_frame_wide(
        theme,
        frame,
        layout.area,
        &format!("Filter — {scope_label}"),
        true,
    );

    let hovered_tab = match hover {
        HoverTarget::FilterTab(tab) => Some(tab),
        _ => None,
    };
    frame.render_widget(
        Paragraph::new(tab_strip_line(
            theme,
            state.tab,
            // The filter dialog is modal, so its active tab is always "focused".
            true,
            hovered_tab,
            0,
            layout.tabs.width,
        )),
        layout.tabs,
    );
    super::render_separator(theme, frame, layout.separator);
    super::render_hint_line(theme, frame, &FILTER_DIALOG_HINTS, layout.hints, hover);

    if rows_len == 0 {
        render_centered_notice(theme, frame, layout.list, &empty_notice(state.tab));
        return;
    }

    let hovered_row = match hover {
        HoverTarget::DialogRow(index) => Some(index),
        _ => None,
    };
    let selected = state.selected_index();
    let items: Vec<ListItem<'_>> = state
        .current_rows()
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let item = ListItem::new(dot_leader_line(
                theme,
                Span::raw(row.label.clone()),
                Span::styled(row.count.to_string(), theme.muted()),
                layout.list.width,
                Some(index) == selected,
            ));
            if Some(index) == hovered_row && Some(index) != selected {
                item.style(theme.hover())
            } else {
                item
            }
        })
        .collect();

    let list = List::new(items).highlight_style(theme.selection());
    let mut render_state = list_state_for_render(state.selected_index(), scroll, max_visible, true);
    frame.render_stateful_widget(list, layout.list, &mut render_state);

    render_dialog_list_scrollbar(theme, frame, layout.list, rows_len, scroll, true);
}

/// The empty-state message for a facet with no values in scope.
fn empty_notice(tab: FilterTab) -> String {
    format!("No {} in scope", tab.title().to_lowercase())
}
