use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Alignment, Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
    },
};

use crate::tui::entry_rows::{text_width, truncate_ellipsis};
use crate::tui::surface::scrollbar_bar_rect;
use crate::tui::theme::{ChromeStyle, Theme};

/// True when the active theme separates surfaces by background layers instead
/// of drawn borders.
pub(crate) fn flat_chrome(theme: &Theme) -> bool {
    theme.chrome() == ChromeStyle::Flat
}

/// The style painted under a whole frame: the theme background plus its default
/// text color, so spans without an explicit fg stay readable on it. A no-op
/// under terminal-default themes (both components are `Reset`/absent).
pub(crate) fn base_style(theme: &Theme) -> Style {
    surface_style(theme, theme.base_bg())
}

/// The surface painted under the hint/footer bar. Defaults to the base surface,
/// so the footer sits flush with the backdrop unless a theme tints
/// `surfaces.footer`.
pub(crate) fn footer_style(theme: &Theme) -> Style {
    surface_style(theme, theme.footer_bg())
}

/// A surface fill: the given background plus the theme's text color, so spans
/// without an explicit fg stay readable on it.
pub(crate) fn surface_style(theme: &Theme, bg: Color) -> Style {
    let mut style = Style::default().bg(bg);
    if let Some(fg) = theme.text().fg {
        style = style.fg(fg);
    }
    style
}

/// Wipe `area` and repaint it as a themed surface, in one step. `Clear` resets
/// cells to the *terminal's* colors — a light-mode surface on a dark terminal
/// would show unstyled text in the terminal's near-white ink — so every
/// overlay must re-establish the ink+bg invariant before drawing content;
/// pairing the two here makes that impossible to forget.
pub(crate) fn clear_surface(theme: &Theme, frame: &mut Frame<'_>, area: Rect, bg: Color) {
    frame.render_widget(ratatui::widgets::Clear, area);
    frame.buffer_mut().set_style(area, surface_style(theme, bg));
}

/// Dim everything drawn so far, so an overlay rendered afterwards floats on a
/// darkened backdrop. True-color cells blend toward black by the theme's scrim
/// strength; palette/terminal-default cells (and strength 0) fall back to the
/// DIM modifier. Cells owned by terminal graphics protocols (`skip`) can't be
/// restyled and stay bright.
pub(crate) fn scrim(theme: &Theme, buf: &mut Buffer, area: Rect) {
    let keep = 1.0 - theme.scrim_strength().clamp(0.0, 1.0);
    let mul = |channel: u8| (f32::from(channel) * keep) as u8;
    for pos in area.positions() {
        let cell = &mut buf[pos];
        if cell.diff_option == ratatui::buffer::CellDiffOption::Skip {
            continue;
        }
        let mut blended = false;
        if keep < 1.0 {
            for color in [&mut cell.fg, &mut cell.bg] {
                if let Color::Rgb(r, g, b) = *color {
                    *color = Color::Rgb(mul(r), mul(g), mul(b));
                    blended = true;
                }
            }
        }
        if !blended {
            cell.modifier.insert(Modifier::DIM);
        }
    }
}

// ── Toasts ────────────────────────────────────────────────────────────────────

/// The style for the thin `─` rules that subdivide dialogs.
pub(crate) fn separator_style(theme: &Theme) -> Style {
    if flat_chrome(theme) {
        theme.faint_rule()
    } else {
        theme.muted()
    }
}

/// A `label ···· value` row: `label` on the left, `value` pinned to the right
/// edge, tied by a dot leader. Each side keeps its own span style. On the
/// highlighted row the leader is blanked so the selection bar carries the eye
/// instead of competing with the dots. Truncates the label with `…` when it
/// would otherwise collide with `value`.
pub(crate) fn dot_leader_line(
    theme: &Theme,
    label: Span<'static>,
    value: Span<'static>,
    width: u16,
    selected: bool,
) -> Line<'static> {
    let total = width as usize;
    let value_w = text_width(&value.content);
    // label + " " + leader(≥1) + " " + value == total.
    let max_label = total.saturating_sub(value_w + 3);
    let label_text = truncate_ellipsis(&label.content, max_label);
    let label_w = text_width(&label_text);
    let fill = total.saturating_sub(label_w + value_w + 2).max(1);
    let leader = if selected {
        Span::raw(" ".repeat(fill + 2))
    } else {
        Span::styled(format!(" {} ", ".".repeat(fill)), separator_style(theme))
    };
    Line::from(vec![Span::styled(label_text, label.style), leader, value])
}

