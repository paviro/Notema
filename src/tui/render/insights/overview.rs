//! The Overview tab: an at-a-glance summary — a title line and a compact grid of
//! the highest-signal headline figures. Deliberately *not* a dashboard: the
//! distributions and histories behind these numbers live on the dedicated tabs;
//! here each area contributes a single figure.

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use notema_analytics::{Analytics, MoodAnalytics};

use super::widgets::{Stat, columns_for, draw_stat_card, draw_stats};
use crate::tui::render::{count_label, flat_chrome, render_centered_notice};
use crate::tui::theme::Theme;

/// Weekday labels indexed Monday (`0`) through Sunday (`6`), matching
/// `MoodAnalytics::by_weekday`.
const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// Preferred card size; both shrink to fit a small panel. Height holds the two
/// lines (label / value) centred, with one blank row above and below.
const CARD_WIDTH: u16 = 34;
const CARD_HEIGHT: u16 = 6;
/// The full-width title box height: two borders around three inner lines, the
/// text centred on the middle one.
const TITLE_HEIGHT: u16 = 5;
/// The narrowest card worth boxing; below it `draw_stat_card` collapses anyway.
const MIN_CARD_WIDTH: u16 = 4;

/// The shortest card that still shows both of its lines, plus the two border rows
/// when the chrome draws them.
const fn min_card_height(flat: bool) -> u16 {
    if flat { 2 } else { 4 }
}

/// The placed title box and card grid. Nothing clips drawing back to the panel,
/// so the block is measured against the area first and abandoned when it doesn't
/// fit — see [`card_grid`].
struct CardGrid {
    title: Rect,
    origin_x: u16,
    grid_top: u16,
    cols: u16,
    card_w: u16,
    card_h: u16,
    gap_x: u16,
    gap_y: u16,
}

pub(super) fn draw(
    theme: &Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    analytics: &Analytics,
    title: &str,
) {
    let cadence = &analytics.cadence;
    if cadence.total_entries == 0 {
        render_centered_notice(theme, frame, area, "No entries");
        return;
    }

    let flat = flat_chrome(theme);
    let stats = metrics(theme, analytics);
    let Some(grid) = card_grid(area, flat, stats.len()) else {
        draw_compact(theme, frame, area, title, cadence, &stats);
        return;
    };

    draw_title_box(theme, frame, grid.title, title, cadence);

    for (index, stat) in stats.iter().enumerate() {
        let col = index as u16 % grid.cols;
        let row = index as u16 / grid.cols;
        draw_stat_card(
            theme,
            frame,
            Rect {
                x: grid.origin_x + col * (grid.card_w + grid.gap_x),
                y: grid.grid_top + row * (grid.card_h + grid.gap_y),
                width: grid.card_w,
                height: grid.card_h,
            },
            stat,
        );
    }
}

/// Place the title box and the card grid inside `area`, or `None` when the cards
/// would come out too small to read — the caller then falls back to compact rows.
/// Every returned rect lies within `area`: card heights take whole rows only, so
/// the division remainder stays as outer margin rather than spilling past the
/// panel bottom.
fn card_grid(area: Rect, flat: bool, stats: usize) -> Option<CardGrid> {
    if stats == 0 {
        return None;
    }
    // Blank cells left between adjacent boxes. Flat cards carry no border, so they
    // need their own separation — a blank row between rows and a wider gutter
    // between columns to stay balanced; bordered cards keep the tighter spacing.
    // Flat cards are also trimmed a column: filled solid, the full-width tile reads
    // heavier than the hollow bordered box, so shrink it to compensate.
    let (gap_x, gap_y) = if flat { (2, 1) } else { (1, 0) };
    let card_w_max = if flat { CARD_WIDTH - 1 } else { CARD_WIDTH };

    // Prefer the widest column count that divides the cards evenly, so the grid
    // stays balanced (6 → 2×3 rather than 3×2). Capped at two so the paired cards
    // read as rows and the block keeps its familiar width rather than spreading.
    let max_cols = columns_for(area).min(stats).clamp(1, 2);
    let cols = (1..=max_cols)
        .rev()
        .find(|c| stats.is_multiple_of(*c))
        .unwrap_or(max_cols) as u16;
    let rows = stats.div_ceil(cols as usize) as u16;

    // Cards are narrower than the panel; the title box above spans their combined
    // width. The whole block is centred so the slack becomes an outer margin.
    let card_w = card_w_max.min(area.width.saturating_sub(gap_x * (cols - 1)) / cols);
    let grid_h = area
        .height
        .saturating_sub(TITLE_HEIGHT + gap_y + (rows - 1) * gap_y);
    let card_h = (grid_h / rows).min(CARD_HEIGHT);
    if card_w < MIN_CARD_WIDTH || card_h < min_card_height(flat) {
        return None;
    }

    let block_w = card_w * cols + gap_x * (cols - 1);
    let block_h = TITLE_HEIGHT + gap_y + card_h * rows + (rows - 1) * gap_y;
    let origin_x = area.x + area.width.saturating_sub(block_w) / 2;
    let origin_y = area.y + area.height.saturating_sub(block_h) / 2;

    Some(CardGrid {
        title: Rect {
            x: origin_x,
            y: origin_y,
            width: block_w,
            height: TITLE_HEIGHT,
        },
        origin_x,
        grid_top: origin_y + TITLE_HEIGHT + gap_y,
        cols,
        card_w,
        card_h,
        gap_x,
        gap_y,
    })
}

