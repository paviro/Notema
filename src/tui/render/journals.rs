use ratatui::{
    Frame,
    text::{Line, Span},
    widgets::{List, ListItem},
};

use crate::tui::{
    app::{App, Focus},
    render::{
        PanelGeometry, clamp_scroll, panel_block, render_scrollbar_if_needed, selected_style,
    },
};

pub(crate) fn draw_journals(frame: &mut Frame<'_>, geometry: PanelGeometry, app: &mut App) {
    let focused = app.focus == Focus::Journals;
    let block = panel_block("Journals", focused, None);
    let viewport_height = geometry.content.height;
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

    frame.render_widget(block, geometry.area);
    frame.render_widget(List::new(items), geometry.content);
    render_scrollbar_if_needed(
        frame,
        geometry.area,
        app.journals.len(),
        viewport_height,
        app.scroll.journal,
    );
}
