use ratatui::layout::Rect;

use super::entry_rows::EntryRowMeta;

pub(crate) fn panel_inner(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

pub(crate) fn point_in_rect(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x
        && x < area.x.saturating_add(area.width)
        && y >= area.y
        && y < area.y.saturating_add(area.height)
}

pub(crate) fn journal_index_at(
    area: Rect,
    x: u16,
    y: u16,
    scroll: u16,
    journal_count: usize,
) -> Option<usize> {
    let inner = panel_inner(area);
    if !point_in_rect(inner, x, y) {
        return None;
    }

    let index = scroll as usize + y.saturating_sub(inner.y) as usize;
    (index < journal_count).then_some(index)
}

pub(crate) fn entry_index_at(
    area: Rect,
    x: u16,
    y: u16,
    scroll: u16,
    rows: &[EntryRowMeta],
) -> Option<usize> {
    let inner = panel_inner(area);
    if !point_in_rect(inner, x, y) {
        return None;
    }

    let target_y = scroll as usize + y.saturating_sub(inner.y) as usize;
    let mut row_y = 0usize;
    for row in rows {
        let next_y = row_y.saturating_add(row.height as usize);
        if target_y < next_y {
            return row.entry_index;
        }
        row_y = next_y;
    }
    None
}
