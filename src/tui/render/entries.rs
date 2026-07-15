use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Clear, HighlightSpacing, List},
};

use crate::tui::{
    app::{AppModel, Focus, Mode},
    entry_rows::visible_box_items,
    render::{
        EntryListGeometry, clamp_scroll, count_label, list_state_for_render, panel_block,
        render_centered_notice, render_scrollbar_if_needed,
    },
    theme::Theme,
};

pub(crate) fn draw_entry_list(
    active_theme: &Theme,
    frame: &mut Frame<'_>,
    geometry: EntryListGeometry,
    app: &mut AppModel,
) -> usize {
    let focused = app.nav.focus == Focus::Entries;
    let mut block = panel_block(
        active_theme,
        match app.nav.mode {
            Mode::Search => "Search",
            Mode::Browse => "Entries",
        },
        focused,
        Some(count_label(
            app.current_entry_list_len(),
            "entry",
            "entries",
        )),
    );
    let text_width = geometry.text_width;
    let cache = app.entry_rows(text_width);
    let viewport_height = geometry.viewport_height;
    let total_height = cache.total_height;
    let pixel_offset = clamp_scroll(app.nav.entry_list.offset(), total_height, viewport_height);

    // iOS-style sticky section header: once a month's divider scrolls above the
    // viewport, pin that month's label to the panel's top-right border so the
    // current month stays visible while browsing.
    if let Some(month) = sticky_month_label(
        &cache.month_sections,
        app.nav.mode == Mode::Browse,
        pixel_offset,
    ) {
        block = block.title(Line::from(format!(" {month} ")).right_aligned());
    }

    let highlight_active = app.entries_highlighted();
    let (items, selected_visible, item_indices) = visible_box_items(
        &cache.rows,
        pixel_offset,
        viewport_height,
        app.nav.selected_entry_index,
        highlight_active,
    );

    // Style the entry cards: in flat chrome every card sits on the element
    // surface (like the journal cards — spacer and divider rows stay on the
    // panel, keeping the blocks distinct), the hovered card lifts to the
    // hover surface, and the selected card keeps its List highlight, which
    // patches over the item style. Bordered chrome keeps plain boxes with
    // only the hover lift.
    let hovered = match app.hover {
        crate::tui::state::HoverTarget::Entry(index) => Some(index),
        _ => None,
    };
    let selected = app.nav.selected_entry_index.filter(|_| highlight_active);
    let flat = super::flat_chrome(active_theme);
    let items: Vec<_> = if flat || (hovered.is_some() && hovered != selected) {
        items
            .into_iter()
            .zip(&item_indices)
            .map(|(item, index)| {
                if index.is_some() && *index == hovered && *index != selected {
                    item.style(active_theme.hover())
                } else if flat && index.is_some() {
                    item.style(Style::default().bg(active_theme.raised_bg()))
                } else {
                    item
                }
            })
            .collect()
    } else {
        items
    };

    let list = List::new(items)
        .highlight_style(active_theme.selection())
        .highlight_spacing(HighlightSpacing::Never);

    let mut render_state =
        list_state_for_render(selected_visible, 0, viewport_height, highlight_active);

    frame.render_widget(block, geometry.panel.area);
    super::panel_focus_stripe(active_theme, frame, geometry.panel.area, focused);
    // In search mode, the query renders as a fixed-width field on the panel's
    // top-right border — sized from the panel, not the typed text, so it
    // doesn't grow and shrink while typing.
    if app.nav.mode == Mode::Search {
        draw_search_field(active_theme, frame, geometry.panel.area, app);
    }
    frame.render_stateful_widget(list, geometry.panel.content, &mut render_state);
    render_scrollbar_if_needed(
        active_theme,
        frame,
        geometry.panel.area,
        total_height,
        viewport_height,
        pixel_offset,
        focused,
    );

    // An empty column gets a centered notice so it doesn't read as a rendering
    // glitch: a blank or unmatched search query, no journal selected to browse,
    // or a selected journal with no entries.
    if cache.rows.is_empty() {
        let message = match app.nav.mode {
            Mode::Search => "No results",
            Mode::Browse if app.selected_journal().is_none() => "No journal selected",
            Mode::Browse => "No entries",
        };
        render_centered_notice(active_theme, frame, geometry.panel.content, message);
    }
    pixel_offset
}

/// The search field on the panel's top-right border: a fixed-width single-line
/// textarea (with the native bar cursor while typing in it), padded one cell on
/// each side so it doesn't run into the border line.
fn draw_search_field(active_theme: &Theme, frame: &mut Frame<'_>, area: Rect, app: &mut AppModel) {
    let Some(rect) = search_field_rect(area) else {
        return;
    };
    let field_w = rect.width;
    let pad = Rect {
        x: rect.x - 1,
        width: field_w + 2,
        ..rect
    };
    frame.render_widget(Clear, pad);
    frame
        .buffer_mut()
        .set_style(pad, Style::default().bg(active_theme.base_bg()));
    let focused = app.is_search_input_active() && !app.has_overlay() && app.editor.is_none();
    let hovered = matches!(
        app.hover,
        crate::tui::state::HoverTarget::TextField(r) if r == rect
    );
    app.search
        .query
        .render_in(active_theme, frame, rect, focused, hovered);
}

pub(super) fn search_field_rect(area: Rect) -> Option<Rect> {
    let field_w = (area.width / 2)
        .clamp(12, 30)
        .min(area.width.saturating_sub(6));
    if field_w < 4 || area.height == 0 {
        return None;
    }
    Some(Rect {
        x: area.x + area.width - field_w - 2,
        y: area.y,
        width: field_w,
        height: 1,
    })
}

/// The month label to pin on the panel border. The first month rides the border
/// from the start (its divider is replaced by a leading blank line); each later
/// month takes over only once its `Month Year` divider has scrolled strictly
/// above the viewport, so the in-list divider and the border label are never
/// shown at once. `None` outside browse mode or when there are no entries.
fn sticky_month_label(
    sections: &[(usize, String)],
    is_browse: bool,
    offset: usize,
) -> Option<String> {
    if !is_browse {
        return None;
    }

    // The latest month whose divider has scrolled above the top, falling back to
    // the first month (which owns the border before anything scrolls past).
    sections
        .iter()
        .rev()
        .find(|(start, _)| *start < offset)
        .or_else(|| sections.first())
        .map(|(_, label)| label.clone())
}
