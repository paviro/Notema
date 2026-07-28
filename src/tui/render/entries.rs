use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Clear, HighlightSpacing, List, ListItem},
};

use crate::tui::{
    app::{AppModel, Focus, Mode},
    entry_rows::visible_box_items,
    render::{
        EntryListGeometry,
        chrome::{dot_leader_line, render_dialog_list_scrollbar, scrim_scaled},
        clamp_scroll, count_label,
        frames::{
            POPUP_FRAME_COLS, POPUP_FRAME_ROWS, dialog_list_width, draw_popup_frame, popup_inner,
        },
        list_state_for_render, panel_block, render_centered_notice, render_scrollbar_if_needed,
    },
    state::ListNav,
    surface::{panel_inner, surface_content_inner},
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

/// How much of the theme's dialog scrim the suggestion popup lays down. Half:
/// the popup is not modal, and the counts it lists are counts of the very
/// results behind it, so those have to stay readable while it floats.
const POPUP_SCRIM: f32 = 0.5;

/// The search field and its suggestion popup, drawn after every panel rather
/// than inside the entries one — the popup dims what it hangs over, and the
/// scrim covers the whole frame. The field is redrawn here so it comes back to
/// full brightness alongside the popup it belongs to.
pub(super) fn draw_search_overlay(
    active_theme: &Theme,
    frame: &mut Frame<'_>,
    entries_area: Option<Rect>,
    app: &mut AppModel,
) {
    let Some(area) = entries_area else {
        return;
    };
    // Nothing dims for a popup that won't be drawn — an unshowable one would
    // leave the screen darkened with nothing to show for it.
    if !app.suggestions_visible()
        || search_suggestions_rect(active_theme, area, app.search.suggestions.rows.len()).is_none()
    {
        return;
    }
    let scrim_area = frame.area();
    scrim_scaled(active_theme, frame.buffer_mut(), scrim_area, POPUP_SCRIM);
    draw_search_field(active_theme, frame, area, app);
    draw_search_suggestions(active_theme, frame, area, app);
}

/// The search field on the panel's top-right border: a fixed-width single-line
/// textarea (with the native bar cursor while typing in it), padded one cell on
/// each side so it doesn't run into the border line.
fn draw_search_field(active_theme: &Theme, frame: &mut Frame<'_>, area: Rect, app: &mut AppModel) {
    let Some(rect) = search_field_rect(active_theme, area) else {
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
    // Restyled from the text on every frame rather than cached against an edit:
    // `TextInput::set_text` drops syntax spans along with everything else set
    // through the deref, so anything stored here would have to be reapplied by
    // every caller that replaces the query. A query is a line long — recomputing
    // it costs less than the bookkeeping would.
    let styling = super::search_query::query_styling(active_theme, app.search.query.as_str());
    app.search.query.set_syntax_spans(vec![styling.spans]);
    app.search
        .query
        .set_glyph_substitutions(vec![styling.substitutions]);
    app.search
        .query
        .render_in(active_theme, frame, rect, focused, hovered);
}

/// The most suggestion rows on screen at once. Past this the list scrolls: it
/// hangs over the results, and one that covered them would trade the narrowing it
/// is there to help with.
const SUGGESTION_MAX_ROWS: u16 = 8;

/// The suggestion popup's outer rect, frame included, hanging under the search
/// field and down over the entry list. `None` when there is nothing to show, or
/// no room for the frame and at least one option.
///
/// Paired with [`search_suggestions_list_rect`], which carves the option rows
/// out of it. Both the draw and the hit-test registration take their geometry
/// from these two, so the click map cannot drift from the pixels.
pub(super) fn search_suggestions_rect(theme: &Theme, area: Rect, rows: usize) -> Option<Rect> {
    let field = search_field_rect(theme, area)?;
    let content = surface_content_inner(theme, panel_inner(area));
    if rows == 0 {
        return None;
    }
    // Squared off against the field itself, both edges, so the popup reads as
    // that field's own drawer. Not against the box `draw_search_field` paints
    // around it: those padding cells carry the base surface, near enough to
    // invisible that lining up with them reads as being a column out.
    let x = field.x.max(content.x);
    let width = (field.x + field.width).saturating_sub(x);
    let below = (area.y + area.height).saturating_sub(field.y + 1);
    let height = (rows as u16)
        .min(SUGGESTION_MAX_ROWS)
        .saturating_add(POPUP_FRAME_ROWS)
        .min(below);
    // The frame alone is not a popup: it takes an option row to be worth drawing.
    if height <= POPUP_FRAME_ROWS || width <= 2 * POPUP_FRAME_COLS {
        return None;
    }
    Some(Rect {
        x,
        y: field.y + 1,
        width,
        height,
    })
}

/// The option rows inside the popup: its content rect, held back by the
/// scrollbar gutter only when the list overflows — so the bar lands inside the
/// popup, on the frame's border, instead of on the entry panel behind it.
pub(crate) fn search_suggestions_list_rect(theme: &Theme, area: Rect, rows: usize) -> Option<Rect> {
    let inner = popup_inner(search_suggestions_rect(theme, area, rows)?);
    Some(Rect {
        width: dialog_list_width(theme, inner.width, rows, inner.height),
        ..inner
    })
}

/// The values on offer for the filter value being typed, listed under the query
/// field with the number of entries each would return.
fn draw_search_suggestions(
    active_theme: &Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    app: &mut AppModel,
) {
    if !app.suggestions_visible() {
        return;
    }
    let rows = app.search.suggestions.rows.len();
    let (Some(outer), Some(list_rect)) = (
        search_suggestions_rect(active_theme, area, rows),
        search_suggestions_list_rect(active_theme, area, rows),
    ) else {
        return;
    };

    let hovered = super::dialogs::hovered_dialog_row(app.hover);
    let selected = app.search.suggestions.selected_index();
    // Clamped here and nowhere else: the viewport is only known at draw time, so an
    // offset the wheel or a thumb drag left past the end self-corrects on this frame.
    let scroll = app
        .search
        .suggestions
        .offset()
        .min(rows.saturating_sub(list_rect.height as usize));
    app.search.suggestions.list.set_offset(scroll);

    let items: Vec<ListItem<'_>> = app
        .search
        .suggestions
        .rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let item = ListItem::new(dot_leader_line(
                active_theme,
                Span::raw(row.label.clone()),
                Span::styled(row.count.to_string(), active_theme.muted()),
                list_rect.width,
                Some(index) == selected,
            ));
            if Some(index) == hovered && Some(index) != selected {
                item.style(active_theme.hover())
            } else {
                item
            }
        })
        .collect();

    draw_popup_frame(active_theme, frame, outer);
    let list = List::new(items).highlight_style(active_theme.selection());
    let mut state = list_state_for_render(selected, scroll, list_rect.height, true);
    frame.render_stateful_widget(list, list_rect, &mut state);
    render_dialog_list_scrollbar(active_theme, frame, list_rect, rows, scroll, true);
}

pub(super) fn search_field_rect(theme: &Theme, area: Rect) -> Option<Rect> {
    // Right-align to the entry-box column (flat chrome insets further than
    // bordered), less one cell for the trailing accent pad in `draw_search_field`.
    let content = surface_content_inner(theme, panel_inner(area));
    let right_edge = (content.x + content.width).saturating_sub(1);
    let field_w = (content.width * 2 / 3).clamp(16, 40).min(content.width);
    if field_w < 4 || area.height == 0 {
        return None;
    }
    Some(Rect {
        x: right_edge - field_w,
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
