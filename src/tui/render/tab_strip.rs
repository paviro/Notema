//! A horizontal tab strip shared by the insights panel and the filter
//! dialog. Both lay their tabs out the same way: leading pad, labels joined by a
//! ` · ` separator, collapsing from full titles to short titles to single-letter
//! initials as the width shrinks. The segment math is the one source of truth for
//! both drawing ([`tab_strip_line`]) and hit-testing ([`tab_segments`]) so they
//! never drift.

use std::ops::Range;

use ratatui::text::{Line, Span};

use crate::tui::entry_rows::text_width;
use crate::tui::theme::Theme;

/// Cells of the ` · ` separator drawn between adjacent tab labels.
const SEPARATOR: u16 = 3;

/// A tab enum that lays out on a strip with three label widths — full, short, and
/// single-letter — collapsing to the narrowest that fits.
pub(crate) trait StripTab: Copy + PartialEq + 'static {
    fn all() -> &'static [Self];
    fn title(self) -> &'static str;
    fn short_title(self) -> &'static str;
    fn initial(self) -> &'static str;
}

/// Which label set the strip is using at a given width.
#[derive(Clone, Copy)]
enum StripLevel {
    Full,
    Short,
    Initial,
}

/// Total strip width for a label function: the leading pad, every label, and a
/// [`SEPARATOR`]-cell separator between each.
fn strip_width<T: StripTab>(leading: u16, label: impl Fn(T) -> &'static str) -> usize {
    let labels: usize = T::all().iter().map(|tab| text_width(label(*tab))).sum();
    leading as usize + labels + SEPARATOR as usize * (T::all().len() - 1)
}

/// The strip's width at its full labels, no leading pad — the widest the labels
/// ever need. A dialog sizes itself to this so its tabs never collapse.
pub(crate) fn full_strip_width<T: StripTab>() -> u16 {
    strip_width::<T>(0, T::title) as u16
}

/// Pick the widest label set that fits `width`: full titles, then short titles,
/// then single-letter initials (which always fit).
fn strip_level<T: StripTab>(leading: u16, width: u16) -> StripLevel {
    let width = width as usize;
    if strip_width::<T>(leading, T::title) <= width {
        StripLevel::Full
    } else if strip_width::<T>(leading, T::short_title) <= width {
        StripLevel::Short
    } else {
        StripLevel::Initial
    }
}

/// The label for `tab` at the strip's current fit level.
fn tab_label<T: StripTab>(tab: T, leading: u16, width: u16) -> &'static str {
    match strip_level::<T>(leading, width) {
        StripLevel::Full => tab.title(),
        StripLevel::Short => tab.short_title(),
        StripLevel::Initial => tab.initial(),
    }
}

/// The column range each tab label occupies within a strip of `width`, measured
/// from the strip's start (`leading` pad, then labels with a ` · ` between).
pub(crate) fn tab_segments<T: StripTab>(leading: u16, width: u16) -> Vec<(T, Range<u16>)> {
    let mut segments = Vec::with_capacity(T::all().len());
    let mut x = leading;
    for (index, tab) in T::all().iter().enumerate() {
        if index > 0 {
            x += SEPARATOR;
        }
        let w = text_width(tab_label(*tab, leading, width)) as u16;
        segments.push((*tab, x..x + w));
        x += w;
    }
    segments
}

/// The strip as a styled line: the `active` tab carries the focused-or-not
/// active-tab style, a `hovered` non-active tab reads as plain text, the rest
/// stay dim. `leading` spaces precede the first label; a ` · ` separates each.
pub(crate) fn tab_strip_line<T: StripTab>(
    theme: &Theme,
    active: T,
    focused: bool,
    hovered: Option<T>,
    leading: u16,
    width: u16,
) -> Line<'static> {
    let mut spans = vec![Span::raw(" ".repeat(leading as usize))];
    for (index, tab) in T::all().iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(
                format!(" {} ", theme.glyphs().tab_separator),
                theme.tab_separator(),
            ));
        }
        let style = if *tab == active {
            theme.active_tab(focused)
        } else if hovered == Some(*tab) {
            theme.text()
        } else {
            theme.inactive_tab()
        };
        spans.push(Span::styled(
            tab_label(*tab, leading, width).to_string(),
            style,
        ));
    }
    Line::from(spans)
}
