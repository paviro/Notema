use super::*;

impl App {
    /// Collect metadata values across every loaded entry, sorted by usage count
    /// (most frequent first) and then alphabetically. Values differing only in
    /// case are consolidated: the most common casing wins (ties go to the
    /// first alphabetically).
    pub(crate) fn all_metadata_sorted(&self, kind: MetadataKind) -> Vec<(String, usize)> {
        // First pass — count per lowercased key, track casing frequency.
        let mut lower_to_casing: std::collections::BTreeMap<String, CasingCount> =
            std::collections::BTreeMap::new();
        for entry in &self.library.entries {
            for value in metadata_values(entry, kind) {
                let lower = value.to_lowercase();
                let entry = lower_to_casing.entry(lower).or_default();
                entry.total += 1;
                *entry.forms.entry(value.clone()).or_default() += 1;
            }
        }
        let mut pairs: Vec<_> = lower_to_casing
            .into_values()
            .map(|cc| {
                // Pick the casing form with the highest frequency; ties → first alphabetically.
                let display = cc
                    .forms
                    .into_iter()
                    .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
                    .map(|(form, _)| form)
                    .unwrap_or_default();
                (display, cc.total)
            })
            .collect();
        pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        pairs
    }

    pub(crate) fn begin_edit_tags(&mut self) {
        self.begin_edit_metadata(MetadataKind::Tags);
    }

    pub(crate) fn begin_edit_people(&mut self) {
        self.begin_edit_metadata(MetadataKind::People);
    }

    pub(crate) fn begin_edit_activities(&mut self) {
        self.begin_edit_metadata(MetadataKind::Activities);
    }

    fn begin_edit_metadata(&mut self, kind: MetadataKind) {
        let all_values = self.all_metadata_sorted(kind);
        let filtered: Vec<usize> = (0..all_values.len()).collect();
        let entry_tags: Vec<String> = self
            .selected_entry_metadata(kind)
            .into_iter()
            .map(|t| t.to_lowercase())
            .collect();
        self.overlay = Overlay::EditMetadata(EditMetadataState::new(
            kind, all_values, filtered, entry_tags,
        ));
    }

    pub(crate) fn begin_edit_feelings(&mut self) {
        let selected = self.selected_entry_feelings();
        self.overlay = Overlay::EditFeelings(EditFeelingState::new(
            FEELINGS.iter().map(|feeling| feeling.to_string()).collect(),
            selected,
        ));
    }

    pub(crate) fn begin_tag_search(&mut self, tag: &str) {
        self.begin_metadata_search(MetadataKind::Tags, tag);
    }

    pub(crate) fn begin_people_search(&mut self, person: &str) {
        self.begin_metadata_search(MetadataKind::People, person);
    }

    pub(crate) fn begin_activity_search(&mut self, activity: &str) {
        self.begin_metadata_search(MetadataKind::Activities, activity);
    }

    fn begin_metadata_search(&mut self, kind: MetadataKind, value: &str) {
        let scope = self.current_journal_scope();
        let hits = self.search_results_by_metadata(kind, value);
        self.enter_search(scope, format!("{}:{value}", kind.search_prefix()), hits);
    }

    pub(crate) fn begin_feeling_search(&mut self, feeling: &str) {
        let scope = self.current_journal_scope();
        let hits = self.search_results_by_feeling(feeling);
        self.enter_search(scope, format!("feelings:{feeling}"), hits);
    }
}
