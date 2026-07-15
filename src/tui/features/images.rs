use std::rc::Rc;

use ratatui::layout::{Rect, Size};

use crate::tui::{
    app::{AppModel, Focus},
    image::{ImageAsset, entry_images, viewer_image_size},
    state::{ImageViewerState, Overlay},
};

impl AppModel {
    /// Manage the image cache lifecycle. Called every tick. Warming is kicked off
    /// when the fullscreen viewer opens (not merely when the entry does), so we
    /// don't decode images the user may never look at. The cache then lives until
    /// the entry closes (or switches, or the target size changes), so reopening
    /// the viewer within the same entry stays instant.
    pub(crate) fn prepare_image_warm(
        &mut self,
        terminal_size: Size,
    ) -> Option<crate::tui::image::WarmRequest> {
        let size = viewer_image_size(Rect::new(0, 0, terminal_size.width, terminal_size.height));
        // The entry currently open in the entry view, if any.
        let open_entry = self
            .selected_entry_target()
            .map(|target| target.path)
            .filter(|_| self.nav.focus == Focus::Reader);

        // Drop the cache when the entry that warmed it is no longer open (closed
        // or switched to another entry) or the viewer's target size changed.
        let stale = match &self.image.warm {
            Some((warmed_path, warmed_size)) => {
                open_entry.as_deref() != Some(warmed_path.as_path()) || *warmed_size != size
            }
            None => false,
        };
        if stale {
            self.image.runtime.clear();
            self.image.warm = None;
        }

        // Warm only once the viewer is actually opened. `image_warm` is `None`
        // here only when nothing valid is cached (a matching cache is never
        // stale), so this builds each entry's images at most once per session.
        if matches!(self.overlay, Overlay::ImageViewer(_))
            && self.image.warm.is_none()
            && let Some(path) = open_entry
        {
            let assets = self.selected_images();
            if !assets.is_empty() {
                self.image.warm = Some((path, size));
                return Some(crate::tui::image::WarmRequest {
                    assets: (*assets).clone(),
                    size,
                });
            }
        }
        None
    }

    /// Selected entry's referenced images in body order, memoized per entry path
    /// since hot callers hit it every render, keypress, and tick. Empty when no
    /// entry is selected or it has no in-folder images.
    fn selected_images(&self) -> Rc<Vec<ImageAsset>> {
        let target_path = self.selected_entry_target().map(|target| target.path);

        if let Some((path, images)) = self.image.selected_cache.borrow().as_ref()
            && target_path.as_deref() == Some(path.as_path())
        {
            return images.clone();
        }

        let images = Rc::new(match (&target_path, self.selected_reader()) {
            (Some(path), Some((_, content))) => entry_images(&content, path),
            _ => Vec::new(),
        });
        if let Some(path) = target_path {
            *self.image.selected_cache.borrow_mut() = Some((path, images.clone()));
        }
        images
    }

    /// Owned copy for the viewer overlay, which takes ownership. Prefer
    /// [`Self::selected_images`] on read-only paths.
    fn selected_entry_images(&self) -> Vec<ImageAsset> {
        (*self.selected_images()).clone()
    }

    /// In-folder image count for the selected entry; drives the `i` footer hint
    /// and the digit shortcuts.
    pub(crate) fn selected_entry_image_count(&self) -> usize {
        self.selected_images().len()
    }

    /// Open the fullscreen viewer on the selected entry's image at `index`
    /// (clamped); no-op when the entry has no images. Focuses the entry view
    /// first so the viewer is only ever open with `Focus::Reader` — the
    /// invariant [`AppModel::prepare_image_warm`] relies on to own the cache lifecycle.
    pub(crate) fn begin_image_viewer(&mut self, index: usize) {
        let assets = self.selected_entry_images();
        if assets.is_empty() {
            return;
        }
        self.nav.focus = Focus::Reader;
        let index = index.min(assets.len() - 1);
        self.overlay = Overlay::ImageViewer(ImageViewerState { assets, index });
    }

    pub(crate) fn image_viewer_state(&self) -> Option<&ImageViewerState> {
        match &self.overlay {
            Overlay::ImageViewer(state) => Some(state),
            _ => None,
        }
    }

    /// Step the open viewer by `delta`, clamped at the ends.
    pub(crate) fn image_viewer_step(&mut self, delta: isize) {
        if let Overlay::ImageViewer(state) = &mut self.overlay {
            let len = state.assets.len();
            if len == 0 {
                return;
            }
            state.index = (state.index as isize + delta).clamp(0, len as isize - 1) as usize;
        }
    }
}
