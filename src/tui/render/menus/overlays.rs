use super::*;
use ratatui::widgets::{List, ListItem};

/// The metadata chooser's rows: a mnemonic key and the facet it opens.
const METADATA_MENU_ITEMS: [(&str, &str); 6] = [
    ("t", "Tags"),
    ("p", "People"),
    ("a", "Activities"),
    ("f", "Feelings"),
    ("m", "Mood"),
    ("l", "Location"),
];

const METADATA_MENU_WIDTH: u16 = 30;

/// The chooser's lone command — Esc closes it — as the standard hint chip.
const METADATA_MENU_HINTS: [Hint; 1] = [Hint::new("close", "esc", HintId::CancelOverlay)];

pub(crate) fn metadata_menu_hints() -> &'static [Hint] {
    &METADATA_MENU_HINTS
}

struct MetadataMenuLayout {
    area: Rect,
    list: Rect,
    footer: Rect,
}

/// The chooser's geometry, shared by the draw and the hit-test so the click map
/// can't drift from the pixels. The menu always fits, so it never scrolls.
fn metadata_menu_layout(theme: &Theme, frame_area: Rect) -> MetadataMenuLayout {
    let rows = METADATA_MENU_ITEMS.len() as u16;
    // The frame, the rows, a blank spacer, and the footer row.
    let h = (dialog_frame_rows(theme) + rows + 1 + 1).min(frame_area.height.saturating_sub(2));
    let area = centered_rect_fixed_size(METADATA_MENU_WIDTH, h, frame_area);
    let inner = dialog_content_full(theme, area);
    let list = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: rows.min(inner.height),
    };
    MetadataMenuLayout {
        area,
        list,
        footer: dialog_hints_rect(inner, 1),
    }
}

/// Draw the "Add metadata" chooser: a centered popup whose key chips open the
/// tags/people/activities/feelings/mood/location dialogs. The keys work directly;
/// the popup is a discovery aid, so a hovered row only lifts, it never "selects".
pub(crate) fn draw_metadata_menu(
    theme: &Theme,
    frame: &mut Frame<'_>,
    hovered_row: Option<usize>,
    hovered_hint: Option<HintId>,
) {
    let layout = metadata_menu_layout(theme, frame.area());
    draw_dialog_frame_wide(theme, frame, layout.area, "Add Metadata", false);

    let items: Vec<ListItem<'_>> = METADATA_MENU_ITEMS
        .iter()
        .enumerate()
        .map(|(index, (key, label))| {
            let item = ListItem::new(Line::from(vec![
                Span::styled(key_chip_text(key), key_chip_style(theme)),
                Span::raw(format!(" {label}")),
            ]));
            if hovered_row == Some(index) {
                item.style(theme.hover())
            } else {
                item
            }
        })
        .collect();
    frame.render_widget(List::new(items), layout.list);

    frame.render_widget(
        Paragraph::new(hint_lines(
            theme,
            &METADATA_MENU_HINTS,
            layout.footer.width,
            hovered_hint,
        )),
        layout.footer,
    );
}

pub(crate) fn metadata_menu_interactions(theme: &Theme, frame_area: Rect) -> MenuInteractions {
    let layout = metadata_menu_layout(theme, frame_area);
    let rows = (0..METADATA_MENU_ITEMS.len())
        .map(|index| {
            (
                Rect {
                    x: layout.list.x,
                    y: layout.list.y + index as u16,
                    width: layout.list.width,
                    height: 1,
                },
                index,
            )
        })
        .collect();
    MenuInteractions {
        rows,
        footer: layout.footer,
    }
}
