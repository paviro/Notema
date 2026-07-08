//! Focused state containers held by [`App`](super::app::App), split out so the
//! reset/lifecycle logic for each concern lives in one place.

use std::time::{Duration, Instant};

use journal_storage::SearchHit;
use ratatui::widgets::ListState;

use super::app::SearchScope;
use super::image::ImageAsset;

const STATUS_DURATION: Duration = Duration::from_secs(3);

/// Vertical scroll offsets for the panels that scroll their own body: the entry
/// preview, and the insights panel's ranked-list tabs (People / Activities / Tags).
#[derive(Default)]
pub(crate) struct ScrollState {
    pub(crate) entry_view: u16,
    /// First visible row of the insights list tabs, in row units (not pixels).
    pub(crate) stats: u16,
}

impl ScrollState {
    /// Reset the entry preview scroll.
    pub(crate) fn reset_entry_view(&mut self) {
        self.entry_view = 0;
    }

    /// Reset the insights list scroll — called when the tab, scope, or journal
    /// changes so a new list starts at the top.
    pub(crate) fn reset_stats(&mut self) {
        self.stats = 0;
    }
}

/// Transient status-bar message with an auto-expiry deadline.
#[derive(Default)]
pub(crate) struct StatusBar {
    text: String,
    until: Option<Instant>,
}

impl StatusBar {
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn set(&mut self, message: impl Into<String>) {
        self.text = message.into();
        self.until = Some(Instant::now() + STATUS_DURATION);
    }

    pub(crate) fn clear(&mut self) {
        self.text.clear();
        self.until = None;
    }

    pub(crate) fn timeout(&self) -> Option<Duration> {
        self.until
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    /// Clear the status if its deadline has passed, reporting whether it did.
    pub(crate) fn expire(&mut self) -> bool {
        if self
            .until
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.clear();
            return true;
        }

        false
    }

    /// Set a message whose deadline is already in the past (test helper).
    #[cfg(test)]
    pub(crate) fn set_expired(&mut self, message: impl Into<String>) {
        self.text = message.into();
        self.until = Some(Instant::now() - Duration::from_secs(1));
    }
}

/// Search query, scope and the hits it currently matches.
pub(crate) struct SearchState {
    pub(crate) query: String,
    /// Caret position as a char index into `query`, in `0..=query.chars().count()`.
    pub(crate) cursor: usize,
    pub(crate) scope: SearchScope,
    pub(crate) hits: Vec<SearchHit>,
    /// Blink phase of the search caret; toggled on a timer by the event loop and
    /// read when rendering the search field. `true` = caret block shown.
    pub(crate) cursor_visible: bool,
    /// Set when the query changed but the (expensive) hit recompute has been
    /// deferred; the event loop runs it once typing pauses (debounce).
    pub(crate) dirty: bool,
    /// Timestamp of the last search keystroke, for the debounce window.
    pub(crate) last_edit: Option<Instant>,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            query: String::new(),
            cursor: 0,
            scope: SearchScope::AllJournals,
            hits: Vec::new(),
            cursor_visible: true,
            dirty: false,
            last_edit: None,
        }
    }
}

/// A `ListState` with the app's shared keyboard/scroll navigation, so overlay
/// list states don't each re-wire selection and offset handling. The item count
/// (`len`) is supplied per call because it lives on the owning state (a filtered
/// view for tags, the full vocabulary for feelings).
#[derive(Default)]
pub(crate) struct SelectableList {
    state: ListState,
}

impl SelectableList {
    pub(crate) fn selected(&self) -> Option<usize> {
        self.state.selected()
    }

    pub(crate) fn offset(&self) -> usize {
        self.state.offset()
    }

    pub(crate) fn set_offset(&mut self, offset: usize) {
        *self.state.offset_mut() = offset;
    }

    pub(crate) fn normalize(&mut self, len: usize) {
        normalize_list_state(&mut self.state, len);
    }