/// The same figures for a panel too small to box them: the title as a plain line
/// and the stats as compact rows, both bounded by `area`.
fn draw_compact(
    theme: &Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    cadence: &notema_analytics::Cadence,
    stats: &[Stat],
) {
    if area.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new(Line::from(title_spans(theme, title, cadence))),
        Rect { height: 1, ..area },
    );
    draw_stats(
        theme,
        frame,
        Rect {
            y: area.y + 1,
            height: area.height - 1,
            ..area
        },
        stats,
    );
}

/// A full-width bordered box holding the journal name, its date span, and the
/// headline totals, centred over the cards below.
fn draw_title_box(
    theme: &Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    cadence: &notema_analytics::Cadence,
) {
    let spans = title_spans(theme, title, cadence);
    // Flat mode fills the tile with the card surface colour and drops the border so
    // it matches the flat cards below; bordered mode keeps the drawn box. The text
    // sits on the middle row: with a border the two border rows plus one leading
    // blank line centre it; without one, pad up to the vertical centre directly.
    let block;
    let pad_top;
    if flat_chrome(theme) {
        block = Block::new().style(Style::default().bg(theme.raised_bg()));
        pad_top = area.height.saturating_sub(1) as usize / 2;
    } else {
        block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme.card_border());
        pad_top = 1;
    }
    let lines = std::iter::repeat_n(Line::default(), pad_top)
        .chain(std::iter::once(Line::from(spans)))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .block(block),
        area,
    );
}

/// The journal name, its date span, and the headline totals — the title box's
/// contents, and the compact fallback's first line.
fn title_spans(
    theme: &Theme,
    title: &str,
    cadence: &notema_analytics::Cadence,
) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(title.to_string(), theme.heading())];
    if let Some(span) = date_span(cadence.date_span) {
        spans.push(Span::styled(format!(" · {span}"), theme.muted()));
    }
    spans.push(Span::styled(
        format!(
            " · {} · {}",
            count_label(cadence.total_entries, "entry", "entries"),
            count_label(cadence.total_words, "word", "words"),
        ),
        theme.muted(),
    ));
    spans
}

/// The six headline figures, paired so the grid reads as lift/drain, best/worst
/// day, and recent feeling / how many days you showed up. Chosen to point at what
/// moves your mood rather than to judge — the "drains" and "toughest" cards name
/// what to watch, not to blame.
fn metrics(theme: &Theme, analytics: &Analytics) -> Vec<Stat> {
    vec![
        lifts_stat(theme, analytics),
        drains_stat(theme, analytics),
        happiest_day_stat(theme, &analytics.mood),
        toughest_day_stat(theme, &analytics.mood),
        top_feeling_stat(theme, analytics),
        Stat::new(
            theme,
            "Active days",
            analytics.cadence.active_days.to_string(),
        ),
    ]
}

/// The people and things linked to your better moods (rotated daily): a person on
/// the value line, an activity or tag beneath it. Falls back to whichever exists.
fn lifts_stat(theme: &Theme, analytics: &Analytics) -> Stat {
    match (
        &analytics.highlights.lifts_person,
        &analytics.highlights.lifts_thing,
    ) {
        (Some(person), Some(thing)) => Stat::new(theme, "Lifts you", person.clone())
            .sub(Span::styled(thing.clone(), theme.muted())),
        (Some(name), None) | (None, Some(name)) => Stat::new(theme, "Lifts you", name.clone()),
        (None, None) => Stat::new(theme, "Lifts you", "—"),
    }
}

/// The mirror of [`lifts_stat`]: the people and things linked to your worse moods.
fn drains_stat(theme: &Theme, analytics: &Analytics) -> Stat {
    match (
        &analytics.highlights.drains_person,
        &analytics.highlights.drains_thing,
    ) {
        (Some(person), Some(thing)) => Stat::new(theme, "Drains you", person.clone())
            .sub(Span::styled(thing.clone(), theme.muted())),
        (Some(name), None) | (None, Some(name)) => Stat::new(theme, "Drains you", name.clone()),
        (None, None) => Stat::new(theme, "Drains you", "—"),
    }
}

