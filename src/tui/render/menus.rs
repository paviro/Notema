//! The metadata chooser popup and the section-table cheatsheets (global help and
//! the editor's shortcut reference).

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use super::tab_strip::{StripTab, full_strip_width, tab_strip_line};
use super::table;
use crate::tui::state::{HelpTab, HoverTarget};
use crate::tui::surface::surface_outer_width;
use crate::tui::theme::Theme;

use super::chrome::{centered_rect_fixed_size, render_dialog_list_scrollbar};
use super::footer::{Hint, HintId, hint_height, hint_lines, key_chip_style, key_chip_text};
use super::frames::{
    dialog_content_full, dialog_frame_rows, dialog_hints_rect, dialog_list_width, dialog_row,
    draw_dialog_frame_wide, render_hint_line, render_separator,
};

mod overlays;
pub(crate) use overlays::{draw_metadata_menu, metadata_menu_hints, metadata_menu_interactions};

impl StripTab for HelpTab {
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

/// Draw the internal editor's shortcut reference: the same centered dialog as the
/// global help overlay, minus the tabs. Opened with Ctrl+T, scrolled with the
/// arrows/page keys, dismissed by esc or the close chip.
pub(crate) fn draw_editor_shortcuts(
    theme: &Theme,
    frame: &mut Frame<'_>,
    hover: HoverTarget,
    scroll: &mut u16,
) {
    draw_section_dialog(theme, frame, &editor_shortcuts_spec(), hover, scroll);
}

/// The editor shortcut reference's layout, exposed so the interaction map can
/// register its hint-line region from the rect the draw uses.
pub(crate) fn editor_shortcuts_layout(theme: &Theme, frame_area: Rect) -> SectionDialogLayout {
    section_dialog_layout(theme, frame_area, &editor_shortcuts_spec())
}

pub(crate) fn editor_shortcuts_hints() -> &'static [Hint] {
    &EDITOR_SHORTCUT_HINTS
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
            ("R", "Rebuild cache"),
            ("?", "This help"),
            ("q", "Quit"),
        ],
    ),
];

/// The search-box command reference: the prefixes the search field understands,
/// the date values they accept, and how to chain them. No prefix runs a
/// full-text search instead.
const SEARCH_SECTIONS: [(&str, &[(&str, &str)]); 5] = [
    (
        "Filters",
        &[
            ("tags:", "Tag contains…"),
            ("people:", "Person contains…"),
            ("activities:", "Activity contains…"),
            ("feelings:", "Feeling or alias contains…"),
            ("location:", "Place, all words match"),
            ("mood:", "Exact score -5 to 5"),
            ("star:", "Favorites"),
            ("star:false", "Everything else"),
        ],
    ),
    (
        "Dates",
        &[
            ("date:", "On a day / month / year"),
            ("before:", "Strictly before"),
            ("after:", "Strictly after"),
        ],
    ),
    (
        "Date values",
        &[
            ("2026-07-25", "A day"),
            ("2026-07", "A month"),
            ("2026", "A year"),
            ("today", "Today"),
            ("yesterday", "Yesterday"),
            ("7d 2w 3m 1y", "The day that long ago"),
            ("*-07-25", "Every year, date: only"),
            ("2026-*-25", "Every month, date: only"),
        ],
    ),
    (
        "Combine",
        &[
            ("tags:x; people:y", "Every filter matches"),
            ("tags:x+y", "All values match"),
            ("tags:x|y", "Any value matches"),
            ("tags:x+y|z", "x and (y or z)"),
            ("beach; tags:x", "Text plus filters"),
            ("tags:\"x\"", "Exactly x, not xy"),
            ("tags:\"x+y\"", "Exact, operators literal"),
        ],
    ),
    ("Notes", &[("no prefix", "Full-text search")]),
];

