//! Responsive building blocks shared by the insight tabs. Every widget measures
//! the `Rect` it is handed and adapts — none knows whether it is drawing into a
//! side column or an expanded full-screen panel. All colour comes from
//! [`theme`], so the blocks stay legible with the palette stripped.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::tui::entry_rows::{DividerAlign, section_divider, text_width, truncate_ellipsis};
use crate::tui::theme::theme;

/// Intra-panel composition breakpoints, measured from the content `Rect` (not
/// the terminal). Named so the responsiveness tests can pin them.
pub(crate) const TWO_COL_MIN_WIDTH: u16 = 56;
pub(crate) const THREE_COL_MIN_WIDTH: u16 = 92;
/// Below this height a tab drops boxed cards for compact one-line rows.
pub(crate) const SHORT_HEIGHT: u16 = 14;

/// One entry in a vertical [`stack`]: a minimum height to be included at all,
/// and a fill weight for sharing the leftover rows.
pub(crate) struct Section {
    pub(crate) min: u16,
    pub(crate) fill: u16,
}

impl Section {
    pub(crate) fn new(min: u16, fill: u16) -> Self {
        Self { min, fill }
    }
}

/// Stack `sections` top-to-bottom in `area`. Each is included only if its `min`
/// still fits (later ones drop out first — put highest-signal sections first);
/// leftover height is shared by `fill` weight, with any rounding remainder given
/// to the last filling section so the area is fully used. Returns one rect per
/// section (`None` when it didn't fit).
pub(crate) fn stack(area: Rect, sections: &[Section]) -> Vec<Option<Rect>> {
    let mut included = Vec::with_capacity(sections.len());
    let mut used = 0u16;
    for section in sections {
        let fits = used + section.min <= area.height;
        if fits {
            used += section.min;
        }
        included.push(fits);
    }

    let total_fill: u16 = sections
        .iter()
        .zip(&included)
        .filter(|(_, keep)| **keep)
        .map(|(section, _)| section.fill)
        .sum();
    let extra = area.height - used;

    // Pre-compute each section's fill share; hand the remainder to the last
    // filling section so no rows are left blank at the bottom.
    let last_fill = sections
        .iter()
        .enumerate()
        .rev()
        .find(|(idx, section)| included[*idx] && section.fill > 0)
        .map(|(idx, _)| idx);
    let mut assigned = 0u16;

    let mut result = vec![None; sections.len()];
    let mut y = area.y;
    for (idx, section) in sections.iter().enumerate() {
        if !included[idx] {
            continue;
        }
        let mut height = section.min;
        if total_fill > 0 && section.fill > 0 {
            let mut share = extra * section.fill / total_fill;
            if Some(idx) == last_fill {
                share = extra - assigned;
            }
            assigned += share;
            height += share;
        }
        result[idx] = Some(Rect {
            x: area.x,
            y,
            width: area.width,
            height,
        });
        y += height;
    }
    result
}

/// Draw a section heading as the app's shared divider rule — a bold left label
/// trailed by a `━` line (`Balance ━━━━━━`), matching the entry list's month
/// headers and the journals column's "Archived" divider. One blank row precedes
/// it to set it off from the section above; the title takes the lone row when the
/// area is a single line high. Returns the area below the title.
pub(crate) fn heading(frame: &mut Frame<'_>, area: Rect, text: &str) -> Rect {
    if area.height == 0 {
        return area;
    }
    let title_y = if area.height >= 2 { area.y + 1 } else { area.y };
    frame.render_widget(
        Paragraph::new(section_divider(
            area.width as usize,
            text,
            DividerAlign::Left,
        )),
        Rect {
            y: title_y,
            height: 1,
            ..area
        },
    );
    let used = title_y + 1 - area.y;
    Rect {
        y: title_y + 1,
        height: area.height - used,
        ..area
    }
}