/// This year's most-logged feeling, noted as such; falls back to the all-time top
/// feeling (noted) when this year has none yet.
fn top_feeling_stat(theme: &Theme, analytics: &Analytics) -> Stat {
    if let Some(name) = &analytics.highlights.top_feeling_this_year {
        Stat::new(theme, "Top feeling", name.clone()).sub(Span::styled("this year", theme.muted()))
    } else if let Some(tally) = analytics.mood.feelings.first() {
        Stat::new(theme, "Top feeling", tally.name.clone())
            .sub(Span::styled("all time", theme.muted()))
    } else {
        Stat::new(theme, "Top feeling", "—")
    }
}

/// The weekday whose entries average the highest mood.
fn happiest_day_stat(theme: &Theme, mood: &MoodAnalytics) -> Stat {
    match extreme_weekday(mood, true) {
        Some(day) => Stat::new(theme, "Happiest day", WEEKDAYS[day].to_string()),
        None => Stat::new(theme, "Happiest day", "—"),
    }
}

/// The mirror of [`happiest_day_stat`]: the weekday whose entries average the
/// lowest mood — the day worth a little extra care.
fn toughest_day_stat(theme: &Theme, mood: &MoodAnalytics) -> Stat {
    match extreme_weekday(mood, false) {
        Some(day) => Stat::new(theme, "Toughest day", WEEKDAYS[day].to_string()),
        None => Stat::new(theme, "Toughest day", "—"),
    }
}

/// The weekday index with the highest (`best`) or lowest average mood, or `None`
/// when no weekday has a mood logged.
fn extreme_weekday(mood: &MoodAnalytics, best: bool) -> Option<usize> {
    let scored = mood
        .by_weekday
        .iter()
        .enumerate()
        .filter_map(|(day, avg)| avg.map(|avg| (day, avg)));
    if best {
        scored
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(day, _)| day)
    } else {
        scored
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(day, _)| day)
    }
}

/// The journal's date span as `2023 – 2024`, or `None` when undated.
fn date_span(span: Option<(jiff::civil::Date, jiff::civil::Date)>) -> Option<String> {
    let (first, last) = span?;
    if first.year() == last.year() {
        Some(first.year().to_string())
    } else {
        Some(format!("{} – {}", first.year(), last.year()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATS: usize = 6;

    fn area(width: u16, height: u16) -> Rect {
        // Offset the origin so a layout that ignores it shows up as an overflow.
        Rect {
            x: 3,
            y: 2,
            width,
            height,
        }
    }

    /// The bottom row the grid's last card would occupy, exclusive.
    fn grid_bottom(grid: &CardGrid, stats: usize) -> u16 {
        let rows = stats.div_ceil(grid.cols as usize) as u16;
        grid.grid_top + rows * (grid.card_h + grid.gap_y) - grid.gap_y
    }

    #[test]
    fn every_placed_grid_stays_inside_the_panel() {
        for flat in [false, true] {
            for width in [20u16, 34, 40, 56, 80, 120] {
                for height in 0u16..40 {
                    let area = area(width, height);
                    let Some(grid) = card_grid(area, flat, STATS) else {
                        continue;
                    };
                    assert!(
                        grid.title.y >= area.y && grid_bottom(&grid, STATS) <= area.bottom(),
                        "grid overflows {width}x{height} (flat: {flat})",
                    );
                    let block_right =
                        grid.origin_x + grid.cols * (grid.card_w + grid.gap_x) - grid.gap_x;
                    assert!(
                        grid.title.x >= area.x && block_right <= area.right(),
                        "grid overflows {width}x{height} horizontally (flat: {flat})",
                    );
                }
            }
        }
    }

    #[test]
    fn short_panels_fall_back_instead_of_overflowing() {
        // A two-column grid needs the title box plus three four-row cards; a
        // one-column grid needs six. Anything shorter must fall back rather than
        // draw past the panel bottom.
        assert!(card_grid(area(80, 16), false, STATS).is_none());
        assert!(card_grid(area(80, 17), false, STATS).is_some());
        assert!(card_grid(area(40, 28), false, STATS).is_none());
        assert!(card_grid(area(40, 29), false, STATS).is_some());
    }

    #[test]
    fn narrow_panels_fall_back_rather_than_boxing_slivers() {
        // Narrower than a box with anything inside it — `draw_stat_card` would
        // collapse to a centred line anyway, which the compact rows do better.
        assert!(card_grid(area(3, 40), false, STATS).is_none());
    }
}
