pub(crate) fn viewer_scroll(requested: u16, line_count: usize, height: u16) -> u16 {
    let max_scroll = line_count
        .saturating_sub(height as usize)
        .min(u16::MAX as usize) as u16;
    requested.min(max_scroll)
}

pub(crate) fn scrollbar_position(scroll: u16, line_count: usize, height: u16) -> usize {
    let max_scroll = line_count.saturating_sub(height as usize);
    if max_scroll == 0 {
        return 0;
    }

    (scroll as usize)
        .saturating_mul(line_count.saturating_sub(1))
        .checked_div(max_scroll)
        .unwrap_or(0)
}

pub(crate) fn clamp_scroll(requested: u16, total_height: usize, viewport_height: u16) -> u16 {
    let max_scroll = total_height
        .saturating_sub(viewport_height as usize)
        .min(u16::MAX as usize) as u16;
    requested.min(max_scroll)
}

pub(crate) fn scroll_offset(
    current: u16,
    delta: i16,
    total_height: usize,
    viewport_height: u16,
) -> u16 {
    let requested = if delta.is_negative() {
        current.saturating_sub(delta.unsigned_abs())
    } else {
        current.saturating_add(delta as u16)
    };
    clamp_scroll(requested, total_height, viewport_height)
}

pub(crate) fn ensure_index_visible(
    scroll: &mut u16,
    index: usize,
    total_height: usize,
    viewport_height: u16,
) {
    if viewport_height == 0 {
        *scroll = clamp_scroll(*scroll, total_height, viewport_height);
        return;
    }

    if index < *scroll as usize {
        *scroll = index.min(u16::MAX as usize) as u16;
    } else {
        let bottom = (*scroll as usize).saturating_add(viewport_height as usize);
        if index >= bottom {
            *scroll = index
                .saturating_add(1)
                .saturating_sub(viewport_height as usize)
                .min(u16::MAX as usize) as u16;
        }
    }
    *scroll = clamp_scroll(*scroll, total_height, viewport_height);
}