/// A titled content container inside a full-screen modal (unlock, pending
/// notices). Bordered chrome keeps the padded box; flat chrome swaps the
/// border for a panel background with the same inner geometry.
pub(crate) fn container_block<'a>(active_theme: &'a Theme, title: &str) -> Block<'a> {
    if flat_chrome(active_theme) {
        Block::new()
            .style(Style::default().bg(active_theme.content_bg()))
            .padding(Padding::new(3, 3, 2, 2))
            .title_top(Line::from(Span::styled(
                format!(" {title} "),
                active_theme.heading(),
            )))
    } else {
        Block::default()
            .borders(Borders::ALL)
            .border_set(active_theme.glyphs().borders.border_set())
            .border_style(active_theme.dialog_border())
            .title_top(Line::from(format!(" {title} ")))
            .padding(Padding::new(2, 2, 1, 1))
    }
}

/// Rows a `container_block` reserves around its content: border + padding, plus
/// the top title row that flat chrome has no border to fold into. Measured off
/// the block so it can't drift from `container_block`'s padding.
pub(crate) fn container_block_vertical_inset(block: &Block<'_>, area: Rect) -> u16 {
    area.height.saturating_sub(block.inner(area).height)
}

/// In flat chrome the focused panel is marked by a `┃` stripe down its left
/// padding column — there is no border to thicken, so focus needs its own ink.
/// No-op on bordered chrome or unfocused panels.
pub(crate) fn panel_focus_stripe(theme: &Theme, frame: &mut Frame<'_>, area: Rect, focused: bool) {
    if !flat_chrome(theme) || !focused || area.width == 0 {
        return;
    }
    let glyph = theme.glyphs().focus_stripe.to_string();
    let stripe: Vec<Line<'static>> = (0..area.height)
        .map(|_| Line::from(Span::styled(glyph.clone(), theme.focus_border())))
        .collect();
    frame.render_widget(Paragraph::new(stripe), Rect { width: 1, ..area });
}

pub(crate) fn panel_block<'a>(
    active_theme: &'a Theme,
    title: &str,
    focused: bool,
    footer_label: Option<String>,
) -> Block<'a> {
    if flat_chrome(active_theme) {
        let mut block = Block::new()
            .style(Style::default().bg(active_theme.content_bg()))
            .padding(Padding::uniform(1))
            .title(panel_title(active_theme, title, focused));
        if let Some(label) = footer_label {
            block = block.title_bottom(
                Line::from(Span::styled(format!(" {label} "), active_theme.muted()))
                    .right_aligned(),
            );
        }
        return block;
    }

    let mut block = Block::default()
        .title(panel_title(active_theme, title, focused))
        .borders(Borders::ALL)
        .border_set(active_theme.glyphs().block_set(focused));

    if focused {
        block = block.border_style(active_theme.focus_border());
    } else {
        block = block.border_style(active_theme.inactive_border());
    }

    if let Some(label) = footer_label {
        block = block.title_bottom(Line::from(format!(" {label} ")).right_aligned());
    }

    block
}

/// Draw a dimmed message centered both horizontally and vertically within a
/// panel's content area — used for empty states like "No entry selected" and
/// "No results".
pub(crate) fn render_centered_notice(
    theme: &Theme,
    frame: &mut Frame<'_>,
    content: Rect,
    message: &str,
) {
    if content.width == 0 || content.height == 0 {
        return;
    }
    let line = Rect {
        y: content.y + content.height.saturating_sub(1) / 2,
        height: 1,
        ..content
    };
    frame.render_widget(
        Paragraph::new(message)
            .alignment(Alignment::Center)
            .style(theme.muted()),
        line,
    );
}

// ── Confirm-dialog buttons (shared by confirm-delete and editor discard) ─────

pub(crate) fn count_label(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("{count} {singular}")
    } else {
        format!("{count} {plural}")
    }
}

