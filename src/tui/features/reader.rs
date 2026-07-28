//! The entry reader: scrolling it, and the link-hint mode that labels every
//! openable target in the body so a keystroke can pick one.

use crate::tui::app::{AppModel, ReaderAnchorFlash, ReaderHint};

use super::PAGE_STEP;

/// What typing one more character does to the pending label.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReaderHintMatch {
    /// No label continues — the keystroke is ignored and the mode stays up.
    /// Which letters mean nothing changes with the entry, so a miss must not be
    /// an exit or a typo would drop the reader out of the mode.
    Dead,
    /// At least one label continues, none completes.
    Prefix,
    /// Index into [`ReaderHintState::labels`] of the label just completed.
    Exact(usize),
}

/// The reader's link-hint mode: labels over every openable target in the entry.
/// Non-modal, like [`SuggestionState`] and unlike [`Overlay`] — the reader stays
/// live and scrollable underneath.
///
/// `Esc` is the only way out, so every letter stays free to be a label, `o`
/// included. `labels` is written only by the renderer through
/// `Action::ViewRendered`, so a label the keyboard matches is always one the
/// reader can see.
#[derive(Default)]
pub(crate) struct ReaderHintState {
    /// How many openable targets the last frame found, whether or not hints were
    /// on — it is what gates the `o` key and its footer chip. Kept here rather
    /// than beside it because one frame writes both, and two fields with one
    /// writer are two chances to disagree.
    openable: usize,
    active: bool,
    /// Label characters typed so far, always a proper prefix of some label.
    pending: String,
    labels: Vec<ReaderHint>,
}

impl ReaderHintState {
    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    /// Whether the reader has anything `o` could open.
    pub(crate) fn has_openable(&self) -> bool {
        self.openable > 0
    }

    pub(crate) fn pending(&self) -> &str {
        &self.pending
    }

    pub(crate) fn labels(&self) -> &[ReaderHint] {
        &self.labels
    }

    /// Enter the mode. The stale label set goes with it; the frame that follows
    /// installs the real one before another key can arrive.
    pub(crate) fn begin(&mut self) {
        self.active = true;
        self.pending.clear();
        self.labels.clear();
    }

    pub(crate) fn deactivate(&mut self) {
        self.active = false;
        self.pending.clear();
        self.labels.clear();
    }

    /// Adopt the labels a frame painted.
    ///
    /// A changed label set drops the pending prefix: the targets the user is
    /// looking at are no longer the ones they started typing against. This is
    /// the whole invalidation story — scrolling, resizing, switching entry,
    /// opening an overlay and losing focus all reach it by painting differently.
    /// A frame that paints none ends the mode, which is how every suspension is
    /// spelt.
    pub(crate) fn sync(&mut self, labels: Vec<ReaderHint>, openable: usize) {
        self.openable = openable;
        if !self.active {
            return;
        }
        if labels.is_empty() {
            self.deactivate();
            return;
        }
        if self.labels != labels {
            self.pending.clear();
        }
        self.labels = labels;
    }

    pub(crate) fn resolve(&self, ch: char) -> ReaderHintMatch {
        let mut typed = self.pending.clone();
        typed.push(ch);
        if let Some(index) = self.labels.iter().position(|hint| hint.label == typed) {
            return ReaderHintMatch::Exact(index);
        }
        if self
            .labels
            .iter()
            .any(|hint| hint.label.starts_with(&typed))
        {
            return ReaderHintMatch::Prefix;
        }
        ReaderHintMatch::Dead
    }

    pub(crate) fn push(&mut self, ch: char) {
        self.pending.push(ch);
    }

    /// Undo one typed character. Backspacing past the start does nothing: `Esc`
    /// is the single exit, so no other key can drop the mode out from under a
    /// reader who is still choosing.
    pub(crate) fn pop(&mut self) {
        self.pending.pop();
    }
}

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

#[cfg(test)]
mod reader_hint_tests {
    use super::*;
    use crate::tui::app::ReaderLinkTarget;

    fn hint(label: &str, uri: &str) -> ReaderHint {
        ReaderHint {
            label: label.to_string(),
            target: ReaderLinkTarget::Uri(uri.to_string()),
            heading_line: None,
        }
    }

    fn active(labels: &[(&str, &str)]) -> ReaderHintState {
        let mut state = ReaderHintState::default();
        state.begin();
        let labels: Vec<_> = labels.iter().map(|(l, u)| hint(l, u)).collect();
        let openable = labels.len();
        state.sync(labels, openable);
        state
    }

    #[test]
    fn resolve_reports_an_exact_match_on_a_unique_label() {
        let state = active(&[("a", "https://one"), ("s", "https://two")]);
        assert_eq!(state.resolve('s'), ReaderHintMatch::Exact(1));
    }

    #[test]
    fn resolve_reports_a_prefix_until_one_label_is_complete() {
        let mut state = active(&[("aa", "https://one"), ("as", "https://two")]);
        assert_eq!(state.resolve('a'), ReaderHintMatch::Prefix);
        state.push('a');
        assert_eq!(state.resolve('s'), ReaderHintMatch::Exact(1));
    }

    #[test]
    fn resolve_reports_dead_for_a_letter_outside_the_alphabet() {
        let state = active(&[("a", "https://one")]);
        assert_eq!(state.resolve('z'), ReaderHintMatch::Dead);
    }

    #[test]
    fn sync_resets_pending_when_the_label_set_changes() {
        let mut state = active(&[("aa", "https://one"), ("as", "https://two")]);
        state.push('a');
        state.sync(
            vec![hint("aa", "https://three"), hint("as", "https://four")],
            2,
        );
        assert!(state.pending().is_empty());
    }

    #[test]
    fn sync_keeps_pending_when_the_label_set_is_unchanged() {
        let mut state = active(&[("aa", "https://one"), ("as", "https://two")]);
        state.push('a');
        state.sync(
            vec![hint("aa", "https://one"), hint("as", "https://two")],
            2,
        );
        assert_eq!(state.pending(), "a");
    }

    /// A frame painting nothing ends the mode — that is how every suspension is
    /// spelt, from losing focus to opening an overlay.
    #[test]
    fn sync_with_no_labels_ends_the_mode() {
        let mut state = active(&[("a", "https://one")]);
        state.sync(Vec::new(), 0);
        assert!(!state.is_active());
    }

    #[test]
    fn sync_is_inert_once_the_mode_is_down() {
        let mut state = ReaderHintState::default();
        state.sync(vec![hint("a", "https://one")], 1);
        assert!(!state.is_active());
        assert!(state.labels().is_empty());
    }

    /// Backspace undoes a typed character but never ends the mode — `Esc` is the
    /// only exit, so nothing else can drop it out from under a reader mid-choice.
    #[test]
    fn pop_undoes_one_character_and_never_leaves_the_mode() {
        let mut state = active(&[("aa", "https://one"), ("as", "https://two")]);
        state.push('a');
        state.pop();
        assert!(state.pending().is_empty());
        assert!(state.is_active());
        state.pop();
        assert!(state.is_active());
    }
}
