use super::*;

impl App {
    pub(crate) fn selected_journal_index(&self) -> usize {
        self.nav.journal_list.selected().unwrap_or(0)
    }

    pub(crate) fn selected_journal(&self) -> Option<&Journal> {
        self.library.journals.get(self.selected_journal_index())
    }

    /// The preview pane shows journal stats (instead of an entry) when browsing
    /// with no entry selected.
    pub(crate) fn show_journal_stats_preview(&self) -> bool {
        self.nav.mode == Mode::Browse && self.nav.selected_entry_index.is_none()
    }

    /// Whether the entries list should draw a highlighted selection row.
    pub(crate) fn entries_highlighted(&self) -> bool {
        self.nav.focus != Focus::Journals && self.nav.selected_entry_index.is_some()
    }

    pub(crate) fn journal_list_ensure_visible(&mut self, viewport_height: u16) {
        ensure_selected_visible(
            &mut self.nav.journal_list,
            self.library.journals.len(),
            viewport_height,
        );
    }

    pub(crate) fn scroll_journal_list(&mut self, delta: i16, viewport_height: u16) {
        scroll_list_offset(
            &mut self.nav.journal_list,
            delta,
            self.library.journals.len(),
            viewport_height,
        );
    }

    pub(super) fn reset_entry_scroll(&mut self) {
        *self.nav.entry_list.offset_mut() = 0;
        self.nav.scroll.reset_entry_view();
    }

    pub(crate) fn scroll_entry_list(
        &mut self,
        delta: i16,
        total_height: usize,
        viewport_height: u16,
    ) {
        let max = total_height.saturating_sub(viewport_height as usize);
        let offset = if delta < 0 {
            self.nav
                .entry_list
                .offset()
                .saturating_sub(delta.unsigned_abs() as usize)
        } else {
            self.nav.entry_list.offset().saturating_add(delta as usize)
        };
        *self.nav.entry_list.offset_mut() = offset.min(max);
    }

    pub(crate) fn entry_list_ensure_visible(
        &mut self,
        rows: &[EntryRowMeta],
        viewport_height: u16,
    ) {
        let mut scroll = self.nav.entry_list.offset();
        crate::tui::entry_rows::ensure_entry_visible(
            &mut scroll,
            rows,
            self.nav.selected_entry_index,
            viewport_height,
        );
        *self.nav.entry_list.offset_mut() = scroll;
    }

    /// Contiguous index range into `entries` for the selected journal, or `None`
    /// when no journal is selected or it has no entries.
    fn selected_entry_range(&self) -> Option<Range<usize>> {
        let journal = self.selected_journal()?;
        self.library.range(&journal.name)
    }

    pub(crate) fn selected_entries(&self) -> Vec<&Entry> {
        match self.selected_entry_range() {
            Some(range) => self.library.entries[range].iter().collect(),
            None => Vec::new(),
        }
    }

    pub(crate) fn current_entry_list_len(&self) -> usize {
        match self.nav.mode {
            Mode::Search => self.search.hits.len(),
            Mode::Browse => self.selected_entry_range().map_or(0, |range| range.len()),
        }
    }

    /// The entry backing the current selection, resolving a search hit through
    /// the id index. Unifies the Search/Browse branches the preview getters share.
    pub(super) fn resolved_selected_entry(&self) -> Option<&Entry> {
        match self.nav.mode {
            Mode::Search => self.library.entry_by_id(&self.selected_search_hit()?.id),
            Mode::Browse => self.selected_entry(),
        }
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let len = match self.nav.focus {
            Focus::Journals if self.nav.mode == Mode::Browse => self.library.journals.len(),
            Focus::Entries | Focus::EntryView | Focus::Journals => self.current_entry_list_len(),
        };
        if len == 0 {
            return;
        }

        let previous_entry_index = self.nav.selected_entry_index;
        if self.nav.focus == Focus::Journals && self.nav.mode == Mode::Browse {
            move_list_selection(&mut self.nav.journal_list, len, delta);
            self.nav.selected_entry_index = Some(0);
            *self.nav.entry_list.offset_mut() = 0;
        } else {
            match self.nav.selected_entry_index {
                // Deselected (Browse shows journal stats): a downward move selects
                // the first entry; an upward move stays on the stats view.
                None if self.nav.mode == Mode::Browse => {
                    if delta > 0 {
                        self.nav.selected_entry_index = Some(0);
                    }
                }
                // Scrolling up past the first entry deselects, revealing journal stats.
                Some(0) if self.nav.mode == Mode::Browse && delta < 0 => {
                    self.nav.selected_entry_index = None;
                }
                current => {
                    let base = current.unwrap_or(0) as isize;
                    let next = (base + delta).clamp(0, len as isize - 1) as usize;
                    self.nav.selected_entry_index = Some(next);
                }
            }
        }
        if self.nav.selected_entry_index != previous_entry_index {
            self.nav.scroll.entry_view = 0;
        }
    }