    pub(crate) fn select(&mut self, index: usize, len: usize) {
        if index < len {
            self.state.select(Some(index));
        }
    }

    pub(crate) fn move_by(&mut self, len: usize, delta: isize) {
        move_list_selection(&mut self.state, len, delta);
    }

    pub(crate) fn scroll_by(&mut self, delta: i16, len: usize, viewport_height: u16) {
        scroll_list_offset(&mut self.state, delta, len, viewport_height);
    }

    pub(crate) fn ensure_visible(&mut self, len: usize, viewport_height: u16) {
        ensure_selected_visible(&mut self.state, len, viewport_height);
    }
}

/// Keyboard/scroll navigation shared by the overlay list states. An implementor
/// exposes its [`SelectableList`] and current item count; the navigation methods
/// come for free, so `EditMetadataState` and `EditFeelingState` don't each re-forward
/// them with their own length source.
pub(crate) trait ListNav {
    fn list(&self) -> &SelectableList;
    fn list_mut(&mut self) -> &mut SelectableList;
    fn item_count(&self) -> usize;

    fn selected_index(&self) -> Option<usize> {
        self.list().selected()
    }

    fn offset(&self) -> usize {
        self.list().offset()
    }

    fn normalize_list_state(&mut self) {
        let len = self.item_count();
        self.list_mut().normalize(len);
    }

    fn select_index(&mut self, index: usize) {
        let len = self.item_count();
        self.list_mut().select(index, len);
    }

    fn move_up(&mut self) {
        let len = self.item_count();
        self.list_mut().move_by(len, -1);
    }

    fn move_down(&mut self) {
        let len = self.item_count();
        self.list_mut().move_by(len, 1);
    }

    fn scroll_by(&mut self, delta: i16, viewport_height: u16) {
        let len = self.item_count();
        self.list_mut().scroll_by(delta, len, viewport_height);
    }

    fn ensure_selected_visible(&mut self, viewport_height: u16) {
        let len = self.item_count();
        self.list_mut().ensure_visible(len, viewport_height);
    }
}

/// Which part of the metadata edit dialog has keyboard focus.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditMetadataFocus {
    #[default]
    List,
    Input,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MetadataKind {
    Tags,
    People,
    Activities,
}

impl MetadataKind {
    pub(crate) fn title(self) -> &'static str {
        match self {
            MetadataKind::Tags => "Tags",
            MetadataKind::People => "People",
            MetadataKind::Activities => "Activities",
        }
    }

    pub(crate) fn value_name(self) -> &'static str {
        match self {
            MetadataKind::Tags => "tag",
            MetadataKind::People => "person",
            MetadataKind::Activities => "activity",
        }
    }

    pub(crate) fn search_prefix(self) -> &'static str {
        match self {
            MetadataKind::Tags => "tags",
            MetadataKind::People => "people",
            MetadataKind::Activities => "activities",
        }
    }
}

/// State for the free-form metadata overlay.
pub(crate) struct EditMetadataState {
    pub(crate) kind: MetadataKind,
    /// The offerable values: active-journal values first (indices `0..active_len`),
    /// then archived-only values, each sorted by usage count descending.
    pub(crate) all_values: Vec<(String, usize)>,
    /// How many leading `all_values` come from active journals. Archived-only
    /// values (the rest) are shown only when the filter query matches them.
    pub(crate) active_len: usize,
    /// Indices into `all_values` that match the current filter input.
    pub(crate) filtered: Vec<usize>,
    /// Values currently selected for the entry (lowercased for look-up).
    pub(crate) selected: Vec<String>,
    /// Stateful list selection and scroll offset.
    pub(crate) list: SelectableList,
    /// Text input for filtering values and adding new ones.
    pub(crate) input: String,
    /// Whether keyboard events go to the list or to the input.
    pub(crate) focus: EditMetadataFocus,
}

