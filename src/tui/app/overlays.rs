use super::*;

impl App {
    pub(crate) fn begin_new_journal_input(&mut self) {
        self.overlay = Overlay::NewJournal(String::new());
        self.clear_status();
    }

    pub(crate) fn new_journal_input(&self) -> Option<&str> {
        match &self.overlay {
            Overlay::NewJournal(name) => Some(name),
            _ => None,
        }
    }

    pub(crate) fn new_journal_input_mut(&mut self) -> Option<&mut String> {
        match &mut self.overlay {
            Overlay::NewJournal(name) => Some(name),
            _ => None,
        }
    }

    pub(crate) fn edit_metadata_state(&self) -> Option<&EditMetadataState> {
        match &self.overlay {
            Overlay::EditMetadata(state) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn edit_metadata_state_mut(&mut self) -> Option<&mut EditMetadataState> {
        match &mut self.overlay {
            Overlay::EditMetadata(state) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn edit_feeling_state(&self) -> Option<&EditFeelingState> {
        match &self.overlay {
            Overlay::EditFeelings(state) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn edit_feeling_state_mut(&mut self) -> Option<&mut EditFeelingState> {
        match &mut self.overlay {
            Overlay::EditFeelings(state) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn selected_entry_mood(&self) -> Option<i8> {
        self.resolved_selected_entry()
            .and_then(|entry| entry.metadata.mood)
    }

    pub(crate) fn begin_edit_mood(&mut self) {
        let saved = self.selected_entry_mood();
        let draft = saved.unwrap_or(0);
        self.overlay = Overlay::EditMood(EditMoodState { saved, draft });
    }

    pub(crate) fn edit_mood_state(&self) -> Option<&EditMoodState> {
        match &self.overlay {
            Overlay::EditMood(state) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn edit_mood_state_mut(&mut self) -> Option<&mut EditMoodState> {
        match &mut self.overlay {
            Overlay::EditMood(state) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn begin_confirm_delete(&mut self) {
        match self.nav.focus {
            Focus::Journals => self.begin_confirm_delete_journal(),
            Focus::Entries | Focus::EntryView => self.begin_confirm_delete_entry(),
        }
    }

    fn begin_confirm_delete_entry(&mut self) {
        let has_body = self
            .selected_entry()
            .map(|e| !e.content.trim().is_empty())
            .unwrap_or(false);
        self.overlay = Overlay::ConfirmDelete(DeleteContext::Entry { has_body });
    }

    fn begin_confirm_delete_journal(&mut self) {
        let Some(journal) = self.selected_journal() else {
            return;
        };
        let name = journal.name.clone();
        let trash_count = self
            .library
            .entries
            .iter()
            .filter(|e| e.journal == name && !e.content.trim().is_empty())
            .count();
        let delete_count = self
            .library
            .entries
            .iter()
            .filter(|e| e.journal == name && e.content.trim().is_empty())
            .count();
        self.overlay = Overlay::ConfirmDelete(DeleteContext::Journal {
            name,
            trash_count,
            delete_count,
        });
    }

    pub(crate) fn has_overlay(&self) -> bool {
        !matches!(self.overlay, Overlay::None)
    }

    pub(crate) fn close_overlay(&mut self) {
        // Cache is scoped to the entry-viewing session, not the viewer overlay
        // (see `sync_image_warm`), so reopening within the same entry stays warm.
        self.overlay = Overlay::None;
    }
}
