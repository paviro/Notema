use ratatui::layout::{Constraint, Direction, Layout, Rect};

use super::entry_rows::EntryRowMeta;
use super::render::{TAGS_SECTION_HEIGHT, panel_content_inner};

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

pub(crate) fn tag_at_point(
    entry_view_area: Rect,
    x: u16,
    y: u16,
    tags: &[String],
) -> Option<String> {
    if tags.is_empty() {
        return None;
    }

    let inner = panel_content_inner(panel_inner(entry_view_area));
    if inner.height <= TAGS_SECTION_HEIGHT {
        return None;
    }

    let tags_rect = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(TAGS_SECTION_HEIGHT)])
        .split(inner)[1];

    let tags_text_y = tags_rect.y + 1;
    if y != tags_text_y {
        return None;
    }

    let mut x_pos = tags_rect.x;
    let prefix = "Tags: ";
    if x < x_pos + prefix.len() as u16 {
        return None;
    }
    x_pos += prefix.len() as u16;

    for tag in tags {
        let tag_width = tag.len() as u16;
        if x >= x_pos && x < x_pos + tag_width {
            return Some(tag.clone());
        }
        x_pos += tag_width + 3;
    }

    None
}