impl EditMetadataState {
    pub(crate) fn new(
        kind: MetadataKind,
        all_values: Vec<(String, usize)>,
        filtered: Vec<usize>,
        selected: Vec<String>,
        active_len: usize,
    ) -> Self {
        let mut state = Self {
            kind,
            all_values,
            active_len,
            filtered,
            selected,
            list: SelectableList::default(),
            input: String::new(),
            focus: EditMetadataFocus::List,
        };
        state.normalize_list_state();
        state
    }

    pub(crate) fn rebuild_filter(&mut self) {
        let query = self.input.to_lowercase();
        // With no query, offer only the active-journal values. Once the user types,
        // match across everything — including archived-only values — so they can
        // reuse an existing archived tag instead of creating a near-duplicate.
        let search_range = if query.is_empty() {
            0..self.active_len
        } else {
            0..self.all_values.len()
        };
        self.filtered = search_range
            .filter(|&i| self.all_values[i].0.to_lowercase().contains(&query))
            .collect();
        self.list.set_offset(0);
        self.normalize_list_state();
    }

    pub(crate) fn selected_value_index(&self) -> Option<usize> {
        self.selected_index()
            .and_then(|index| self.filtered.get(index).copied())
    }

    pub(crate) fn toggle_selected(&mut self) {
        if let Some(tag_idx) = self.selected_value_index() {
            let tag = self.all_values[tag_idx].0.to_lowercase();
            if let Some(pos) = self.selected.iter().position(|t| t == &tag) {
                self.selected.remove(pos);
            } else {
                self.selected.push(tag);
            }
        }
    }
}

impl ListNav for EditMetadataState {
    fn list(&self) -> &SelectableList {
        &self.list
    }

    fn list_mut(&mut self) -> &mut SelectableList {
        &mut self.list
    }

    fn item_count(&self) -> usize {
        self.filtered.len()
    }
}

/// State for the edit-feelings overlay.
pub(crate) struct EditFeelingState {
    /// Fixed feelings vocabulary in display order.
    pub(crate) all_feelings: Vec<String>,
    /// Feelings currently selected for the entry.
    pub(crate) selected: Vec<String>,
    /// Stateful list selection and scroll offset.
    pub(crate) list: SelectableList,
}

impl EditFeelingState {
    pub(crate) fn new(all_feelings: Vec<String>, selected: Vec<String>) -> Self {
        let mut state = Self {
            all_feelings,
            selected,
            list: SelectableList::default(),
        };
        state.normalize_list_state();
        state
    }

    pub(crate) fn toggle_selected(&mut self) {
        if let Some(index) = self.selected_index() {
            let feeling = self.all_feelings[index].clone();
            if let Some(pos) = self.selected.iter().position(|v| v == &feeling) {
                self.selected.remove(pos);
            } else {
                self.selected.push(feeling);
            }
        }
    }
}

impl ListNav for EditFeelingState {
    fn list(&self) -> &SelectableList {
        &self.list
    }

    fn list_mut(&mut self) -> &mut SelectableList {
        &mut self.list
    }

    fn item_count(&self) -> usize {
        self.all_feelings.len()
    }
}

pub(crate) fn normalize_list_state(state: &mut ListState, len: usize) {
    if len == 0 {
        state.select(None);
        return;
    }

    let selected = state.selected().unwrap_or(0).min(len - 1);
    state.select(Some(selected));
    if state.offset() >= len {
        *state.offset_mut() = len - 1;
    }
}

pub(crate) fn move_list_selection(state: &mut ListState, len: usize, delta: isize) {
    if len == 0 {
        state.select(None);
        return;
    }

    let selected = state.selected().unwrap_or(0);
    let next = (selected as isize + delta).clamp(0, len as isize - 1) as usize;
    state.select(Some(next));
}

pub(crate) fn scroll_list_offset(
    state: &mut ListState,
    delta: i16,
    len: usize,
    viewport_height: u16,
) {
    if len == 0 || viewport_height == 0 {
        *state.offset_mut() = 0;
        return;
    }
    // Item-index space here (`len` items, one row each), but the clamp is the same
    // shape as the pixel lists', so share it.
    *state.offset_mut() =
        crate::tui::scroll::scroll_pixels(state.offset(), delta, len, viewport_height);
}

