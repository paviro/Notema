//! The Overview tab: an at-a-glance summary — a title line and a compact grid of
//! the highest-signal headline figures. Deliberately *not* a dashboard: the
//! distributions and histories behind these numbers live on the dedicated tabs;
//! here each area contributes a single figure.

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use journal_analytics::{Analytics, MoodAnalytics};

use super::widgets::{Stat, columns_for, draw_stat_card};
use crate::tui::render::{count_label, render_centered_notice};
use crate::tui::theme::theme;

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

/// Preferred card size; both shrink to fit a small panel.
const CARD_WIDTH: u16 = 34;
const CARD_HEIGHT: u16 = 7;
/// The full-width title box height: two borders around three inner lines, the
/// text centred on the middle one.
const TITLE_HEIGHT: u16 = 5;
/// Blank cells left between adjacent boxes.
const GAP_X: u16 = 1;
const GAP_Y: u16 = 0;

pub(super) fn draw(frame: &mut Frame<'_>, area: Rect, analytics: &Analytics, title: &str) {
    let cadence = &analytics.cadence;
    if cadence.total_entries == 0 {
        render_centered_notice(frame, area, "No entries yet");
        return;
    }

    let stats = metrics(analytics);
    // Prefer the widest column count that divides the cards evenly, so the grid
    // stays balanced (6 → 2×3 rather than 3×2). Capped at two so the paired cards
    // read as rows and the block keeps its familiar width rather than spreading.
    let max_cols = columns_for(area).min(stats.len()).clamp(1, 2);
    let cols = (1..=max_cols)
        .rev()
        .find(|c| stats.len().is_multiple_of(*c))
        .unwrap_or(max_cols) as u16;
    let rows = stats.len().div_ceil(cols as usize) as u16;

    // Cards are narrower than the panel; the title box above spans their combined
    // width. The whole block is centred so the slack becomes an outer margin.
    let card_w = CARD_WIDTH.min(area.width.saturating_sub(GAP_X * (cols - 1)) / cols);
    let block_w = card_w * cols + GAP_X * (cols - 1);
    let origin_x = area.x + (area.width.saturating_sub(block_w)) / 2;

    let grid_h = area
        .height
        .saturating_sub(TITLE_HEIGHT + GAP_Y + (rows - 1) * GAP_Y);
    let card_h = (grid_h / rows).clamp(3, CARD_HEIGHT);
    let block_h = TITLE_HEIGHT + GAP_Y + card_h * rows + (rows - 1) * GAP_Y;
    let origin_y = area.y + (area.height.saturating_sub(block_h)) / 2;

    draw_title_box(
        frame,
        Rect {
            x: origin_x,
            y: origin_y,
            width: block_w,
            height: TITLE_HEIGHT,
        },
        title,
        cadence,
    );

    let grid_top = origin_y + TITLE_HEIGHT + GAP_Y;
    for (index, stat) in stats.iter().enumerate() {
        let col = index as u16 % cols;
        let row = index as u16 / cols;
        draw_stat_card(
            frame,
            Rect {
                x: origin_x + col * (card_w + GAP_X),
                y: grid_top + row * (card_h + GAP_Y),
                width: card_w,
                height: card_h,
            },
            stat,
        );
    }
}

/// A full-width bordered box holding the journal name, its date span, and the
/// headline totals, centred over the cards below.
fn draw_title_box(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    cadence: &journal_analytics::Cadence,
) {
    let mut spans = vec![Span::styled(title.to_string(), theme().heading())];
    if let Some(span) = date_span(cadence.date_span) {
        spans.push(Span::styled(format!(" · {span}"), theme().muted()));
    }
    spans.push(Span::styled(
        format!(
            " · {} · {}",
            count_label(cadence.total_entries, "entry", "entries"),
            count_label(cadence.total_words, "word", "words"),
        ),
        theme().muted(),
    ));
    // A leading blank line centres the text on the middle of the three inner rows.
    frame.render_widget(
        Paragraph::new(vec![Line::default(), Line::from(spans)])
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(theme().card_border()),
            ),
        area,
    );
}

/// The six headline figures, paired so the grid reads as lift/drain, best/worst
/// day, and recent feeling / how many days you showed up. Chosen to point at what
/// moves your mood rather than to judge — the "drains" and "toughest" cards name
/// what to watch, not to blame.
fn metrics(analytics: &Analytics) -> Vec<Stat> {
    vec![
        lifts_stat(analytics),
        drains_stat(analytics),
        happiest_day_stat(&analytics.mood),
        toughest_day_stat(&analytics.mood),
        top_feeling_stat(analytics),
        Stat::new("Active days", analytics.cadence.active_days.to_string()),
    ]
}

/// The people and things linked to your better moods (rotated daily): a person on
/// the value line, an activity or tag beneath it. Falls back to whichever exists.
fn lifts_stat(analytics: &Analytics) -> Stat {
    match (
        &analytics.highlights.lifts_person,
        &analytics.highlights.lifts_thing,
    ) {
        (Some(person), Some(thing)) => {
            Stat::new("Lifts you", person.clone()).sub(Span::styled(thing.clone(), theme().muted()))
        }
        (Some(name), None) | (None, Some(name)) => Stat::new("Lifts you", name.clone()),
        (None, None) => Stat::new("Lifts you", "—"),
    }
}

/// The mirror of [`lifts_stat`]: the people and things linked to your worse moods.
fn drains_stat(analytics: &Analytics) -> Stat {
    match (
        &analytics.highlights.drains_person,
        &analytics.highlights.drains_thing,
    ) {
        (Some(person), Some(thing)) => Stat::new("Drains you", person.clone())
            .sub(Span::styled(thing.clone(), theme().muted())),
        (Some(name), None) | (None, Some(name)) => Stat::new("Drains you", name.clone()),
        (None, None) => Stat::new("Drains you", "—"),
    }
}

/// This year's most-logged feeling, noted as such; falls back to the all-time top
/// feeling (noted) when this year has none yet.
fn top_feeling_stat(analytics: &Analytics) -> Stat {
    if let Some(name) = &analytics.highlights.top_feeling_this_year {
        Stat::new("Top feeling", name.clone()).sub(Span::styled("this year", theme().muted()))
    } else if let Some(tally) = analytics.mood.feelings.first() {
        Stat::new("Top feeling", tally.name.clone()).sub(Span::styled("all time", theme().muted()))
    } else {
        Stat::new("Top feeling", "—")
    }
}

/// The weekday whose entries average the highest mood.
fn happiest_day_stat(mood: &MoodAnalytics) -> Stat {
    match extreme_weekday(mood, true) {
        Some(day) => Stat::new("Happiest day", WEEKDAYS[day].to_string()),
        None => Stat::new("Happiest day", "—"),
    }
}

/// The mirror of [`happiest_day_stat`]: the weekday whose entries average the
/// lowest mood — the day worth a little extra care.
fn toughest_day_stat(mood: &MoodAnalytics) -> Stat {
    match extreme_weekday(mood, false) {
        Some(day) => Stat::new("Toughest day", WEEKDAYS[day].to_string()),
        None => Stat::new("Toughest day", "—"),
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
fn date_span(span: Option<(chrono::NaiveDate, chrono::NaiveDate)>) -> Option<String> {
    let (first, last) = span?;
    use chrono::Datelike;
    if first.year() == last.year() {
        Some(first.year().to_string())
    } else {
        Some(format!("{} – {}", first.year(), last.year()))
    }
}
