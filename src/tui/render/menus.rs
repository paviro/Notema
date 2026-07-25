//! The metadata chooser popup and the section-table cheatsheets (global help and
//! the editor's shortcut reference).

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use super::table;
use crate::tui::surface::surface_outer_width;
use crate::tui::theme::Theme;

use super::chrome::{
    centered_rect_fixed_size, clear_surface, flat_chrome, render_scrollbar_if_needed,
};
use super::footer::{Hint, HintId, hint_lines, key_chip_style, key_chip_text};
use super::frames::{
    dialog_content_full, dialog_frame_rows, dialog_inner, draw_dialog_frame, draw_dialog_frame_wide,
};

mod overlays;
pub(crate) use overlays::{draw_metadata_menu, metadata_menu_hints, metadata_menu_interactions};

const EDITOR_SHORTCUT_SECTIONS: [(&str, &[(&str, &str)]); 4] = [
    (
        "File",
        &[
            ("ctrl/⌘+s", "Save"),
            ("ctrl+o", "Fullscreen"),
            ("ctrl+g", "Metadata"),
            ("esc", "Discard"),
        ],
    ),
    (
        "Edit",
        &[
            ("ctrl+a", "Select all"),
            ("ctrl/⌘+z", "Undo"),
            ("ctrl+y · ⌘⇧z", "Redo"),
            ("ctrl/⌘+x", "Cut → clipboard"),
            ("ctrl/⌘+c", "Copy → clipboard"),
            ("ctrl/⌘+v", "Paste"),
            ("ctrl+k", "Cut to line end"),
            ("ctrl+w", "Delete word"),
        ],
    ),
    (
        "Move",
        &[
            ("arrows", "Move"),
            ("shift+move", "Select"),
            ("ctrl/⌥+←/→", "Word"),
            ("home/end", "Line start/end"),
            ("ctrl+↑/↓", "Paragraph"),
            ("pgup/pgdn", "Page"),
        ],
    ),
    // The textarea also honors these emacs bindings; the app leaves them alone
    // (unlike ctrl+a, which it takes for select-all).
    (
        "Emacs",
        &[
            ("ctrl+b/f", "Char left / right"),
            ("ctrl+p/n", "Line up / down"),
            ("ctrl+e", "Line end"),
            ("ctrl+h/d", "Delete back / forward"),
            ("ctrl+j", "Delete to line start"),
        ],
    ),
];

/// Draw the internal editor's shortcut reference: the same centered, multi-column
/// table as the global help overlay. Opened with Ctrl+T, scrolled with the
/// arrows/page keys, dismissed by any other key or a click.
pub(crate) fn draw_editor_shortcuts(theme: &Theme, frame: &mut Frame<'_>, scroll: &mut u16) {
    draw_section_table(
        theme,
        frame,
        &EDITOR_SHORTCUT_SECTIONS,
        "Editor Shortcuts",
        "press any key to close",
        scroll,
    );
}

/// A menu popup's clickable regions, shared by the metadata chooser's draw and
/// its hit-test so the click map can't drift from the pixels.
pub(crate) struct MenuInteractions {
    pub(crate) rows: Vec<(Rect, usize)>,
    pub(crate) footer: Rect,
}

/// The global keyboard-shortcut cheatsheet, grouped by the panel/context each key
/// applies to. Opened with `?` from browse or a search result.
const HELP_SECTIONS: [(&str, &[(&str, &str)]); 6] = [
    (
        "Move",
        &[
            ("↑ ↓", "Move / scroll"),
            ("← →", "Panels"),
            ("enter", "View / expand"),
            ("esc", "Back"),
        ],
    ),
    (
        "Journals",
        &[("n", "New journal"), ("a", "Archive"), ("d", "Delete")],
    ),
    (
        "Entry",
        &[
            ("e", "Edit"),
            ("n", "New entry"),
            ("d", "Delete"),
            ("s", "Star"),
            ("i", "Images"),
        ],
    ),
    (
        "Metadata",
        &[
            ("t", "Tags"),
            ("p", "People"),
            ("a", "Activities"),
            ("f", "Feelings"),
            ("m", "Mood"),
            ("l", "Location"),
            ("ctrl+g", "Metadata menu"),
        ],
    ),
    ("Insights", &[("g", "Scope"), ("w", "Timeframe")]),
    (
        "General",
        &[
            ("/", "Search"),
            ("b", "Filter"),
            ("j", "Journals"),
            (",", "Settings"),
            ("h", "Toggle hints"),
            ("r", "Refresh"),
            ("?", "This help"),
            ("q", "Quit"),
        ],
    ),
];

/// Cap on the cheatsheet's columns: three reads as a balanced grid without the
/// key/action pairs drifting too far apart.
const HELP_MAX_COLS: usize = 3;

/// The `│`-with-a-space-each-side rule drawn between two columns.
const HELP_RULE_PAD: u16 = 3;