/// The section set shown on each help tab.
fn help_sections(
    tab: HelpTab,
) -> &'static [(&'static str, &'static [(&'static str, &'static str)])] {
    match tab {
        HelpTab::Shortcuts => &HELP_SECTIONS,
        HelpTab::Search => &SEARCH_SECTIONS,
    }
}

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
    widths: &[u16],
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
                // Pad every cell — real, short, or missing — to its own column's
                // width so the rules run dead straight down the table.
                let col_w = widths.get(c).copied().unwrap_or(0) as usize;
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

/// Bottom hint chips for the help overlay: switch tabs, and close.
const HELP_HINTS: [Hint; 2] = [
    Hint::new("switch tab", "tab ←→", HintId::HelpSwitchTab),
    Hint::new("close", "esc", HintId::CancelOverlay),
];

/// Bottom hint chip for the editor's shortcut reference: close.
const EDITOR_SHORTCUT_HINTS: [Hint; 1] = [Hint::new("close", "esc", HintId::CancelOverlay)];

/// Rows above the table when the dialog carries tabs: the strip, its separator,
/// and a blank line. The other tabbed dialogs skip that blank line, but this
/// table opens with its own faint rule, and two rules a row apart smudge.
const SECTION_TAB_CHROME: u16 = 3;
/// A blank row between the table and the hint line.
const SECTION_HINT_SPACER: u16 = 1;

/// Draw the global help overlay: the two-tab reference (keyboard shortcuts and
/// search commands) opened with `?` from browse or a search result. The active
/// `tab` picks the section set and drives the tab strip.
pub(crate) fn draw_help(
    theme: &Theme,
    frame: &mut Frame<'_>,
    tab: HelpTab,
    hover: HoverTarget,
    scroll: &mut u16,
) {
    draw_section_dialog(theme, frame, &help_spec(tab), hover, scroll);
}

/// The help overlay's layout, exposed so the interaction map registers its tab
/// and hint regions from the very rects the draw uses (mirrors the filter
/// dialog).
pub(crate) fn help_dialog_layout(
    theme: &Theme,
    frame_area: Rect,
    tab: HelpTab,
) -> SectionDialogLayout {
    section_dialog_layout(theme, frame_area, &help_spec(tab))
}

pub(crate) fn help_dialog_hints() -> &'static [Hint] {
    &HELP_HINTS
}

/// The table's shape at a given width, worked out without building a line — so
/// sizing is free and the body renders from the same plan.
struct SectionGrid {
    /// Each section's height, in [`section_block`]'s terms: heading, rule,
    /// bindings. Must stay in lockstep with that function.
    sizes: Vec<usize>,
    /// Each column's end index into `sizes`; the last is always `sizes.len()`.
    bounds: Vec<usize>,
    /// Each column's width, sized to the widest section it holds.
    widths: Vec<u16>,
}

impl SectionGrid {
    /// Rows the spliced table occupies — its tallest column.
    fn rows(&self) -> u16 {
        column_span(&self.sizes, &self.bounds) as u16
    }

    /// Columns the table spans, the rules between them included.
    fn content_w(&self) -> u16 {
        let ncols = self.widths.len() as u16;
        self.widths.iter().sum::<u16>() + HELP_RULE_PAD * ncols.saturating_sub(1)
    }
}

/// Plan the grid for `sections` in `avail_w` columns of room. Widening until the
/// room or the column cap runs out keeps the table short and square; the cut then
/// balances the columns so none towers over the others.
///
/// Column count is fitted against the widest section — the worst case — but each
/// column then takes only the width it needs, so a column of narrow sections
/// doesn't inherit the table's widest row.
fn section_grid(sections: &[(&str, &[(&str, &str)])], avail_w: u16) -> SectionGrid {
    let section_widths: Vec<u16> = sections
        .iter()
        .map(|(_, items)| section_width(items) as u16)
        .collect();
    let widest = section_widths.iter().copied().max().unwrap_or(0);
    let fit = ((avail_w + HELP_RULE_PAD) / (widest + HELP_RULE_PAD)).max(1) as usize;
    let ncols = fit.min(HELP_MAX_COLS).min(sections.len().max(1));
    let sizes: Vec<usize> = sections.iter().map(|(_, items)| items.len() + 2).collect();
    let bounds = balanced_splits(&sizes, ncols);

    let mut widths = Vec::with_capacity(bounds.len());
    let mut start = 0;
    for &end in &bounds {
        widths.push(
            section_widths[start..end]
                .iter()
                .copied()
                .max()
                .unwrap_or(0),
        );
        start = end;
    }

    SectionGrid {
        sizes,
        bounds,
        widths,
    }
}

