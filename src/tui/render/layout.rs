use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::config::BodyLayout;
use crate::tui::app::{
    AppModel, ENTRY_LIST_INLINE_WIDTH, ENTRY_LIST_MIN_WIDTH, Focus, JOURNAL_LIST_WIDTH, Mode,
    inline_reader_is_visible, single_panel_is_active,
};
use crate::tui::surface::{
    EntryListGeometry, EntryMetadataLayout, EntryMetadataValues, PanelGeometry,
    entry_metadata_layout, metadata_section_height,
};
use crate::tui::theme::Theme;

use super::footer::footer_height;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TuiLayout {
    pub(crate) content: Rect,
    pub(crate) footer: Rect,
    pub(crate) journals: Option<PanelGeometry>,
    pub(crate) entries: Option<EntryListGeometry>,
    pub(crate) reader: Option<PanelGeometry>,
    pub(crate) insights: Option<PanelGeometry>,
    pub(crate) single_panel: bool,
}

/// The blank row the entry body always starts with, one line below the border,
/// matching the blank that leads the journal and entry columns. The viewer bakes
/// it into its rendered lines; the editor carries it as textarea top padding.
pub(crate) const BODY_LEADING_BLANK: u16 = 1;

/// Columns the body always keeps clear on each side — the horizontal counterpart
/// of [`BODY_LEADING_BLANK`], and the floor both the `max_width` gutter and the
/// top-padding ramp measure from.
pub(crate) const BODY_MIN_SIDE_INSET: u16 = 2;

/// Blank lines to prepend above the body, ramping in at one line per two gutter
/// columns (cells run ~2× taller than wide) and staying at 0 until the pane is
/// wider than `max_width` plus the side insets.
pub(crate) fn body_top_pad_lines(content_width: u16, max_width: u16, setting: u16) -> u16 {
    let inner = content_width.saturating_sub(BODY_MIN_SIDE_INSET * 2);
    let gutter = if max_width > 0 && inner > max_width {
        (inner - max_width) / 2
    } else {
        0
    };
    setting.min(gutter / 2)
}

/// Inset `rect` by [`BODY_MIN_SIDE_INSET`] a side, then cap it at `max_width` and
/// center it horizontally. The height is untouched, and a `max_width` of 0 means
/// no cap — leaving just the inset. Panes too narrow to spare the inset keep it.
pub(crate) fn centered_body_rect(rect: Rect, max_width: u16) -> Rect {
    let inset = if rect.width > BODY_MIN_SIDE_INSET * 2 {
        BODY_MIN_SIDE_INSET
    } else {
        0
    };
    let rect = Rect {
        x: rect.x + inset,
        width: rect.width - inset * 2,
        ..rect
    };
    if max_width == 0 || rect.width <= max_width {
        return rect;
    }
    Rect {
        x: rect.x + (rect.width - max_width) / 2,
        width: max_width,
        ..rect
    }
}

/// Rows the body keeps even after the metadata claims its share; below this the
/// metadata gives up its pinned slot instead of squeezing the body further.
const MIN_ENTRY_BODY_LINES: u16 = 20;

/// Pin the metadata below the body only when doing so still leaves the body at
/// least [`MIN_ENTRY_BODY_LINES`]; otherwise fold it into the scroll. With no
/// metadata the height is zero and this reduces to a plain minimum-body check.
pub(crate) fn metadata_scrolls_with_body(
    theme: &Theme,
    area: Rect,
    values: EntryMetadataValues<'_>,
) -> bool {
    let inner = PanelGeometry::new(theme, area).content;
    let metadata_height = metadata_section_height(inner.width, values);
    inner.height < MIN_ENTRY_BODY_LINES.saturating_add(metadata_height)
}

/// How an entry pane frames its body: where the text goes, how much blank leads
/// it, and whether the metadata gets a pinned slot below. The viewer and the
/// editor both build one so the two can't frame the same entry differently.
///
/// Two stages, because the wrapped line count is only knowable once the width is
/// fixed: [`Self::new`] settles the rect, then [`Self::centers`] and
/// [`Self::centered`] take the count.
pub(crate) struct EntryBodyFrame {
    /// `None` while the metadata scrolls with the body instead of pinning below it.
    pub(crate) metadata: Option<EntryMetadataLayout>,
    /// The rect the body renders into, already inset, capped and centered. Also
    /// the width it must wrap to — the two must not be computed separately.
    pub(crate) body: Rect,
    ramp: u16,
    center_vertically: bool,
}

impl EntryBodyFrame {
    pub(crate) fn new(
        theme: &Theme,
        area: Rect,
        values: EntryMetadataValues<'_>,
        body_layout: BodyLayout,
    ) -> Self {
        let scrolls = metadata_scrolls_with_body(theme, area, values);
        let (content, metadata) = if scrolls {
            (PanelGeometry::new(theme, area).content, None)
        } else {
            let layout = entry_metadata_layout(theme, area, values);
            (layout.content, Some(layout))
        };
        // Scrolling metadata shares the body's paragraph, so it renders uncapped
        // (the side inset still applies); otherwise the body is guttered to a
        // readable measure.
        let max_width = if scrolls { 0 } else { body_layout.max_width };
        let body = centered_body_rect(content, max_width);
        Self {
            metadata,
            // One spare column keeps wrapped text off the pane's last cell.
            body: Rect {
                width: body.width.saturating_sub(1).max(1),
                ..body
            },
            ramp: if scrolls {
                0
            } else {
                body_top_pad_lines(content.width, max_width, body_layout.max_top_padding)
            },
            center_vertically: body_layout.center_vertically && !scrolls,
        }
    }

    pub(crate) fn metadata_scrolls(&self) -> bool {
        self.metadata.is_none()
    }

