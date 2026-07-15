//! Bordered ASCII-table primitives shared by the insights correlate table and
//! the editor's popup dialogs. Callers assemble their own rows (so each can style
//! and align cells its own way) and lean on these for the box-drawing grid.

use ratatui::{
    style::Style,
    text::{Line, Span},
};

use crate::tui::theme::Theme;

/// The muted style used for table borders and header labels.
pub(crate) fn themed_border_style(theme: &Theme) -> Style {
    theme.muted()
}

/// The fainter style for the dashes of an inter-row rule, so the grid lines
/// *between* data rows read lighter than the outer border.
pub(crate) fn themed_faint_rule_style(theme: &Theme) -> Style {
    theme.faint_rule()
}

/// A dim column border in the theme's line set.
pub(crate) fn themed_border(theme: &Theme) -> Span<'static> {
    Span::styled(
        theme.glyphs().borders.line_set().vertical.to_string(),
        themed_border_style(theme),
    )
}

/// Where a horizontal rule sits in the grid, deciding its corner and junction
/// glyphs.
#[derive(Clone, Copy)]
pub(crate) enum RulePos {
    Top,
    Mid,
    Bottom,
    /// An inter-row rule: the column borders run straight through as plain
    /// verticals (no junctions), so the vertical lines stay continuous.
    Row,
}

/// Pad `text` to `width`, right-aligned for numeric columns and left otherwise.
pub(crate) fn pad(text: &str, width: usize, right: bool) -> String {
    if right {
        format!("{text:>width$}")
    } else {
        format!("{text:<width$}")
    }
}

/// Push a padded cell (` content `) plus its trailing column border.
pub(crate) fn themed_push_cell(
    theme: &Theme,
    spans: &mut Vec<Span<'static>>,
    content: Span<'static>,
) {
    spans.push(Span::raw(" "));
    spans.push(content);
    spans.push(Span::raw(" "));
    spans.push(themed_border(theme));
}

/// A horizontal border rule spanning `widths`, e.g. `┌────┬────┐`, drawn in
/// the theme's line set. The junction glyphs (which sit on the vertical column
/// borders) take `junction` and the horizontal fill takes `dash`; giving
/// inter-row rules a fainter `dash` but a full-weight `junction` keeps the
/// vertical column lines uniform instead of banding where the rules cross them.
pub(crate) fn themed_rule(
    theme: &Theme,
    widths: &[usize],
    pos: RulePos,
    junction: Style,
    dash: Style,
) -> Line<'static> {
    let set = theme.glyphs().borders.line_set();
    let (left, mid, right) = match pos {
        RulePos::Top => (set.top_left, set.horizontal_down, set.top_right),
        RulePos::Mid => (set.vertical_right, set.cross, set.vertical_left),
        RulePos::Bottom => (set.bottom_left, set.horizontal_up, set.bottom_right),
        RulePos::Row => (set.vertical, set.vertical, set.vertical),
    };
    let mut spans = vec![Span::styled(left.to_string(), junction)];
    for (i, w) in widths.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(mid.to_string(), junction));
        }
        spans.push(Span::styled(set.horizontal.repeat(w + 2), dash));
    }
    spans.push(Span::styled(right.to_string(), junction));
    Line::from(spans)
}

/// Push a padded cell (` c0 c1 … `) built from several spans, plus its trailing
/// column border. The spans must already be padded to the column width.
pub(crate) fn themed_push_cell_spans(
    theme: &Theme,
    spans: &mut Vec<Span<'static>>,
    cell: Vec<Span<'static>>,
) {
    spans.push(Span::raw(" "));
    spans.extend(cell);
    spans.push(Span::raw(" "));
    spans.push(themed_border(theme));
}