/// One section rendered as a block of lines: a bold group heading, a full-`width`
/// faint rule under it, then its bindings — each an aligned key chip followed by
/// the action. The rows themselves are left ragged; `section_table_lines` pads
/// every cell to the column width when it splices the columns together.
fn section_block(
    theme: &Theme,
    group: &str,
    items: &[(&str, &str)],
    width: usize,
) -> Vec<Line<'static>> {
    let chip_w = max_chip_width(items);
    let set = theme.glyphs().borders.line_set();
    let mut lines = Vec::with_capacity(items.len() + 2);
    lines.push(Line::from(Span::styled(group.to_string(), theme.heading())));
    lines.push(Line::from(Span::styled(
        set.horizontal.repeat(width),
        table::themed_faint_rule_style(theme),
    )));
    for (keys, action) in items {
        let chip = key_chip_text(keys);
        let gap = chip_w.saturating_sub(UnicodeWidthStr::width(chip.as_str())) + 2;
        lines.push(Line::from(vec![
            Span::styled(chip, key_chip_style(theme)),
            Span::raw(" ".repeat(gap)),
            Span::styled((*action).to_string(), theme.text()),
        ]));
    }
    lines
}

/// The widest key chip in a section — every binding's action starts two columns
/// past it, so the chips and actions line up. Shared by `section_block` (layout)
/// and `section_width` (sizing) so the two cannot drift.
fn max_chip_width(items: &[(&str, &str)]) -> usize {
    items
        .iter()
        .map(|(keys, _)| UnicodeWidthStr::width(key_chip_text(keys).as_str()))
        .max()
        .unwrap_or(0)
}

/// The widest line a section block would render at, used to size every column
/// to a common width before the blocks are built.
fn section_width(items: &[(&str, &str)]) -> usize {
    let chip_w = max_chip_width(items);
    items
        .iter()
        .map(|(_, action)| chip_w + 2 + UnicodeWidthStr::width(*action))
        .max()
        .unwrap_or(0)
}

/// Split the sections into `ncols` contiguous, non-empty columns, choosing the
/// cut that keeps the tallest column as short as possible so the grid reads as
/// balanced (and no column is ever left empty). A blank row separates sections
/// stacked in the same column.
fn section_columns(blocks: &[Vec<Line<'static>>], ncols: usize) -> Vec<Vec<Line<'static>>> {
    let ncols = ncols.clamp(1, blocks.len().max(1));
    let sizes: Vec<usize> = blocks.iter().map(Vec::len).collect();
    let mut start = 0;
    balanced_splits(&sizes, ncols)
        .into_iter()
        .map(|end| {
            let mut column: Vec<Line<'static>> = Vec::new();
            for block in &blocks[start..end] {
                if !column.is_empty() {
                    column.push(Line::default());
                }
                column.extend(block.iter().cloned());
            }
            start = end;
            column
        })
        .collect()
}

/// The tallest column produced by a set of end-boundaries, counting a blank row
/// between sections stacked in the same column.
pub(super) fn column_span(sizes: &[usize], bounds: &[usize]) -> usize {
    let mut start = 0;
    let mut tallest = 0;
    for &end in bounds {
        let height = sizes[start..end].iter().sum::<usize>() + (end - start).saturating_sub(1);
        tallest = tallest.max(height);
        start = end;
    }
    tallest
}

/// Recurse over every way to place the remaining column boundaries after
/// `start`, keeping the split whose tallest column is smallest. Each column
/// must take at least one section, so a boundary always leaves room for the
/// columns still to come.
fn search_splits(
    sizes: &[usize],
    start: usize,
    cols: usize,
    current: &mut Vec<usize>,
    best: &mut Option<(usize, Vec<usize>)>,
) {
    let n = sizes.len();
    if cols == 1 {
        current.push(n);
        let span = column_span(sizes, current);
        if best.as_ref().is_none_or(|(best_span, _)| span < *best_span) {
            *best = Some((span, current.clone()));
        }
        current.pop();
        return;
    }
    for end in (start + 1)..=(n - (cols - 1)) {
        current.push(end);
        search_splits(sizes, end, cols - 1, current, best);
        current.pop();
    }
}

/// End-boundaries of the `ncols` columns (the last is always `sizes.len()`) that
/// minimize the tallest column when the sections are cut into contiguous groups.
pub(super) fn balanced_splits(sizes: &[usize], ncols: usize) -> Vec<usize> {
    let n = sizes.len();
    if n == 0 {
        return vec![0];
    }
    // Clamp so a request for more columns than sections can't underflow the
    // `n - (cols - 1)` range in `search_splits`; production callers already keep
    // `ncols <= n`, so this only guards direct/edge use.
    let ncols = ncols.min(n);
    if ncols <= 1 {
        return vec![n];
    }
    let mut best = None;
    let mut current = Vec::with_capacity(ncols);
    search_splits(sizes, 0, ncols, &mut current, &mut best);
    best.map_or_else(|| vec![n], |(_, bounds)| bounds)
}

/// Splice the columns into one table body, row by row, with a themed vertical
/// rule between each pair. Short columns are padded with blank rows so the rules
/// run straight to the bottom.
fn section_table_lines(
    theme: &Theme,
    columns: &[Vec<Line<'static>>],
    col_w: usize,
) -> Vec<Line<'static>> {
    let rows = columns.iter().map(Vec::len).max().unwrap_or(0);
    let rule = theme.glyphs().borders.line_set().vertical.to_string();
    (0..rows)
        .map(|r| {
            let mut spans = Vec::new();
            for (c, column) in columns.iter().enumerate() {
                if c > 0 {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        rule.to_string(),
                        table::themed_border_style(theme),
                    ));
                    spans.push(Span::raw(" "));
                }
                // Pad every cell — real, short, or missing — to the column
                // width so the rules run dead straight down the table.
                let cell = column
                    .get(r)
                    .map(|line| line.spans.as_slice())
                    .unwrap_or(&[]);
                let used: usize = cell
                    .iter()
                    .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
                    .sum();
                spans.extend(cell.iter().cloned());
                spans.push(Span::raw(" ".repeat(col_w.saturating_sub(used))));
            }
            Line::from(spans)
        })
        .collect()
}