    /// Blank rows centering asks for above `line_count` rendered rows: half the
    /// slack, and 0 once the body no longer fits.
    fn center_pad(&self, line_count: usize) -> u16 {
        if !self.center_vertically {
            return 0;
        }
        ((self.body.height as usize).saturating_sub(line_count) / 2) as u16
    }

    /// Whether `line_count` rendered rows float in the middle: only once that
    /// sits lower than the ramp, which is a floor the body never rises above.
    pub(crate) fn centers(&self, line_count: usize) -> bool {
        self.center_pad(line_count) > self.ramp
    }

    /// Blank rows above the body, on top of [`BODY_LEADING_BLANK`]. Takes the
    /// count because the larger of the ramp and the centering offset wins —
    /// they never stack.
    pub(crate) fn top_pad(&self, line_count: usize) -> u16 {
        if self.centers(line_count) {
            return 0;
        }
        self.ramp
    }

    /// The body rect for `line_count` rendered rows: pushed down to sit in the
    /// vertical middle when it floats, otherwise unchanged so scrolling covers
    /// every line. Gated on [`Self::centers`] alone, so it can't disagree with
    /// the [`Self::top_pad`] that was chosen for the same count.
    ///
    /// The viewer measures the two at counts a ramp apart (it prepends between
    /// the calls); they still agree because `center_pad` only shrinks as the
    /// count grows, so a ramp that won at the smaller count wins at the larger.
    pub(crate) fn centered(&self, line_count: usize) -> Rect {
        if !self.centers(line_count) {
            return self.body;
        }
        let pad = self.center_pad(line_count);
        Rect {
            y: self.body.y + pad,
            height: self.body.height - pad,
            ..self.body
        }
    }
}

pub(crate) fn tui_layout(area: Rect, app: &AppModel) -> TuiLayout {
    let footer_height = footer_height(app, area.width).min(area.height);
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(footer_height)])
        .split(area);
    let content = root[0];
    let footer = root[1];
    let inline_reader_visible = inline_reader_is_visible(content.width);
    let single_panel = single_panel_is_active(content.width);
    let show_journals = app.state.ui.show_journals;

    let mut layout = TuiLayout {
        content,
        footer,
        journals: None,
        entries: None,
        reader: None,
        insights: None,
        single_panel,
    };

    // A full-screen viewer owns the whole content area at any width, so mouse
    // hit-testing lines up with what `draw` paints.
    if app.reader_is_fullscreen(content.width) {
        layout.reader = Some(PanelGeometry::new(&app.appearance.theme, content));
        return layout;
    }

    // Likewise for an expanded insights panel: hand it the whole content area and
    // let its responsive renderer pick a larger, multi-column layout from the
    // bigger `Rect` — no fullscreen flag reaches the render code.
    if app.insights_is_fullscreen(content.width) {
        layout.insights = Some(PanelGeometry::new(&app.appearance.theme, content));
        return layout;
    }

    if single_panel {
        match app.nav.focus {
            Focus::Journals if app.nav.mode == Mode::Browse && show_journals => {
                layout.journals = Some(PanelGeometry::new(&app.appearance.theme, content))
            }
            Focus::Reader => {
                layout.reader = Some(PanelGeometry::new(&app.appearance.theme, content))
            }
            // Reached by pressing Right from the entries column (or stranded here by a
            // resize) — show the panel full-width, the only pane at this width.
            Focus::Insights => {
                layout.insights = Some(PanelGeometry::new(&app.appearance.theme, content))
            }
            Focus::Journals | Focus::Entries => {
                layout.entries = Some(EntryListGeometry::new(&app.appearance.theme, content))
            }
        }
        return layout;
    }

    if inline_reader_visible {
        if show_journals {
            let body = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(JOURNAL_LIST_WIDTH),
                    Constraint::Length(ENTRY_LIST_INLINE_WIDTH),
                    Constraint::Min(ENTRY_LIST_MIN_WIDTH),
                ])
                .split(content);
            layout.journals = Some(PanelGeometry::new(&app.appearance.theme, body[0]));
            layout.entries = Some(EntryListGeometry::new(&app.appearance.theme, body[1]));
            // The right column is the insights panel whenever no entry is
            // shown (Journals/Entries/Insights focus with nothing selected), and
            // the reader once an entry is selected.
            if app.show_journal_insights() {
                layout.insights = Some(PanelGeometry::new(&app.appearance.theme, body[2]));
            } else {
                layout.reader = Some(PanelGeometry::new(&app.appearance.theme, body[2]));
            }
        } else {
            let body = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(ENTRY_LIST_INLINE_WIDTH),
                    Constraint::Min(ENTRY_LIST_MIN_WIDTH),
                ])
                .split(content);
            layout.entries = Some(EntryListGeometry::new(&app.appearance.theme, body[0]));
            layout.reader = Some(PanelGeometry::new(&app.appearance.theme, body[1]));
        }
    } else {
        if show_journals && app.nav.mode == Mode::Browse && app.nav.focus == Focus::Journals {
            let body = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(JOURNAL_LIST_WIDTH),
                    Constraint::Min(ENTRY_LIST_MIN_WIDTH),
                ])
                .split(content);
            layout.journals = Some(PanelGeometry::new(&app.appearance.theme, body[0]));
            layout.entries = Some(EntryListGeometry::new(&app.appearance.theme, body[1]));
        } else {
            let body = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(ENTRY_LIST_INLINE_WIDTH),
                    Constraint::Min(0),
                ])
                .split(content);
            layout.entries = Some(EntryListGeometry::new(&app.appearance.theme, body[0]));
            layout.reader = Some(PanelGeometry::new(&app.appearance.theme, body[1]));
        }
    }

    layout
}