/// Draw a dim caption line (e.g. a histogram axis) in `area`'s first row.
pub(crate) fn caption(frame: &mut Frame<'_>, area: Rect, text: &str) {
    if area.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text.to_string(), theme().muted()))),
        Rect { height: 1, ..area },
    );
}

/// How many card columns the area affords.
pub(crate) fn columns_for(area: Rect) -> usize {
    if area.width >= THREE_COL_MIN_WIDTH {
        3
    } else if area.width >= TWO_COL_MIN_WIDTH {
        2
    } else {
        1
    }
}

/// Whether the area is too short for boxed cards / multi-widget stacks.
pub(crate) fn is_short(area: Rect) -> bool {
    area.height < SHORT_HEIGHT
}

/// Split `area` into a row-major grid of `cols × rows` even cells.
pub(crate) fn grid(area: Rect, cols: usize, rows: usize) -> Vec<Rect> {
    if cols == 0 || rows == 0 {
        return Vec::new();
    }
    let row_rects = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Fill(1); rows])
        .split(area);
    let mut cells = Vec::with_capacity(cols * rows);
    for row in row_rects.iter() {
        let col_rects = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Fill(1); cols])
            .split(*row);
        cells.extend(col_rects.iter().copied());
    }
    cells
}

/// A single headline metric: a dim label, a bold value, and an optional dim
/// sub-line (a secondary figure, trend, or unit).
pub(crate) struct Stat {
    pub(crate) label: String,
    pub(crate) value: String,
    pub(crate) value_style: ratatui::style::Style,
    pub(crate) sub: Option<Span<'static>>,
}

impl Stat {
    pub(crate) fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            value_style: theme().heading(),
            sub: None,
        }
    }

    pub(crate) fn styled(mut self, style: ratatui::style::Style) -> Self {
        self.value_style = style;
        self
    }

    pub(crate) fn sub(mut self, sub: Span<'static>) -> Self {
        self.sub = Some(sub);
        self
    }
}

/// Lay a row of headline metrics out as boxed cards, collapsing to compact
/// one-line rows when the area is short.
pub(crate) fn draw_stats(frame: &mut Frame<'_>, area: Rect, stats: &[Stat]) {
    if stats.is_empty() || area.width == 0 || area.height == 0 {
        return;
    }
    if is_short(area) {
        let lines: Vec<Line> = stats.iter().map(stat_row_line).collect();
        frame.render_widget(Paragraph::new(lines), area);
        return;
    }
    let cols = columns_for(area).min(stats.len());
    let rows = stats.len().div_ceil(cols);
    for (cell, stat) in grid(area, cols, rows).into_iter().zip(stats) {
        draw_stat_card(frame, cell, stat);
    }
}

/// A metric as one compact line: `Label  Value  sub`.
fn stat_row_line(stat: &Stat) -> Line<'static> {
    let mut spans = vec![
        Span::styled(format!("{}  ", stat.label), theme().muted()),
        Span::styled(stat.value.clone(), stat.value_style),
    ];
    if let Some(sub) = &stat.sub {
        spans.push(Span::raw(" "));
        spans.push(sub.clone());
    }
    Line::from(spans)
}

/// One metric as a bordered card: value centered and bold, label dim above,
/// optional sub-line below. Falls back to a single centered line if the cell is
/// too short to box. The caller sizes the card — keep it compact so the tile hugs
/// its content rather than boxing empty space.
pub(crate) fn draw_stat_card(frame: &mut Frame<'_>, area: Rect, stat: &Stat) {
    if area.height < 3 || area.width < 4 {
        frame.render_widget(
            Paragraph::new(stat_row_line(stat)).alignment(Alignment::Center),
            area,
        );
        return;
    }
    let mut lines = vec![
        Line::from(Span::styled(stat.label.clone(), theme().muted())),
        Line::from(Span::styled(stat.value.clone(), stat.value_style)),
    ];
    if let Some(sub) = &stat.sub {
        lines.push(Line::from(sub.clone()));
    }
    // Vertically centre the block inside the card. Pad against a fixed three-line
    // slot (label / value / sub) so every card's label and value land on the same
    // rows whether or not it carries a sub-line; round the pad up so the block
    // never hugs the top border on an even inner height.
    let inner_height = area.height.saturating_sub(2) as usize;
    let pad_top = inner_height.saturating_sub(3).div_ceil(2);
    let lines = std::iter::repeat_n(Line::default(), pad_top)
        .chain(lines)
        .collect::<Vec<_>>();
    let card = Paragraph::new(lines).alignment(Alignment::Center).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme().card_border()),
    );
    frame.render_widget(card, area);
}