/// Draw the global keyboard cheatsheet: the centered, multi-column reference
/// table opened with `?` from browse or a search result.
pub(crate) fn draw_help(theme: &Theme, frame: &mut Frame<'_>, scroll: &mut u16) {
    draw_section_table(
        theme,
        frame,
        &HELP_SECTIONS,
        "Keyboard Shortcuts",
        "press any key to close",
        scroll,
    );
}

/// Draw a keyboard cheatsheet as a centered table: the `sections` arranged in
/// balanced columns split by vertical rules, sized to their content and
/// centered on screen. `scroll` only engages when a short terminal can't show
/// every row. Shared by the global help overlay and the editor's reference.
fn draw_section_table(
    theme: &Theme,
    frame: &mut Frame<'_>,
    sections: &[(&str, &[(&str, &str)])],
    title: &str,
    footer: &str,
    scroll: &mut u16,
) {
    let frame_area = frame.area();
    let col_w = sections
        .iter()
        .map(|(_, items)| section_width(items))
        .max()
        .unwrap_or(0) as u16;

    // Widen the grid until it either runs out of horizontal room or hits the
    // column cap; more columns means a shorter, squarer table.
    let avail_w = frame_area
        .width
        .saturating_sub(surface_outer_width(theme, 0));
    let fit = ((avail_w + HELP_RULE_PAD) / (col_w + HELP_RULE_PAD)).max(1) as usize;
    let ncols = fit.min(HELP_MAX_COLS).min(sections.len());

    let blocks: Vec<Vec<Line<'static>>> = sections
        .iter()
        .map(|(group, items)| section_block(theme, group, items, col_w as usize))
        .collect();
    let columns = section_columns(&blocks, ncols);
    let ncols = columns.len() as u16;
    let lines = section_table_lines(theme, &columns, col_w as usize);

    let content_w = col_w * ncols + HELP_RULE_PAD * ncols.saturating_sub(1);
    let frame_rows = if flat_chrome(theme) {
        dialog_frame_rows(theme) + 1
    } else {
        dialog_frame_rows(theme)
    };
    let avail_h = frame_area.height.saturating_sub(2).max(3);
    let total = lines.len() as u16;
    // A blank row sits between the table and the footer, so the box is a row
    // taller than its content and the footer breathes off the last line.
    let outer_h = (total + frame_rows + 1).min(avail_h);
    let content_h = outer_h.saturating_sub(frame_rows + 1);
    let border_label = |text: &str| surface_outer_width(theme, UnicodeWidthStr::width(text) as u16);
    let outer_w = surface_outer_width(theme, content_w)
        .max(border_label(title))
        .max(border_label(footer))
        .min(frame_area.width);
    let area = centered_rect_fixed_size(outer_w, outer_h, frame_area);

    let mut content = dialog_inner(theme, area);
    if flat_chrome(theme) {
        content.height = content.height.saturating_sub(1);
    }
    // Center the table within the (possibly wider) content box.
    let content = Rect {
        x: content.x + content.width.saturating_sub(content_w) / 2,
        width: content_w.min(content.width),
        height: content_h,
        ..content
    };
    *scroll = (*scroll).min(total.saturating_sub(content.height));

    if flat_chrome(theme) {
        draw_dialog_frame(theme, frame, area, title, false);
        let bottom = Rect {
            y: area.y + area.height.saturating_sub(2),
            height: 1,
            ..area
        };
        frame.render_widget(
            Paragraph::new(Span::styled(footer.to_string(), theme.muted()))
                .alignment(Alignment::Center),
            bottom,
        );
    } else {
        clear_surface(theme, frame, area, theme.dialog_bg());
        let block = Block::default()
            .title(format!(" {title} "))
            .title_bottom(Line::from(format!(" {footer} ")).centered())
            .borders(Borders::ALL)
            .border_set(theme.glyphs().borders.border_set())
            .border_style(theme.dialog_border());
        frame.render_widget(block, area);
    }

    frame.render_widget(Paragraph::new(lines).scroll((*scroll, 0)), content);
    render_scrollbar_if_needed(
        theme,
        frame,
        area,
        total as usize,
        content.height,
        *scroll as usize,
        true,
    );
}
