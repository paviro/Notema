use ratatui::{
    Frame,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use super::centered_rect;

pub(super) fn draw_confirm_delete(frame: &mut Frame<'_>) {
    let area = centered_rect(50, 20, frame.area());
    frame.render_widget(Clear, area);
    let dialog = Paragraph::new("Move selected file to trash? y/n")
        .block(
            Block::default()
                .title("Confirm Delete")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(dialog, area);
}

pub(super) fn draw_new_journal_input(frame: &mut Frame<'_>, input: &str) {
    let area = centered_rect(60, 20, frame.area());
    frame.render_widget(Clear, area);
    let dialog = Paragraph::new(format!("Name: {input}\n\nEnter saves | Esc cancels"))
        .block(Block::default().title("New Journal").borders(Borders::ALL))
        .wrap(Wrap { trim: true });
    frame.render_widget(dialog, area);
}
