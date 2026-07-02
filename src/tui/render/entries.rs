use ratatui::{Frame, layout::Rect, widgets::List};

use crate::tui::{
    app::{App, Focus, Mode},
    entry_rows::{entry_list_rows, visible_entry_items},
    render::{clamp_scroll, entry_row_metadata, panel_block, panel_inner, total_entry_row_height},
};

pub(crate) fn draw_entry_list(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let focused = app.focus == Focus::Entries;
    let title = match app.mode {
        Mode::Search => "Search",
        Mode::Browse => "Entries",
    };
    let rows = entry_list_rows(app);
    let viewport_height = panel_inner(area).height;
    app.entry_scroll = clamp_scroll(
        app.entry_scroll,
        total_entry_row_height(&entry_row_metadata(app)),
        viewport_height,
    );
    let items = visible_entry_items(&rows, app.entry_scroll, viewport_height);

    let list = List::new(items).block(panel_block(title, focused));
    frame.render_widget(list, area);
}