pub(crate) fn ensure_selected_visible(state: &mut ListState, len: usize, viewport_height: u16) {
    if len == 0 || viewport_height == 0 {
        *state.offset_mut() = 0;
        return;
    }

    let Some(selected) = state.selected().map(|index| index.min(len - 1)) else {
        return;
    };
    let viewport_height = viewport_height as usize;
    let offset = state.offset();
    let max_offset = len.saturating_sub(viewport_height);
    let next_offset = if selected < offset {
        selected
    } else if selected >= offset.saturating_add(viewport_height) {
        selected.saturating_add(1).saturating_sub(viewport_height)
    } else {
        offset
    };

    *state.offset_mut() = next_offset.min(max_offset);
}

/// State for the edit-mood overlay.
pub(crate) struct EditMoodState {
    /// The mood score currently saved on the entry (None = not set).
    pub(crate) saved: Option<i8>,
    /// The score being edited (-5..=5).
    pub(crate) draft: i8,
}

/// Fullscreen image viewer overlay: the entry's images in body order and the
/// one currently shown.
pub(crate) struct ImageViewerState {
    pub(crate) assets: Vec<ImageAsset>,
    pub(crate) index: usize,
}

pub(crate) enum DeleteContext {
    Entry {
        has_body: bool,
    },
    Journal {
        name: String,
        trash_count: usize,
        delete_count: usize,
    },
}

/// The single modal overlay that can be active over the browse view. Making
/// this an enum keeps the modals mutually exclusive by construction.
#[derive(Default)]
pub(crate) enum Overlay {
    #[default]
    None,
    ConfirmDelete(DeleteContext),
    NewJournal(String),
    EditMetadata(EditMetadataState),
    EditFeelings(EditFeelingState),
    EditMood(EditMoodState),
    ImageViewer(ImageViewerState),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag_state(count: usize) -> EditMetadataState {
        let all_values: Vec<(String, usize)> = (0..count)
            .map(|index| (format!("tag-{index:02}"), index))
            .collect();
        let filtered: Vec<usize> = (0..count).collect();
        EditMetadataState::new(MetadataKind::Tags, all_values, filtered, Vec::new(), count)
    }

    #[test]
    fn tag_keyboard_selection_scrolls_down_to_remain_visible() {
        let mut state = tag_state(10);

        for _ in 0..5 {
            state.move_down();
            state.ensure_selected_visible(4);
        }

        assert_eq!(state.selected_index(), Some(5));
        assert_eq!(state.offset(), 2);
    }

    #[test]
    fn tag_keyboard_selection_scrolls_up_to_remain_visible() {
        let mut state = tag_state(10);
        state.select_index(5);
        state.list.set_offset(5);

        state.move_up();
        state.ensure_selected_visible(4);

        assert_eq!(state.selected_index(), Some(4));
        assert_eq!(state.offset(), 4);
    }

    #[test]
    fn filter_hides_archived_only_values_until_query_matches() {
        // One active value (index 0) and one archived-only value (index 1).
        let all_values = vec![("berlin".to_string(), 3), ("wanderlust".to_string(), 5)];
        let mut state =
            EditMetadataState::new(MetadataKind::Tags, all_values, vec![0], Vec::new(), 1);

        // With no query only the active value is offered.
        assert_eq!(state.filtered, vec![0]);

        // Typing part of the archived-only value surfaces it (so the user reuses
        // it instead of creating a near-duplicate).
        state.input = "wan".to_string();
        state.rebuild_filter();
        assert_eq!(state.filtered, vec![1]);

        // Clearing the query hides it again.
        state.input.clear();
        state.rebuild_filter();
        assert_eq!(state.filtered, vec![0]);
    }
}