/// Render the `sections` into the table body `grid` planned.
fn section_table_body(
    theme: &Theme,
    sections: &[(&str, &[(&str, &str)])],
    grid: &SectionGrid,
) -> Vec<Line<'static>> {
    let mut columns = Vec::with_capacity(grid.bounds.len());
    let mut start = 0;
    for (index, &end) in grid.bounds.iter().enumerate() {
        let width = grid.widths[index] as usize;
        let mut column: Vec<Line<'static>> = Vec::new();
        // A blank row separates sections stacked in the same column.
        for (group, items) in &sections[start..end] {
            if !column.is_empty() {
                column.push(Line::default());
            }
            column.extend(section_block(theme, group, items, width));
        }
        columns.push(column);
        start = end;
    }
    section_table_lines(theme, &columns, &grid.widths)
}

/// A section-table cheatsheet dialog's content: its title, section set, the
/// active tab (`None` for a tabless dialog), and its bottom hint chips.
struct SectionDialogSpec<'a> {
    title: &'a str,
    sections: &'a [(&'a str, &'a [(&'a str, &'a str)])],
    active_tab: Option<HelpTab>,
    hints: &'a [Hint],
}

impl SectionDialogSpec<'_> {
    /// The tab strip at full labels — the least the body may be without the
    /// strip collapsing, and the marker that this dialog carries tabs at all.
    /// `None` for the tabless editor reference.
    fn strip_width(&self) -> Option<u16> {
        self.active_tab.map(|_| full_strip_width::<HelpTab>())
    }
}

fn help_spec(tab: HelpTab) -> SectionDialogSpec<'static> {
    SectionDialogSpec {
        title: "Help",
        sections: help_sections(tab),
        active_tab: Some(tab),
        hints: &HELP_HINTS,
    }
}

fn editor_shortcuts_spec() -> SectionDialogSpec<'static> {
    SectionDialogSpec {
        title: "Editor Shortcuts",
        sections: &EDITOR_SHORTCUT_SECTIONS,
        active_tab: None,
        hints: &EDITOR_SHORTCUT_HINTS,
    }
}

/// The rects of a section-table dialog: the outer `area`, an optional `tabs`
/// strip and `separator` (a blank row follows the separator), the `table` (the
/// scrollable body, centered), the `track` its scrollbar rides, and the bottom
/// `hints` block. Drawing and hit-testing both derive from this so they can never
/// drift. `total` is the table's full row count, for the scroll clamp.
#[derive(Clone, Copy)]
pub(crate) struct SectionDialogLayout {
    pub(crate) area: Rect,
    pub(crate) tabs: Option<Rect>,
    separator: Option<Rect>,
    table: Rect,
    pub(crate) track: Rect,
    pub(crate) hints: Rect,
    pub(crate) total: u16,
}

/// Horizontal room a section table lays its columns out in — shared by the sizing
/// pass and the draw so both pick the same column count.
fn section_avail_width(theme: &Theme, frame_area: Rect) -> u16 {
    frame_area
        .width
        .saturating_sub(surface_outer_width(theme, 0))
}

