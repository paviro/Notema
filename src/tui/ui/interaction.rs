use ratatui::layout::Rect;

use crate::tui::app::ScrollbarDrag;

/// A pane scrollbar's geometry and content metrics, captured at render time so
/// the mouse handler can map presses and drags on the bar back to a scroll
/// offset without reconstructing the layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScrollbarMetrics {
    pub(crate) which: ScrollbarDrag,
    /// The full bar column (`scrollbar_bar_rect`); its first/last rows are the arrows.
    pub(crate) bar: Rect,
    pub(crate) max_scroll: usize,
    pub(crate) content_length: usize,
    pub(crate) viewport: u16,
    /// Current scrollbar position, for locating the thumb.
    pub(crate) position: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PanelId {
    Journals,
    Entries,
    Reader,
    Insights,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TextFieldId {
    Search,
    NewJournal,
    Metadata,
    Feelings,
    LocationQuery,
    LocationName,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DialogId {
    Settings,
    MetadataMenu,
    EditorMetadataMenu,
    ThemePicker,
    Metadata,
    Feelings,
    Location,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConfirmId {
    Delete,
    EditorDiscard,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DialogInputId {
    Metadata,
    Feelings,
    LocationQuery,
    LocationName,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum InteractionKind {
    Panel(PanelId),
    Row {
        panel: PanelId,
        index: usize,
    },
    TextField(TextFieldId),
    Hint(crate::tui::render::HintId),
    DialogList {
        dialog: DialogId,
        viewport: u16,
    },
    DialogRow {
        dialog: DialogId,
        index: usize,
    },
    DialogInput(DialogInputId),
    DialogClose(DialogId),
    ConfirmButton {
        confirm: ConfirmId,
        destructive: bool,
    },
    MoodBar(Rect),
    Link {
        target: crate::tui::app::ReaderLinkTarget,
        heading_line: Option<usize>,
    },
    Scrollbar(ScrollbarMetrics),
    Overlay,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Region {
    area: Rect,
    kind: InteractionKind,
}

#[derive(Default)]
pub(crate) struct InteractionMap {
    regions: Vec<Region>,
}

impl InteractionMap {
    pub(crate) fn clear(&mut self) {
        self.regions.clear();
    }

    pub(crate) fn push(&mut self, area: Rect, kind: InteractionKind) {
        if area.width > 0 && area.height > 0 {
            self.regions.push(Region { area, kind });
        }
    }

    pub(crate) fn hit(&self, column: u16, row: u16) -> Option<&InteractionKind> {
        self.regions
            .iter()
            .rev()
            .find(|region| {
                column >= region.area.x
                    && column < region.area.x.saturating_add(region.area.width)
                    && row >= region.area.y
                    && row < region.area.y.saturating_add(region.area.height)
            })
            .map(|region| &region.kind)
    }

    pub(crate) fn area_for_text_field(&self, id: TextFieldId) -> Option<Rect> {
        self.regions.iter().rev().find_map(|region| {
            (region.kind == InteractionKind::TextField(id)).then_some(region.area)
        })
    }

    /// The registered scrollbar metrics for `which`, so an ongoing drag can map
    /// rows against the frame on screen without a cursor hit (the cursor may
    /// drift off the narrow bar mid-drag).
    pub(crate) fn scrollbar(&self, which: ScrollbarDrag) -> Option<ScrollbarMetrics> {
        self.regions
            .iter()
            .rev()
            .find_map(|region| match region.kind {
                InteractionKind::Scrollbar(metrics) if metrics.which == which => Some(metrics),
                _ => None,
            })
    }

    pub(crate) fn dialog_list_viewport(&self, dialog: DialogId) -> Option<u16> {
        self.regions
            .iter()
            .rev()
            .find_map(|region| match region.kind {
                InteractionKind::DialogList {
                    dialog: candidate,
                    viewport,
                } if candidate == dialog => Some(viewport),
                _ => None,
            })
    }
}

#[derive(Default)]
pub(crate) struct ViewState {
    pub(crate) interactions: InteractionMap,
    pub(crate) layout: Option<crate::tui::render::TuiLayout>,
    pub(crate) reader: crate::tui::app::ReaderHits,
    pub(crate) insights: crate::tui::app::InsightsScrollGeometry,
    pub(crate) journal_offset: Option<usize>,
    pub(crate) entry_offset: Option<usize>,
}

impl ViewState {
    pub(crate) fn begin_frame(&mut self) {
        self.interactions.clear();
        self.layout = None;
        self.reader = crate::tui::app::ReaderHits::default();
        self.insights = crate::tui::app::InsightsScrollGeometry::default();
        self.journal_offset = None;
        self.entry_offset = None;
    }

    pub(crate) fn reader_link_hit_at(&self, col: u16, row: u16) -> Option<(usize, usize, usize)> {
        let line = self.reader_body_line_at(col, row)?;
        let column = (col - self.reader.content_rect.x) as usize;
        self.reader
            .links
            .iter()
            .find(|hit| hit.line == line && column >= hit.start && column < hit.end)
            .map(|hit| (hit.line, hit.start, hit.end))
    }

    fn reader_body_line_at(&self, col: u16, row: u16) -> Option<usize> {
        let rect = self.reader.content_rect;
        (rect.width > 0
            && rect.height > 0
            && col >= rect.x
            && col < rect.x.saturating_add(rect.width)
            && row >= rect.y
            && row < rect.y.saturating_add(rect.height))
        .then(|| self.reader.scroll as usize + (row - rect.y) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_regions_win_at_the_same_point() {
        let mut map = InteractionMap::default();
        let area = Rect::new(1, 1, 4, 4);
        map.push(area, InteractionKind::Panel(PanelId::Reader));
        map.push(area, InteractionKind::Overlay);

        assert_eq!(map.hit(2, 2), Some(&InteractionKind::Overlay));
    }

    #[test]
    fn begin_frame_clears_stale_regions() {
        let mut view = ViewState::default();
        view.interactions.push(
            Rect::new(0, 0, 3, 3),
            InteractionKind::Panel(PanelId::Entries),
        );

        view.begin_frame();

        assert_eq!(view.interactions.hit(1, 1), None);
    }

    /// A link hit answers only inside its column span — hovering the rest of
    /// the row must not light it up (regression: image labels used to hit the
    /// whole row).
    #[test]
    fn link_hits_are_bounded_by_their_column_span() {
        let view = ViewState {
            reader: crate::tui::app::ReaderHits {
                content_rect: Rect::new(10, 5, 40, 10),
                scroll: 0,
                line_count: 3,
                links: vec![crate::tui::app::ReaderLinkHit {
                    line: 2,
                    start: 0,
                    end: 8,
                    target: crate::tui::app::ReaderLinkTarget::Image(0),
                    group: 0,
                }],
                headings: Vec::new(),
            },
            ..ViewState::default()
        };

        let row = 5 + 2;
        assert_eq!(view.reader_link_hit_at(10, row), Some((2, 0, 8)));
        assert_eq!(view.reader_link_hit_at(17, row), Some((2, 0, 8)));
        assert_eq!(view.reader_link_hit_at(18, row), None, "past the label");
        assert_eq!(
            view.reader_link_hit_at(30, row),
            None,
            "same row, far right"
        );
        assert_eq!(view.reader_link_hit_at(10, row + 1), None, "other row");
    }
}
