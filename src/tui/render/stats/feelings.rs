//! The Feelings tab: the positive/neutral/negative balance across rolling
//! windows above a scrollable table of every feeling — its frequency, mood
//! association, and the feelings it most often shows up together with (the last
//! column). Balance sits fixed on top; the table fills the rest and scrolls.

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};

use journal_analytics::{Analytics, Sentiment};

use super::correlate::{self, StatsListMetrics};
use super::widgets::{Section, heading, sentiment_segments, stack};
use crate::tui::render::render_centered_notice;
use crate::tui::theme::theme;

/// Balance rows, newest window last: all-time, then the trailing year / month /
/// week from `MoodAnalytics::sentiment_windows`.
const BALANCE_LABELS: [&str; 4] = ["All", "Year", "Month", "Week"];

pub(super) fn draw(
    frame: &mut Frame<'_>,
    area: Rect,
    analytics: &Analytics,
    scroll: &mut u16,
) -> StatsListMetrics {
    let mood = &analytics.mood;
    if mood.feelings.is_empty() {
        *scroll = 0;
        render_centered_notice(frame, area, "No feelings logged yet");
        return StatsListMetrics { total: 0, viewport: 0 };
    }

    let sections = stack(
        area,
        &[
            Section::new(6, 0), // balance: heading + blank + 4 window rows
            Section::new(5, 3), // feelings table (heading + blank + scrolling table)
        ],
    );

    if let Some(area) = sections[0] {
        let body = heading(frame, area, "Balance");
        draw_balance(frame, body, &mood.sentiment, &mood.sentiment_windows);
    }

    match sections[1] {
        Some(area) => {
            let body = heading(frame, area, "Feelings");
            // The trailing column here is which feelings co-occur with each row's feeling.
            correlate::draw(frame, body, &analytics.correlations.feelings, "—", "Together", scroll)
        }
        None => {
            *scroll = 0;
            StatsListMetrics { total: 0, viewport: 0 }
        }
    }
}

/// One sentiment bar per window, `label ▓▓▓···`, so the trend across all-time /
/// year / month / week reads down the column. Empty windows show a dim track.
fn draw_balance(frame: &mut Frame<'_>, area: Rect, all: &Sentiment, windows: &[Sentiment; 3]) {
    const LABEL_W: usize = 5;
    let rows = [all, &windows[0], &windows[1], &windows[2]];
    let seg_width = (area.width as usize).saturating_sub(LABEL_W + 1);
    for (index, (label, sentiment)) in BALANCE_LABELS.iter().zip(rows).enumerate() {
        if index as u16 >= area.height {
            break;
        }
        let row = Rect {
            y: area.y + index as u16,
            height: 1,
            ..area
        };
        let mut spans = vec![Span::styled(format!("{label:<LABEL_W$} "), theme().muted())];
        spans.extend(
            sentiment_segments(
                sentiment.positive,
                sentiment.neutral,
                sentiment.negative,
                seg_width,
            )
            .spans,
        );
        frame.render_widget(Paragraph::new(Line::from(spans)), row);
    }
}