pub(crate) fn panel_title(theme: &Theme, title: &str, focused: bool) -> Line<'static> {
    let label = format!(" {title} ");
    if flat_chrome(theme) {
        // No border to thicken, so the title itself carries focus: accent+bold
        // when focused, muted otherwise. An extra leading space
        // indents the title clear of the card's left edge (and the focus
        // stripe) so it lines up with the padded content below.
        let style = if focused {
            theme.primary().add_modifier(Modifier::BOLD)
        } else {
            theme.muted()
        };
        return Line::from(Span::styled(format!(" {label}"), style));
    }
    if focused {
        Line::from(Span::styled(label, theme.selection()))
    } else {
        Line::from(label)
    }
}

pub(crate) fn render_vertical_scrollbar(
    theme: &Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    state: &mut ScrollbarState,
    focused: bool,
) {
    render_vertical_scrollbar_in(theme, frame, scrollbar_bar_rect(theme, area), state, focused);
}

/// Render a vertical scrollbar into an explicit track rect, bypassing the
/// `scrollbar_bar_rect` inset — callers that already know the exact column and
/// row span pass it directly.
fn render_vertical_scrollbar_in(
    theme: &Theme,
    frame: &mut Frame<'_>,
    bar: Rect,
    state: &mut ScrollbarState,
    focused: bool,
) {
    let glyphs = theme.glyphs();
    let thumb = glyphs.scrollbar_thumb.to_string();
    let track = glyphs.scrollbar_track.to_string();
    let up = glyphs.scrollbar_up.to_string();
    let down = glyphs.scrollbar_down.to_string();
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .thumb_symbol(&thumb)
        .thumb_style(theme.scrollbar_thumb(focused))
        .track_symbol(Some(&track))
        .track_style(theme.scrollbar_track(focused))
        .begin_symbol(Some(&up))
        .begin_style(theme.scrollbar_arrow(focused))
        .end_symbol(Some(&down))
        .end_style(theme.scrollbar_arrow(focused));
    frame.render_stateful_widget(scrollbar, bar, state);
}

pub(crate) fn render_scrollbar_if_needed(
    theme: &Theme,
    frame: &mut Frame<'_>,
    area: Rect,
    total_height: usize,
    viewport_height: u16,
    scroll: usize,
    focused: bool,
) {
    if total_height > viewport_height as usize {
        let mut state = ScrollbarState::default()
            .content_length(total_height)
            .viewport_content_length(viewport_height as usize)
            .position(crate::tui::scroll::scrollbar_position(
                scroll,
                total_height,
                viewport_height,
            ));
        render_vertical_scrollbar(theme, frame, area, &mut state, focused);
    }
}

/// The column a dialog list's scrollbar occupies: one padding column past the
/// list's right edge. In flat chrome that seats it inside the surface with a
/// symmetric margin; in bordered chrome it lands on the frame's right border.
fn dialog_list_scrollbar_x(list: Rect) -> u16 {
    list.x.saturating_add(list.width).saturating_add(1)
}

/// Draw a dialog list's scrollbar one padding column past the list, spanning
/// only the list rows so it reads as part of the list rather than dialog
/// chrome. Bordered chrome keeps the bar on the frame border, just not full
/// height.
pub(crate) fn render_dialog_list_scrollbar(
    theme: &Theme,
    frame: &mut Frame<'_>,
    list: Rect,
    total_height: usize,
    scroll: usize,
    focused: bool,
) {
    let viewport = list.height;
    if total_height > viewport as usize {
        let mut state = ScrollbarState::default()
            .content_length(total_height)
            .viewport_content_length(viewport as usize)
            .position(crate::tui::scroll::scrollbar_position(
                scroll,
                total_height,
                viewport,
            ));
        let bar = Rect {
            x: dialog_list_scrollbar_x(list),
            y: list.y,
            width: 1,
            height: list.height,
        };
        render_vertical_scrollbar_in(theme, frame, bar, &mut state, focused);
    }
}

pub(crate) fn centered_rect_fixed_size(width: u16, height: u16, area: Rect) -> Rect {
    let [row] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(area);
    let [cell] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(row);
    cell
}