fn section_dialog_layout(
    theme: &Theme,
    frame_area: Rect,
    spec: &SectionDialogSpec<'_>,
) -> SectionDialogLayout {
    let grid = section_grid(spec.sections, section_avail_width(theme, frame_area));
    let content_w = grid.content_w();
    let total = grid.rows();

    let strip_w = spec.strip_width();
    // Leave the title room (and, in flat chrome, the `esc` hint sharing its row).
    let title_w = UnicodeWidthStr::width(spec.title) as u16 + 6;
    let body_w = content_w.max(strip_w.unwrap_or(0)).max(title_w);
    let outer_w = surface_outer_width(theme, body_w).min(frame_area.width);

    // Probe the inner width to wrap the hint line, then size the box to fit.
    let probe = centered_rect_fixed_size(outer_w, 1, frame_area);
    let hint_h = hint_height(spec.hints, dialog_content_full(theme, probe).width);
    let chrome = if strip_w.is_some() {
        SECTION_TAB_CHROME
    } else {
        0
    };
    let overhead = dialog_frame_rows(theme) + chrome + SECTION_HINT_SPACER + hint_h;

    let avail_h = frame_area.height.saturating_sub(2).max(3);
    let visible = total.min(avail_h.saturating_sub(overhead)).max(1);
    let outer_h = (overhead + visible).min(avail_h);
    let area = centered_rect_fixed_size(outer_w, outer_h, frame_area);

    let inner = dialog_content_full(theme, area);
    let (tabs, separator) = if strip_w.is_some() {
        (Some(dialog_row(inner, 0)), Some(dialog_row(inner, 1)))
    } else {
        (None, None)
    };
    let body_h = inner
        .height
        .saturating_sub(chrome + SECTION_HINT_SPACER + hint_h);
    // The bar hangs off the content box, not the centered table, so it lands in
    // the same column as every other dialog list's.
    let list_w = dialog_list_width(theme, inner.width, total as usize, body_h);
    // Center the table within the (possibly wider) content box, below the chrome.
    // The box normally has a spare column for the bar; a terminal too narrow for
    // the content clamps it away, so hold the table off the bar's column.
    let table = Rect {
        x: inner.x + inner.width.saturating_sub(content_w) / 2,
        y: inner.y + chrome,
        width: content_w.min(inner.width).min(list_w.saturating_add(1)),
        height: body_h,
    };
    let track = Rect {
        x: inner.x,
        y: table.y,
        width: list_w,
        height: body_h,
    };
    SectionDialogLayout {
        area,
        tabs,
        separator,
        table,
        track,
        hints: dialog_hints_rect(inner, hint_h),
        total,
    }
}

