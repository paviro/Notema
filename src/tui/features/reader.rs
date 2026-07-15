use crate::tui::app::{AppModel, ReaderAnchorFlash};

use super::PAGE_STEP;

impl AppModel {
    pub(crate) fn scroll_reader(&mut self, delta: i16) {
        if delta.is_negative() {
            self.nav.scroll.reader = self.nav.scroll.reader.saturating_sub(delta.unsigned_abs());
        } else {
            self.nav.scroll.reader = self.nav.scroll.reader.saturating_add(delta as u16);
        }
    }

    pub(crate) fn page_reader(&mut self, delta: i16) {
        self.scroll_reader(delta.saturating_mul(PAGE_STEP));
    }

    /// Anchor a brief highlight on the reader line a link jumped to, so a
    /// same-page heading jump is visible. Expires via
    /// [`Self::expire_reader_heading_flash`] on tick.
    pub(crate) fn flash_reader_heading(&mut self, line: usize) {
        self.reader_anchor_flash = Some(ReaderAnchorFlash {
            line,
            until: std::time::Instant::now() + std::time::Duration::from_millis(700),
        });
    }

    pub(crate) fn expire_reader_heading_flash(&mut self) -> bool {
        if self
            .reader_anchor_flash
            .as_ref()
            .is_some_and(|flash| std::time::Instant::now() >= flash.until)
        {
            self.reader_anchor_flash = None;
            return true;
        }
        false
    }
}
