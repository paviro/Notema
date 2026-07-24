//! Dialog and full-screen modal frames, plus the shared yes/no confirm
//! buttons: the chrome every overlay draws before its own content.

use ratatui::{
    Frame,
    layout::{Alignment, Margin, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::tui::surface::surface_content_inner;
use crate::tui::theme::Theme;

use super::chrome::{centered_rect_fixed_size, clear_surface, flat_chrome, surface_style};
use super::footer::key_chip_style;

/// Rows a dialog's frame consumes above and below its content: the two border
/// rows when bordered; flat trades them for a padding row, the title row, and
/// a blank row below the title on top, plus a padding row below the content —
/// so nothing sits on the card's edge and the title breathes. Sizing helpers
/// add this to their content rows.
pub(crate) fn dialog_frame_rows(theme: &Theme) -> u16 {
    if flat_chrome(theme) { 4 } else { 2 }
}

/// A dialog's content rect within its outer `area`. Draw functions and mouse
/// hit-tests both derive geometry from this one place, so they can never
/// drift apart. Bordered chrome insets by the border; flat chrome trades the
/// side borders for a wider breathing margin, with a blank padding row above
/// the title and below the content.
pub(crate) fn dialog_inner(theme: &Theme, area: Rect) -> Rect {
    // Saturating per-axis (unlike `Rect::inner`, which zeroes the whole rect):
    // sizing helpers probe with height-1 rects and still need the real width.
    let top = if flat_chrome(theme) { 3 } else { 1 };
    let frame_inner = Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(top),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(dialog_frame_rows(theme)),
    };
    surface_content_inner(theme, frame_inner)
}

/// Columns between a dialog list's right edge and the non-list rows' right
/// edge: a padding column and the scrollbar. The list holds them back; other
/// rows reclaim them via [`dialog_content_full`]. Bordered chrome reserves
/// nothing here — its bar rides the frame border.
pub(crate) fn dialog_list_gutter(theme: &Theme) -> u16 {
    2 * u16::from(flat_chrome(theme))
}

/// A dialog's content rect for rows that sit beside no scrollbar. In flat
/// chrome it runs flush with the scrollbar's right edge so titles/inputs/hints
/// don't shrink to leave the bar room, while keeping the surface margin
/// symmetric with the left; a list narrows to [`dialog_list_width`] only when it
/// overflows and shows a bar. Bordered chrome equals [`dialog_inner`].
pub(crate) fn dialog_content_full(theme: &Theme, area: Rect) -> Rect {
    let inner = dialog_inner(theme, area);
    // Flat reclaims one of the two columns the narrow inner held off the edge,
    // leaving the other as a right margin that matches the left.
    let reclaim = u16::from(flat_chrome(theme));
    Rect {
        width: inner.width.saturating_add(reclaim),
        ..inner
    }
}

/// A dialog list's width within a [`dialog_content_full`] rect. A list of
/// `total` rows in a `viewport`-row window reserves the scrollbar gutter only
/// when it overflows and a bar is drawn; otherwise it fills the full width flush
/// with the other rows. Mirrors `render_dialog_list_scrollbar`'s overflow guard.
pub(crate) fn dialog_list_width(theme: &Theme, inner_width: u16, total: usize, viewport: u16) -> u16 {
    if total > viewport as usize {
        inner_width.saturating_sub(dialog_list_gutter(theme))
    } else {
        inner_width
    }
}

/// Clear and frame a dialog, returning its content rect (always
/// [`dialog_inner`] of `area`). Bordered chrome draws the classic titled box;
/// flat chrome paints a dialog-colored surface with a bold title row and, when
/// `esc_hint` is set, a muted `esc` dismiss hint on the right.
pub(crate) fn draw_dialog_frame(
    theme: &Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    esc_hint: bool,
) -> Rect {
    draw_dialog_frame_with_title_row(theme, frame, area, dialog_inner(theme, area), title, esc_hint)
}

/// Like [`draw_dialog_frame`], but the flat title row runs flush with the
/// scrollbar's right edge ([`dialog_content_full`]) — for list dialogs whose
/// body reclaims the scrollbar gutter, so the `esc` hint aligns with them.
pub(crate) fn draw_dialog_frame_wide(
    theme: &Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    esc_hint: bool,
) -> Rect {
    let title_row = dialog_content_full(theme, area);
    draw_dialog_frame_with_title_row(theme, frame, area, title_row, title, esc_hint)
}

fn draw_dialog_frame_with_title_row(
    theme: &Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    title_row: Rect,
    title: &str,
    esc_hint: bool,
) -> Rect {
    clear_surface(theme, frame, area, theme.dialog_bg());
    let title = title.trim();
    if flat_chrome(theme) {
        // The title sits below a blank padding row, off the card's edge.
        let top = Rect {
            x: title_row.x,
            y: area.y + 1.min(area.height.saturating_sub(1)),
            width: title_row.width,
            height: 1.min(area.height),
        };
        if !title.is_empty() {
            frame.render_widget(
                Paragraph::new(Span::styled(title.to_string(), theme.heading())),
                top,
            );
        }
        if esc_hint {
            frame.render_widget(
                Paragraph::new(Span::styled("esc", theme.muted())).alignment(Alignment::Right),
                top,
            );
        }
    } else {
        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_set(theme.glyphs().borders.border_set())
            .border_style(theme.dialog_border());
        if !title.is_empty() {
            block = block.title(format!(" {title} "));
        }
        frame.render_widget(block, area);
    }
    dialog_inner(theme, area)
}

/// Width and gap of the two confirm buttons; sized for a comfortable click target
/// with room for the label and its key hint.
const CONFIRM_BUTTON_WIDTH: u16 = 16;

const CONFIRM_BUTTON_GAP: u16 = 2;

/// The `(yes, no)` button rects, centered on the last row of `inner`. Sizing and
/// hit-testing both derive from this, so the drawn buttons match the click targets.
pub(crate) fn confirm_button_rects(inner: Rect) -> (Rect, Rect) {
    let y = inner.y + inner.height.saturating_sub(1);
    let total = CONFIRM_BUTTON_WIDTH * 2 + CONFIRM_BUTTON_GAP;
    let start = inner.x + inner.width.saturating_sub(total) / 2;
    let yes = Rect {
        x: start,
        y,
        width: CONFIRM_BUTTON_WIDTH,
        height: 1,
    };
    let no = Rect {
        x: start + CONFIRM_BUTTON_WIDTH + CONFIRM_BUTTON_GAP,
        ..yes
    };
    (yes, no)
}

/// Draw the two confirm buttons on the last row of `inner`. `selected` is the
/// keyboard-highlighted button (`true` = yes); `hovered` overrides it. The
/// active button gets the accent chip, the other the neutral surface, and a
/// hovered button takes the `button_hover` patch on top.
pub(crate) fn render_confirm_buttons(
    theme: &Theme,
    frame: &mut Frame<'_>,
    inner: Rect,
    yes_label: &str,
    no_label: &str,
    selected: bool,
    hovered: Option<bool>,
) {
    let (yes, no) = confirm_button_rects(inner);
    let active = hovered.unwrap_or(selected);
    for (area, label, is_yes) in [(yes, yes_label, true), (no, no_label, false)] {
        // Flat chrome pads a filled chip; bordered brackets it. Same rects either
        // way, so the click targets from `confirm_button_rects` stay valid.
        let text = if flat_chrome(theme) {
            format!(" {label} ")
        } else {
            format!("[ {label} ]")
        };
        let mut style = if is_yes == active {
            if flat_chrome(theme) {
                theme.button()
            } else {
                key_chip_style(theme)
            }
        } else {
            surface_style(theme, theme.raised_bg()).patch(theme.muted())
        };
        if hovered == Some(is_yes) {
            style = style.patch(theme.button_hover());
        }
        frame.render_widget(
            Paragraph::new(Span::styled(text, style)).alignment(Alignment::Center),
            area,
        );
    }
}

/// Draw the internal editor's "Discard changes?" confirmation as a centered
/// modal, matching the confirm-delete dialog's look.
pub(crate) fn draw_editor_discard_confirm(
    theme: &Theme,
    frame: &mut Frame<'_>,
    selected: bool,
    hovered: Option<bool>,
) {
    let area = editor_discard_confirm_area(theme, frame.area());
    let inner = draw_dialog_frame(theme, frame, area, "Discard Changes", true);
    let line = Rect {
        y: inner.y,
        height: 1,
        ..inner
    };
    frame.render_widget(
        Paragraph::new("Discard unsaved changes?").alignment(Alignment::Center),
        line,
    );
    render_confirm_buttons(theme, frame, inner, "Discard", "Keep", selected, hovered);
}

pub(crate) fn editor_discard_confirm_area(theme: &Theme, frame_area: Rect) -> Rect {
    // Message + blank + buttons, inside the frame.
    centered_rect_fixed_size(42, 3 + dialog_frame_rows(theme), frame_area)
}

/// Draw the full-screen "journal chrome" frame shared by the startup modals
/// (unlock, device-access request, and the enroll/awaiting/disable notices)
/// and the image viewer: a bordered block titled top-left with the screen
/// name and, when non-empty, `status` bottom-left and `key_hint` bottom-right.
/// Clears the screen first and returns the inner area to lay the modal's
/// content into.
pub(crate) fn draw_modal_frame(
    theme: &Theme,
    frame: &mut Frame<'_>,
    title: &str,
    status: &str,
    key_hint: &str,
) -> Rect {
    let area = frame.area();
    clear_surface(theme, frame, area, theme.base_bg());

    if flat_chrome(theme) {
        // No outer border: the screen name and hints sit on full-width
        // element-surface bars along the top and bottom, like status bars.
        let bar = Style::default().bg(theme.raised_bg());
        let top_bar = Rect {
            height: 1.min(area.height),
            ..area
        };
        frame.buffer_mut().set_style(top_bar, bar);
        let top = Rect {
            x: area.x + 1,
            width: area.width.saturating_sub(2),
            ..top_bar
        };
        frame.render_widget(
            Paragraph::new(Span::styled(format!(" {title} "), theme.muted())),
            top,
        );
        if area.height > 1 && (!status.is_empty() || !key_hint.is_empty()) {
            let bottom_bar = Rect {
                y: area.y + area.height - 1,
                ..top_bar
            };
            frame.buffer_mut().set_style(bottom_bar, bar);
            let bottom = Rect {
                y: bottom_bar.y,
                ..top
            };
            if !status.is_empty() {
                frame.render_widget(
                    Paragraph::new(Span::styled(format!(" {status} "), theme.muted())),
                    bottom,
                );
            }
            if !key_hint.is_empty() {
                frame.render_widget(
                    Paragraph::new(Span::styled(format!(" {key_hint} "), theme.muted()))
                        .alignment(Alignment::Right),
                    bottom,
                );
            }
        }
        return area.inner(Margin {
            vertical: 1,
            horizontal: 1,
        });
    }

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_set(theme.glyphs().borders.border_set())
        .border_style(theme.dialog_border())
        .title_top(Line::from(format!(" {title} ")));
    if !status.is_empty() {
        block = block.title_bottom(Line::from(format!(" {status} ")));
    }
    if !key_hint.is_empty() {
        block = block.title_bottom(Line::from(format!(" {key_hint} ")).right_aligned());
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}
