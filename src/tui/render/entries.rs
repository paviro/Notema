use ratatui::{Frame, widgets::List};

use crate::tui::{
    app::{App, Focus, Mode},
    entry_rows::{entry_list_rows, visible_entry_items},
    render::{
        EntryListGeometry, clamp_scroll, entry_row_metadata, panel_block,
        render_scrollbar_if_needed, total_entry_row_height,
    },
};

pub(crate) fn draw_entry_list(frame: &mut Frame<'_>, geometry: EntryListGeometry, app: &mut App) {
    let focused = app.focus == Focus::Entries;
    let block = panel_block(
        match app.mode {
            Mode::Search => "Search",
            Mode::Browse => "Entries",
        },
        focused,
        None,
    );
    let text_width = geometry.text_width;
    let rows = entry_list_rows(app, text_width);
    let viewport_height = geometry.viewport_height;
    let meta = entry_row_metadata(app, text_width);
    let total_height = total_entry_row_height(&meta);
    app.scroll.entry = clamp_scroll(app.scroll.entry, total_height, viewport_height);
    let items = visible_entry_items(&rows, app.scroll.entry, viewport_height);

    frame.render_widget(block, geometry.panel.area);
    frame.render_widget(List::new(items), geometry.panel.content);
    render_scrollbar_if_needed(
        frame,
        geometry.panel.area,
        total_height,
        viewport_height,
        app.scroll.entry,
    );
}
