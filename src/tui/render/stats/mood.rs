//! The Mood tab: the mood series over time and the temporal breakdowns that
//! point at *when* mood dips — best/worst period, by weekday, by month.

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};

use journal_analytics::Analytics;

use super::widgets::{Bar, Section, Stat, draw_bars, draw_stats, heading, stack};
use super::signed;
use crate::tui::render::render_centered_notice;
use crate::tui::theme::theme;

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

pub(super) fn draw(frame: &mut Frame<'_>, area: Rect, analytics: &Analytics) {
    let mood = &analytics.mood;
    if mood.mean.is_none() {
        render_centered_notice(frame, area, "No mood logged yet");
        return;
    }

    let sections = stack(
        area,
        &[
            Section::new(4, 2), // mood over time (heading + blank + bars)
            Section::new(4, 0), // best / worst (cards, no heading)
            Section::new(4, 1), // by weekday
            Section::new(4, 1), // by month
        ],
    );

    if let Some(area) = sections[0] {
        let body = heading(frame, area, "Mood over time");
        let bars: Vec<Bar> = mood
            .series
            .iter()
            .map(|bucket| Bar {
                label: bucket.label.clone(),
                fill: (bucket.avg + 5.0) / 10.0,
                value: signed(bucket.avg),
                style: theme().signed(bucket.avg),
            })
            .collect();
        draw_bars(frame, body, &bars);
    }

    if let Some(cards) = sections[1] {
        let best = period_stat("Best", mood.best_period.as_ref());
        let worst = period_stat("Worst", mood.worst_period.as_ref());
        draw_stats(frame, cards, &[best, worst]);
    }

    if let Some(area) = sections[2] {
        let body = heading(frame, area, "By weekday");
        draw_avg_bars(frame, body, &mood.by_weekday, &WEEKDAYS);
    }

    if let Some(area) = sections[3] {
        let body = heading(frame, area, "By month");
        draw_avg_bars(frame, body, &mood.by_month, &MONTHS);
    }
}

fn period_stat(label: &str, bucket: Option<&journal_analytics::MoodBucket>) -> Stat {
    match bucket {
        Some(bucket) => Stat::new(label, format!("{} {}", bucket.label, signed(bucket.avg)))
            .styled(theme().signed(bucket.avg)),
        None => Stat::new(label, "—"),
    }
}

/// Horizontal signed bars of per-bucket average mood, skipping empty buckets.
/// A signed value maps `-5..+5` onto the bar fill with green/red by sign, so the
/// direction reads without colour.
fn draw_avg_bars(frame: &mut Frame<'_>, area: Rect, values: &[Option<f32>], labels: &[&str]) {
    let bars: Vec<Bar> = values
        .iter()
        .zip(labels)
        .filter_map(|(value, label)| {
            value.map(|avg| Bar {
                label: (*label).to_string(),
                fill: (avg + 5.0) / 10.0,
                value: signed(avg),
                style: theme().signed(avg),
            })
        })
        .collect();
    if bars.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("—", theme().muted()))),
            area,
        );
        return;
    }
    draw_bars(frame, area, &bars);
}