/// One horizontal bar: a label, a 0..1 fill, a value caption, and the fill style.
pub(crate) struct Bar {
    pub(crate) label: String,
    pub(crate) fill: f32,
    pub(crate) value: String,
    pub(crate) style: ratatui::style::Style,
}

/// Render `bars` as `label ████····  value`, showing the top rows that fit plus
/// a dim `+k more` footer when the list overflows the area.
pub(crate) fn draw_bars(frame: &mut Frame<'_>, area: Rect, bars: &[Bar]) {
    if area.width == 0 || area.height == 0 || bars.is_empty() {
        return;
    }
    let (rows_area, shown, more) = list_regions(area, bars.len());
    let label_w = bars
        .iter()
        .map(|bar| text_width(&bar.label))
        .max()
        .unwrap_or(0)
        .min(14);
    let value_w = bars.iter().map(|bar| bar.value.len()).max().unwrap_or(0);
    // label + ' ' + bar + ' ' + value
    let bar_w = (rows_area.width as usize)
        .saturating_sub(label_w + value_w + 2)
        .max(1);

    let lines: Vec<Line> = bars
        .iter()
        .take(shown)
        .map(|bar| {
            let filled = ((bar.fill.clamp(0.0, 1.0) * bar_w as f32).round() as usize).min(bar_w);
            Line::from(vec![
                Span::raw(format!(
                    "{:<label_w$}",
                    truncate_ellipsis(&bar.label, label_w)
                )),
                Span::raw(" "),
                // `▓` (dark shade) shares the airy texture of the `░` empty track
                // rather than reading as a heavy solid slab.
                Span::styled("▓".repeat(filled), bar.style),
                Span::styled("░".repeat(bar_w - filled), theme().muted()),
                Span::raw(" "),
                Span::raw(format!("{:>value_w$}", bar.value)),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), rows_area);
    draw_more_note(frame, more);
}

/// Split `area` into a rows region and, when `total` overflows, a one-line
/// `+k more` footer. Returns the rows rect, how many to draw, and the footer.
pub(crate) fn list_regions(area: Rect, total: usize) -> (Rect, usize, Option<(Rect, String)>) {
    let capacity = area.height as usize;
    if total <= capacity {
        return (area, total, None);
    }
    let shown = capacity.saturating_sub(1);
    let rows = Rect {
        height: shown as u16,
        ..area
    };
    let footer = Rect {
        y: area.y + shown as u16,
        height: 1,
        ..area
    };
    (
        rows,
        shown,
        Some((footer, format!("+{} more", total - shown))),
    )
}

pub(crate) fn draw_more_note(frame: &mut Frame<'_>, more: Option<(Rect, String)>) {
    if let Some((area, text)) = more {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(text, theme().muted()))),
            area,
        );
    }
}

/// The eighths ramp used by histograms and sparklines: index 0 is blank, 8 full.
const RAMP: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

fn ramp_cell(eighths: usize) -> char {
    RAMP[eighths.min(8)]
}

