use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{List, ListItem},
};

use crate::tui::{
    app::{App, Focus},
    render::{clamp_scroll, panel_block, panel_inner, selected_style},
};

pub(crate) fn draw_journals(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let focused = app.focus == Focus::Journals;
    let viewport_height = panel_inner(area).height;
    app.scroll.journal = clamp_scroll(app.scroll.journal, app.journals.len(), viewport_height);
    let offset = app.scroll.journal as usize;
    let items: Vec<ListItem> = app
        .journals
        .iter()
        .enumerate()
        .skip(offset)
        .take(viewport_height as usize)
        .map(|(index, journal)| {
            let style = selected_style(index == app.selected_journal);
            ListItem::new(Line::from(Span::raw(&journal.name))).style(style)
        })
        .collect();

    let list = List::new(items).block(panel_block("Journals", focused));
    frame.render_widget(list, area);
}
