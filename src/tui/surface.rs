use ratatui::layout::{Constraint, Direction, Layout, Rect};
use unicode_width::UnicodeWidthStr;

pub(crate) const ENTRY_TIME_GUTTER_WIDTH: u16 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PanelGeometry {
    pub(crate) area: Rect,
    pub(crate) content: Rect,
}

impl PanelGeometry {
    pub(crate) fn new(area: Rect) -> Self {
        let inner = panel_inner(area);
        let content = panel_content_inner(inner);
        Self { area, content }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EntryListGeometry {
    pub(crate) panel: PanelGeometry,
    pub(crate) text_width: u16,
    pub(crate) viewport_height: u16,
}

impl EntryListGeometry {
    pub(crate) fn new(area: Rect) -> Self {
        let panel = PanelGeometry::new(area);
        Self {
            text_width: panel.content.width.saturating_sub(ENTRY_TIME_GUTTER_WIDTH),
            viewport_height: panel.content.height,
            panel,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MetadataRowLayout {
    pub(crate) rect: Rect,
    pub(crate) prefix_width: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EntryMetadataLayout {
    pub(crate) content: Rect,
    pub(crate) metadata: Option<Rect>,
    pub(crate) mood: Option<Rect>,
    pub(crate) feelings: Option<MetadataRowLayout>,
    pub(crate) tags: Option<MetadataRowLayout>,
}

pub(crate) fn panel_inner(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

pub(crate) fn panel_content_inner(area: Rect) -> Rect {
    let pad = 1;
    Rect {
        x: area.x.saturating_add(pad),
        width: area.width.saturating_sub(pad * 2).max(1),
        ..area
    }
}

pub(crate) fn point_in_rect(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x
        && x < area.x.saturating_add(area.width)
        && y >= area.y
        && y < area.y.saturating_add(area.height)
}

pub(crate) fn entry_metadata_layout(
    entry_view_area: Rect,
    has_tags: bool,
    has_feelings: bool,
    has_mood: bool,
) -> EntryMetadataLayout {
    let inner = PanelGeometry::new(entry_view_area).content;
    let metadata_height = metadata_section_height(has_tags, has_feelings, has_mood);
    let show_metadata = metadata_height > 0 && inner.height > metadata_height;

    let (content, metadata) = if show_metadata {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(metadata_height)])
            .split(inner);
        (chunks[0], Some(chunks[1]))
    } else {
        (inner, None)
    };

    let mut mood = None;
    let mut feelings = None;
    let mut tags = None;

    if let Some(metadata_rect) = metadata {
        let mut y = metadata_rect.y.saturating_add(1);
        if has_mood {
            mood = Some(Rect {
                y,
                height: 1,
                ..metadata_rect
            });
            y = y.saturating_add(1);
        }
        if has_feelings {
            feelings = Some(MetadataRowLayout {
                rect: Rect {
                    y,
                    height: 1,
                    ..metadata_rect
                },
                prefix_width: "Feelings: ".len() as u16,
            });
            y = y.saturating_add(1);
        }
        if has_tags {
            tags = Some(MetadataRowLayout {
                rect: Rect {
                    y,
                    height: 1,
                    ..metadata_rect
                },
                prefix_width: "Tags: ".len() as u16,
            });
        }
    }

    EntryMetadataLayout {
        content,
        metadata,
        mood,
        feelings,
        tags,
    }
}

pub(crate) fn metadata_item_at(
    row: MetadataRowLayout,
    x: u16,
    y: u16,
    values: &[String],
) -> Option<String> {
    if y != row.rect.y || values.is_empty() {
        return None;
    }

    let mut x_pos = row.rect.x.saturating_add(row.prefix_width);
    if x < x_pos {
        return None;
    }

    for value in values {
        let width = UnicodeWidthStr::width(value.as_str()).min(u16::MAX as usize) as u16;
        if x >= x_pos && x < x_pos.saturating_add(width) {
            return Some(value.clone());
        }
        x_pos = x_pos.saturating_add(width).saturating_add(3);
    }

    None
}

fn metadata_section_height(has_tags: bool, has_feelings: bool, has_mood: bool) -> u16 {
    let rows = has_mood as u16 + has_feelings as u16 + has_tags as u16;
    if rows == 0 { 0 } else { 1 + rows }
}