    pub(crate) fn select_journal(&mut self, index: usize) {
        if index >= self.library.journals.len() {
            return;
        }

        if self.selected_journal_index() != index {
            self.nav.journal_list.select(Some(index));
            self.nav.selected_entry_index = Some(0);
            self.reset_entry_scroll();
        }
    }

    pub(crate) fn select_entry_index(&mut self, index: usize) {
        if index >= self.current_entry_list_len() {
            return;
        }

        if self.nav.selected_entry_index != Some(index) {
            self.nav.selected_entry_index = Some(index);
            self.nav.scroll.entry_view = 0;
        }
    }

    pub(crate) fn select_entry_by_id(&mut self, id: &str, reset_entry_scroll: bool) -> bool {
        let index = match self.nav.mode {
            Mode::Search => self.search.hits.iter().position(|hit| hit.id == id),
            Mode::Browse => self.journal_name_for_entry_id(id).and_then(|journal_name| {
                self.library
                    .entries
                    .iter()
                    .filter(|entry| entry.journal == journal_name)
                    .position(|entry| entry.id == id)
            }),
        };
        let Some(index) = index else { return false };

        if self.nav.selected_entry_index != Some(index) {
            self.nav.selected_entry_index = Some(index);
        }
        if reset_entry_scroll {
            self.nav.scroll.entry_view = 0;
        }
        true
    }

    fn journal_name_for_entry_id(&mut self, id: &str) -> Option<String> {
        let journal_name = self
            .library
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.journal.clone())?;
        let journal_index = self
            .library
            .journals
            .iter()
            .position(|journal| journal.name == journal_name)?;
        if self.selected_journal_index() != journal_index {
            self.nav.journal_list.select(Some(journal_index));
            *self.nav.entry_list.offset_mut() = 0;
        }
        Some(journal_name)
    }

    pub(super) fn selected_entry(&self) -> Option<&Entry> {
        let index = self.nav.selected_entry_index?;
        let range = self.selected_entry_range()?;
        (index < range.len()).then(|| &self.library.entries[range.start + index])
    }

    pub(crate) fn selected_search_hit(&self) -> Option<&SearchHit> {
        self.search.hits.get(self.nav.selected_entry_index?)
    }

    pub(crate) fn selected_entry_target(&self) -> Option<EntryTarget> {
        // In Search mode the title comes from the hit (journal-prefixed label),
        // otherwise from the entry itself; the rest is shared.
        let title = match self.nav.mode {
            Mode::Search => self.search_hit_label(self.selected_search_hit()?),
            Mode::Browse => self.selected_entry()?.display_label(),
        };
        let entry = self.resolved_selected_entry()?;
        Some(EntryTarget {
            id: entry.id.clone(),
            path: entry.path.clone(),
            title,
            locked: entry.encryption_state == EntryEncryptionState::EncryptedLocked,
        })
    }

    pub(crate) fn selected_entry_tags(&self) -> Vec<String> {
        self.selected_entry_metadata(MetadataKind::Tags)
    }

    pub(crate) fn selected_entry_people(&self) -> Vec<String> {
        self.selected_entry_metadata(MetadataKind::People)
    }

    pub(crate) fn selected_entry_activities(&self) -> Vec<String> {
        self.selected_entry_metadata(MetadataKind::Activities)
    }

    pub(super) fn selected_entry_metadata(&self, kind: MetadataKind) -> Vec<String> {
        self.resolved_selected_entry()
            .map(|entry| metadata_values(entry, kind).to_vec())
            .unwrap_or_default()
    }

    pub(crate) fn selected_entry_feelings(&self) -> Vec<String> {
        self.resolved_selected_entry()
            .map(|entry| entry.metadata.feelings.clone())
            .unwrap_or_default()
    }

    pub(crate) fn has_selected_entry_target(&self) -> bool {
        self.selected_entry_target().is_some()
    }

    pub(crate) fn can_act_on_selected_entry(&self) -> bool {
        matches!(self.nav.focus, Focus::Entries | Focus::EntryView)
            && self.has_selected_entry_target()
    }

    pub(crate) fn selected_entry_view(&self) -> Option<(String, String)> {
        let entry = self.resolved_selected_entry()?;
        if entry.encryption_state == EntryEncryptionState::EncryptedLocked {
            return Some((
                entry_timestamp_label(entry),
                "Encryption identity not available".to_string(),
            ));
        }
        Some((entry_timestamp_label(entry), entry.content.clone()))
    }

    pub(crate) fn select_journal_by_name(&mut self, name: &str) {
        if let Some(index) = self
            .library
            .journals
            .iter()
            .position(|journal| journal.name == name)
        {
            self.nav.journal_list.select(Some(index));
            *self.nav.journal_list.offset_mut() = index;
            self.nav.selected_entry_index = Some(0);
            self.reset_entry_scroll();
            self.nav.focus = Focus::Entries;
        }
    }
}
