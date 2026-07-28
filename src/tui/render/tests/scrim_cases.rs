//! The scrim's blend math: how far a cell is pulled toward the dim colour.

use super::*;
use crate::tui::theme;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Style};

#[test]
fn scrim_blends_rgb_cells_and_dims_the_rest() {
    let theme = theme::test_flat_theme(); // scrim strength 0.45
    let area = Rect::new(0, 0, 3, 1);
    let mut buf = Buffer::empty(area);
    buf.set_string(0, 0, "abc", Style::default());
    buf[(0u16, 0u16)].fg = Color::Rgb(0x80, 0x80, 0x80);
    buf[(0u16, 0u16)].bg = Color::Rgb(0x10, 0x10, 0x10);
    buf[(1u16, 0u16)].fg = Color::Reset;
    buf[(2u16, 0u16)].set_diff_option(ratatui::buffer::CellDiffOption::Skip);

    chrome::scrim(&theme, &mut buf, area);

    // 0x80 * (1 - 0.45) = 0x46; the blended cell gains no DIM.
    assert_eq!(buf[(0u16, 0u16)].fg, Color::Rgb(0x46, 0x46, 0x46));
    assert_eq!(buf[(0u16, 0u16)].bg, Color::Rgb(0x08, 0x08, 0x08));
    assert!(!buf[(0u16, 0u16)].modifier.contains(Modifier::DIM));
    // Non-RGB cells fall back to the DIM modifier.
    assert!(buf[(1u16, 0u16)].modifier.contains(Modifier::DIM));
    // Graphics-protocol cells are untouched.
    assert!(!buf[(2u16, 0u16)].modifier.contains(Modifier::DIM));
}

#[test]
fn scrim_at_zero_strength_only_dims() {
    // The default terminal theme has scrim 0: every cell gets DIM, colors
    // stay untouched.
    let area = Rect::new(0, 0, 1, 1);
    let mut buf = Buffer::empty(area);
    buf[(0u16, 0u16)].fg = Color::Rgb(0x80, 0x80, 0x80);

    chrome::scrim(&theme::Theme::terminal_default(), &mut buf, area);

    assert_eq!(buf[(0u16, 0u16)].fg, Color::Rgb(0x80, 0x80, 0x80));
    assert!(buf[(0u16, 0u16)].modifier.contains(Modifier::DIM));
}

/// A fractional scrim blends proportionally and drops the DIM fallback —
/// there is no half a DIM, so a non-modal surface leaves palette themes
/// alone rather than dimming them as hard as a dialog would.
#[test]
fn a_scaled_scrim_blends_less_and_never_dims() {
    let theme = theme::test_flat_theme(); // scrim strength 0.45
    let area = Rect::new(0, 0, 2, 1);
    let mut buf = Buffer::empty(area);
    buf[(0u16, 0u16)].fg = Color::Rgb(0x80, 0x80, 0x80);
    buf[(1u16, 0u16)].fg = Color::Reset;

    chrome::scrim_scaled(&theme, &mut buf, area, 0.5);

    // 0x80 * (1 - 0.225) = 0x63, against 0x46 at full strength.
    assert_eq!(buf[(0u16, 0u16)].fg, Color::Rgb(0x63, 0x63, 0x63));
    assert!(!buf[(1u16, 0u16)].modifier.contains(Modifier::DIM));
}