/// A vertical bar chart of `values` drawn with the block ramp, scaled to the
/// tallest bucket. One cell per column plus a space when it fits; degrades to a
/// one-row sparkline when the area is a single line high.
pub(crate) fn draw_histogram(frame: &mut Frame<'_>, area: Rect, values: &[usize]) {
    if area.width == 0 || area.height == 0 || values.is_empty() {
        return;
    }
    let n = values.len();
    let gap = usize::from((2 * n).saturating_sub(1) <= area.width as usize);
    let max = (*values.iter().max().unwrap_or(&0)).max(1);
    let bar_h = area.height as usize;
    let eighths_per = bar_h * 8;

    let mut lines: Vec<Line> = Vec::with_capacity(bar_h);
    for row in 0..bar_h {
        // Row 0 is the top; `level` counts cells up from the baseline.
        let level = bar_h - row;
        let lower = (level - 1) * 8;
        let mut spans: Vec<Span> = Vec::with_capacity(n * 2);
        for (i, &value) in values.iter().enumerate() {
            if i > 0 && gap == 1 {
                spans.push(Span::raw(" "));
            }
            let filled = (value as f32 / max as f32 * eighths_per as f32).round() as usize;
            let cell = filled.saturating_sub(lower).min(8);
            spans.push(Span::styled(
                ramp_cell(cell).to_string(),
                theme().bar_fill(),
            ));
        }
        lines.push(Line::from(spans));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

/// A proportion bar split into positive / neutral / negative segments by their
/// share, over `width` cells. Segment length carries the proportion (so it reads
/// on monochrome); colour and weight distinguish the three.
pub(crate) fn sentiment_segments(
    positive: usize,
    neutral: usize,
    negative: usize,
    width: usize,
) -> Line<'static> {
    let total = positive + neutral + negative;
    if total == 0 || width == 0 {
        return Line::from(Span::styled("░".repeat(width), theme().muted()));
    }
    let cells = |count: usize| ((count as f32 / total as f32) * width as f32).round() as usize;
    let mut pos = cells(positive);
    let mut neg = cells(negative);
    // Give any rounding remainder to the neutral middle so the bar fills exactly.
    let neu = width.saturating_sub(pos + neg);
    if pos + neu + neg > width {
        // Trim the larger of the coloured ends if rounding overshot.
        if pos >= neg {
            pos = pos.saturating_sub(pos + neu + neg - width);
        } else {
            neg = neg.saturating_sub(pos + neu + neg - width);
        }
    }
    Line::from(vec![
        Span::styled("▓".repeat(pos), theme().positive()),
        Span::styled("▓".repeat(neu), theme().muted()),
        Span::styled("▓".repeat(neg), theme().negative()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(width: u16, height: u16) -> Rect {
        Rect::new(0, 0, width, height)
    }

    #[test]
    fn columns_scale_with_width() {
        assert_eq!(columns_for(rect(40, 20)), 1);
        assert_eq!(columns_for(rect(60, 20)), 2);
        assert_eq!(columns_for(rect(100, 20)), 3);
    }

    #[test]
    fn short_area_is_flagged() {
        assert!(is_short(rect(80, 6)));
        assert!(!is_short(rect(80, 20)));
    }

    #[test]
    fn stack_drops_trailing_sections_that_do_not_fit_and_fills_height() {
        // Only the first two of three min-4 sections fit in 9 rows; the filling
        // section absorbs the leftover so the area is fully used.
        let slots = stack(
            rect(20, 9),
            &[Section::new(4, 1), Section::new(4, 1), Section::new(4, 0)],
        );
        assert!(slots[0].is_some());
        assert!(slots[1].is_some());
        assert!(slots[2].is_none());
        let total: u16 = slots.iter().flatten().map(|rect| rect.height).sum();
        assert_eq!(total, 9);
    }

    #[test]
    fn sentiment_segments_fill_exactly_and_split_by_share() {
        let line = sentiment_segments(3, 0, 1, 8);
        let width: usize = line
            .spans
            .iter()
            .map(|span| span.content.chars().count())
            .sum();
        assert_eq!(width, 8);
        // 3:0:1 over 8 cells → 6 positive, 0 neutral, 2 negative.
        assert_eq!(line.spans[0].content.chars().count(), 6);
        assert_eq!(line.spans[2].content.chars().count(), 2);
    }
}