/// Draw a section-table cheatsheet as a centered dialog: the shared frame (with
/// the flat-mode `esc` hint), an optional tab strip with a separator and a blank
/// line beneath it, the scrollable table, and a bottom hint line. Shared by the
/// global help overlay and the editor's shortcut reference.
fn draw_section_dialog(
    theme: &Theme,
    frame: &mut Frame<'_>,
    spec: &SectionDialogSpec<'_>,
    hover: HoverTarget,
    scroll: &mut u16,
) {
    let layout = section_dialog_layout(theme, frame.area(), spec);
    *scroll = (*scroll).min(layout.total.saturating_sub(layout.table.height));

    draw_dialog_frame_wide(theme, frame, layout.area, spec.title, true);

    if let (Some(active), Some(tabs_rect), Some(sep_rect)) =
        (spec.active_tab, layout.tabs, layout.separator)
    {
        let hovered = match hover {
            HoverTarget::HelpTab(tab) => Some(tab),
            _ => None,
        };
        frame.render_widget(
            // Modal dialog: the active tab always reads as focused.
            Paragraph::new(tab_strip_line::<HelpTab>(
                theme,
                active,
                true,
                hovered,
                0,
                tabs_rect.width,
            )),
            tabs_rect,
        );
        render_separator(theme, frame, sep_rect);
    }

    let grid = section_grid(spec.sections, section_avail_width(theme, frame.area()));
    let lines = section_table_body(theme, spec.sections, &grid);
    frame.render_widget(Paragraph::new(lines).scroll((*scroll, 0)), layout.table);
    render_dialog_list_scrollbar(
        theme,
        frame,
        layout.track,
        layout.total as usize,
        *scroll as usize,
        true,
    );
    render_hint_line(theme, frame, spec.hints, layout.hints, hover);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each column takes only the width its own sections need. A uniform width
    /// would pad the narrow columns out to the table's widest row, which is what
    /// pushed the search reference past most terminals.
    #[test]
    fn columns_size_to_their_own_sections() {
        for sections in [
            &SEARCH_SECTIONS[..],
            &HELP_SECTIONS[..],
            &EDITOR_SHORTCUT_SECTIONS[..],
        ] {
            let grid = section_grid(sections, 200);
            assert!(
                grid.widths.len() > 1,
                "the probe width fits several columns"
            );
            let widest = grid.widths.iter().copied().max().unwrap();
            let uniform =
                widest * grid.widths.len() as u16 + HELP_RULE_PAD * (grid.widths.len() as u16 - 1);
            assert!(
                grid.content_w() < uniform,
                "expected narrower than a uniform grid: {} vs {uniform}",
                grid.content_w()
            );
            // Every column still holds its own widest section without clipping.
            let mut start = 0;
            for (index, &end) in grid.bounds.iter().enumerate() {
                let needed = sections[start..end]
                    .iter()
                    .map(|(_, items)| section_width(items) as u16)
                    .max()
                    .unwrap();
                assert_eq!(grid.widths[index], needed);
                start = end;
            }
        }
    }

    /// Every tab's table fits an 80-column terminal unclipped — the search
    /// reference is the widest, and sizing the columns to their own sections is
    /// what bought it the room.
    #[test]
    fn every_tab_fits_an_eighty_column_terminal() {
        let theme = Theme::terminal_default();
        let frame_area = Rect::new(0, 0, 80, 40);
        for spec in [
            help_spec(HelpTab::Shortcuts),
            help_spec(HelpTab::Search),
            editor_shortcuts_spec(),
        ] {
            let layout = section_dialog_layout(&theme, frame_area, &spec);
            let content_w =
                section_grid(spec.sections, section_avail_width(&theme, frame_area)).content_w();
            assert_eq!(
                layout.table.width, content_w,
                "{}: the table is clipped at {} of {content_w} columns",
                spec.title, layout.table.width
            );
            assert!(layout.area.width <= frame_area.width);
        }
    }

    /// The table never runs into the scrollbar's column. Flat chrome normally
    /// leaves the box one spare column for the bar, but a terminal too narrow
    /// for the content clamps the box down and that spare disappears — the
    /// table has to give way, not paint underneath the bar.
    #[test]
    fn the_table_never_reaches_the_scrollbar_column() {
        use crate::tui::render::chrome::dialog_list_scrollbar_rect;
        use crate::tui::theme::{ChromeStyle, test_flat_theme};

        for theme in [
            Theme::terminal_default(),
            test_flat_theme().with_chrome_override(Some(ChromeStyle::Flat)),
        ] {
            for width in 20..=100 {
                let frame_area = Rect::new(0, 0, width, 30);
                for spec in [
                    help_spec(HelpTab::Shortcuts),
                    help_spec(HelpTab::Search),
                    editor_shortcuts_spec(),
                ] {
                    let layout = section_dialog_layout(&theme, frame_area, &spec);
                    // No overflow means no bar is drawn, so the column is free.
                    if layout.total <= layout.track.height {
                        continue;
                    }
                    let bar = dialog_list_scrollbar_rect(layout.track);
                    assert!(
                        layout.table.x + layout.table.width <= bar.x,
                        "{} at {width} cols: table ends at {} but the bar is at {}",
                        spec.title,
                        layout.table.x + layout.table.width,
                        bar.x
                    );
                }
            }
        }
    }
}
