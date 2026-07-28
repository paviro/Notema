//! Viewer scroll clamping, and scrollbar track and thumb geometry.

use super::*;

#[test]
fn viewer_scroll_clamps_to_rendered_content_height() {
    assert_eq!(viewer_scroll(100, 20, 8), 12);
    assert_eq!(viewer_scroll(5, 4, 8), 0);
}

#[test]
fn viewer_scroll_saturates_large_rendered_content_height() {
    assert_eq!(viewer_scroll(u16::MAX, 100_000, 8), u16::MAX);
}

#[test]
fn scrollbar_position_reaches_end_at_viewer_bottom() {
    let line_count = 40;
    let height = 20;
    let scroll = viewer_scroll(u16::MAX, line_count, height);

    assert_eq!(scroll, 20);
    assert_eq!(scrollbar_position(scroll as usize, line_count, height), 39);
}

#[test]
fn scrollbar_position_stays_at_start_when_content_fits() {
    assert_eq!(scrollbar_position(0, 4, 8), 0);
}

#[test]
fn scrollbar_bar_rect_matches_rendered_track() {
    let area = Rect::new(2, 3, 20, 10);
    let flat =
        theme::test_flat_theme().with_chrome_override(Some(crate::tui::theme::ChromeStyle::Flat));
    let flat_bar = scrollbar_bar_rect(&flat, area);
    let flat_content = PanelGeometry::new(&flat, area).content;
    assert_eq!(flat_bar, Rect::new(20, 4, 1, 8));
    assert_eq!(flat_content.x + flat_content.width, flat_bar.x - 1);
    assert_eq!(flat_bar.x + flat_bar.width + 1, area.x + area.width);

    let bordered = theme::test_flat_theme()
        .with_chrome_override(Some(crate::tui::theme::ChromeStyle::Bordered));
    let bordered_bar = scrollbar_bar_rect(&bordered, area);
    let bordered_content = PanelGeometry::new(&bordered, area).content;
    assert_eq!(bordered_bar, Rect::new(21, 4, 1, 8));
    assert_eq!(bordered_content.x, area.x + 2);
    assert_eq!(
        bordered_content.x + bordered_content.width,
        bordered_bar.x - 1
    );
}

#[test]
fn scroll_from_thumb_top_maps_travel_ends_to_scroll_range() {
    // track_top 5, track_len 10, thumb_len 4 → the thumb travels rows 5..=11
    // (travel 6). Top of travel → 0, bottom → max, rows clamp past the ends.
    let (track_top, track_len, thumb_len, max) = (5, 10, 4, 100);
    assert_eq!(
        scroll_from_thumb_top(track_top, track_top, track_len, thumb_len, max),
        0
    );
    assert_eq!(
        scroll_from_thumb_top(0, track_top, track_len, thumb_len, max),
        0
    );
    assert_eq!(
        scroll_from_thumb_top(track_top + 6, track_top, track_len, thumb_len, max),
        max
    );
    assert_eq!(
        scroll_from_thumb_top(u16::MAX, track_top, track_len, thumb_len, max),
        max
    );
}

#[test]
fn scroll_from_thumb_top_handles_untravellable_thumbs() {
    assert_eq!(scroll_from_thumb_top(7, 5, 10, 4, 0), 0); // no overflow
    assert_eq!(scroll_from_thumb_top(7, 5, 4, 4, 100), 0); // thumb fills track
}

#[test]
fn scrollbar_thumb_sits_below_the_up_arrow_at_the_top() {
    // Bar of 12 rows starting at y=3: arrows at rows 3 and 14, track rows 4..=13.
    let bar = Rect::new(20, 3, 1, 12);
    let (top, len) = scrollbar_thumb(bar, 40, 10, 0).expect("thumb");
    assert_eq!(top, 4, "thumb starts just below the up arrow at scroll 0");
    assert!(len >= 1);
}

#[test]
fn scrollbar_thumb_reaches_bottom_of_track_at_max_scroll() {
    let bar = Rect::new(20, 3, 1, 12);
    let line_count = 40;
    let height = 10;
    let scroll = viewer_scroll(u16::MAX, line_count, height) as usize;
    let position = scrollbar_position(scroll, line_count, height);
    let (top, len) = scrollbar_thumb(bar, line_count, height, position).expect("thumb");
    // Track rows are 4..=13; the thumb's bottom edge reaches the last track row.
    assert_eq!(top + len - 1, 13);
}

#[test]
fn scrollbar_thumb_none_when_bar_too_short() {
    assert_eq!(scrollbar_thumb(Rect::new(0, 0, 1, 2), 40, 10, 0), None);
}
