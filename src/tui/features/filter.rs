//! The filter browser: a tabbed overlay that lists every distinct
//! tag/person/activity/feeling/place value in scope with its post
//! count, and launches a filtered search on the chosen row.
//!
//! Scope is captured when the dialog opens — all journals from the journals
//! column, the selected journal from the entries column — and drives both the
//! listed counts and the launched search. Each row's count is computed through
//! the *same* predicate the launched search uses (via [`AppModel::search_results`]
//! at launch), so the number shown on a row always equals the number of results
//! the search returns.

use std::collections::BTreeSet;

use notema_domain::{Entry, Location, PlaceGroup};

use crate::tui::app::{AppModel, Focus, SearchScope};
use crate::tui::features::metadata::count_metadata;
use crate::tui::features::search::{
    entry_in_search_scope, feeling_predicate, location_predicate, metadata_predicate,
};
use crate::tui::render::tab_strip::StripTab;
use crate::tui::state::{FilterTab, ListNav, MetadataKind, Overlay, SelectableList};

/// One row of a filter tab: its display `label`, the search payload it
/// launches (`search_value`, placed after the tab's prefix), and its post count.
pub(crate) struct FilterRow {
    pub(crate) search_value: String,
    pub(crate) label: String,
    pub(crate) count: usize,
}

/// State for the filter overlay: the captured scope, the active tab, each
/// tab's rows (built once at open), and the selection/scroll of the active tab.
pub(crate) struct FilterState {
    /// Captured at open — drives the listed counts *and* the launched search.
    pub(crate) scope: SearchScope,
    pub(crate) tab: FilterTab,
    /// One entry per [`FilterTab::ALL`], indexed by [`FilterTab::index`].
    pub(crate) rows: [Vec<FilterRow>; FilterTab::COUNT],
    pub(crate) list: SelectableList,
}

impl FilterState {
    fn new(scope: SearchScope, rows: [Vec<FilterRow>; FilterTab::COUNT]) -> Self {
        // Open on the first tab that has anything to show, so the dialog never
        // greets the user with an empty list when other tabs are populated.
        let tab = FilterTab::ALL
            .into_iter()
            .find(|tab| !rows[tab.index()].is_empty())
            .unwrap_or_default();
        let mut state = Self {
            scope,
            tab,
            rows,
            list: SelectableList::default(),
        };
        state.set_tab(tab);
        state
    }

    pub(crate) fn current_rows(&self) -> &[FilterRow] {
        &self.rows[self.tab.index()]
    }

    pub(crate) fn selected_row(&self) -> Option<&FilterRow> {
        self.selected_index()
            .and_then(|index| self.current_rows().get(index))
    }

    /// Switch to `tab`, resetting the selection to its first row.
    pub(crate) fn set_tab(&mut self, tab: FilterTab) {
        self.tab = tab;
        self.list = SelectableList::default();
        self.normalize_list_state();
        self.select_index(0);
    }
}

impl ListNav for FilterState {
    fn list(&self) -> &SelectableList {
        &self.list
    }

    fn list_mut(&mut self) -> &mut SelectableList {
        &mut self.list
    }

    fn item_count(&self) -> usize {
        self.current_rows().len()
    }
}

/// Sort rows by count descending, breaking ties by label ascending.
fn sort_by_count(rows: &mut [FilterRow]) {
    rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));
}

impl AppModel {
    pub(crate) fn begin_filter(&mut self) {
        let scope = if self.nav.focus == Focus::Journals {
            SearchScope::AllJournals
        } else {
            self.current_journal_scope()
        };
        let rows: [Vec<FilterRow>; FilterTab::COUNT] =
            std::array::from_fn(|i| self.filter_rows(&scope, FilterTab::ALL[i]));
        self.overlay = Overlay::Filter(Box::new(FilterState::new(scope, rows)));
    }

    pub(crate) fn filter_state(&self) -> Option<&FilterState> {
        match &self.overlay {
            Overlay::Filter(state) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn filter_state_mut(&mut self) -> Option<&mut FilterState> {
        match &mut self.overlay {
            Overlay::Filter(state) => Some(state),
            _ => None,
        }
    }

    /// The unlocked, in-scope entries a search under `scope` would see — the same
    /// set [`search_results_matching`](Self::search_results_matching) filters to.
    fn scoped_entries<'a>(
        &'a self,
        scope: &'a SearchScope,
    ) -> impl Iterator<Item = &'a Entry> + 'a {
        self.library
            .entries
            .iter()
            .filter(move |entry| entry_in_search_scope(entry, scope))
    }

    pub(crate) fn filter_rows(&self, scope: &SearchScope, tab: FilterTab) -> Vec<FilterRow> {
        match tab {
            FilterTab::Tags => self.metadata_filter_rows(scope, MetadataKind::Tags),
            FilterTab::People => self.metadata_filter_rows(scope, MetadataKind::People),
            FilterTab::Activities => self.metadata_filter_rows(scope, MetadataKind::Activities),
            FilterTab::Feelings => self.feeling_filter_rows(scope),
            FilterTab::Locations => self.location_filter_rows(scope),
        }
    }

    fn metadata_filter_rows(&self, scope: &SearchScope, kind: MetadataKind) -> Vec<FilterRow> {
        // The distinct display values (nicely cased); their counts are then
        // recomputed with the substring predicate the launched `tags:`/`people:`/
        // `activities:` search uses, so each row's count equals its result count.
        let mut rows: Vec<FilterRow> = count_metadata(self.scoped_entries(scope), kind)
            .into_iter()
            .map(|(label, _)| {
                let matches = metadata_predicate(kind, &label);
                let count = self.scoped_entries(scope).filter(|e| matches(e)).count();
                FilterRow {
                    search_value: label.clone(),
                    label,
                    count,
                }
            })
            .collect();
        sort_by_count(&mut rows);
        rows
    }

    fn feeling_filter_rows(&self, scope: &SearchScope) -> Vec<FilterRow> {
        let mut distinct: BTreeSet<String> = BTreeSet::new();
        for entry in self.scoped_entries(scope) {
            distinct.extend(entry.feelings.iter().cloned());
        }
        let mut rows: Vec<FilterRow> = distinct
            .into_iter()
            .map(|feeling| {
                let matches = feeling_predicate(&feeling);
                let count = self.scoped_entries(scope).filter(|e| matches(e)).count();
                FilterRow {
                    search_value: feeling.clone(),
                    label: feeling,
                    count,
                }
            })
            .collect();
        sort_by_count(&mut rows);
        rows
    }

    fn location_filter_rows(&self, scope: &SearchScope) -> Vec<FilterRow> {
        // A row shows the bucket's display label but launches — and counts through —
        // its search query, so the count equals the search's results.
        let mut groups: BTreeSet<PlaceGroup> = BTreeSet::new();
        for entry in self.scoped_entries(scope) {
            if let Some(group) = entry.location.as_ref().and_then(Location::place_group) {
                groups.insert(group);
            }
        }
        let mut rows: Vec<FilterRow> = groups
            .into_iter()
            .map(|group| {
                let search_value = group.search_query();
                let matches = location_predicate(&search_value);
                let count = self.scoped_entries(scope).filter(|e| matches(e)).count();
                FilterRow {
                    label: group.display_label(),
                    search_value,
                    count,
                }
            })
            .collect();
        sort_by_count(&mut rows);
        rows
    }

    /// Close the filter browser and run the search for the highlighted row under
    /// the captured scope.
    pub(crate) fn launch_filter_search(&mut self) {
        let Some(state) = self.filter_state() else {
            return;
        };
        let Some(row) = state.selected_row() else {
            return;
        };
        let scope = state.scope.clone();
        let query = format!("{}:{}", state.tab.search_prefix(), row.search_value);
        self.close_overlay();
        // `search_results` parses the query box and reads `self.search.scope`, so
        // set both to the captured values before computing hits; `enter_search`
        // then re-sets them (idempotent) and finishes entering search mode.
        self.search.scope = scope.clone();
        self.search.query.set_text(&query);
        let hits = self.search_results();
        self.enter_search(scope, query, hits);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notema_domain::{EntryEncryptionState, Location};

    fn entry(journal: &str, configure: impl FnOnce(&mut Entry)) -> Entry {
        let mut entry = Entry {
            id: "id".to_string(),
            journal: journal.to_string(),
            path: std::path::PathBuf::from(format!("{journal}/e.md")),
            encryption_state: EntryEncryptionState::Plain,
            created_at: None,
            edited_at: None,
            preview: String::new(),
            activities: Vec::new(),
            feelings: Vec::new(),
            people: Vec::new(),
            tags: Vec::new(),
            mood: None,
            starred: false,
            location: None,
            weather: None,
            celestial: None,
            air_quality: None,
            import: None,
            body: String::new(),
            word_count: 0,
            search_haystack: String::new(),
            warning: None,
        };
        configure(&mut entry);
        entry
    }

    fn place(city: &str, country: &str) -> Location {
        Location {
            city: Some(city.to_string()),
            country: Some(country.to_string()),
            ..Location::default()
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    /// A fixture spanning two journals with every facet populated, plus one locked
    /// entry (whose "secret" tag must never surface).
    fn fixture_app() -> AppModel {
        let entries = vec![
            entry("work", |e| {
                e.tags = strings(&["berlin", "work"]);
                e.people = strings(&["Alice"]);
                e.activities = strings(&["coding"]);
                e.feelings = strings(&["happy"]);
                e.mood = Some(3);
                e.starred = true;
                e.location = Some(place("Berlin", "Germany"));
            }),
            entry("work", |e| {
                e.tags = strings(&["Berlin"]);
                e.feelings = strings(&["happy", "calm"]);
                e.mood = Some(3);
                e.location = Some(place("Berlin", "Germany"));
            }),
            entry("trips", |e| {
                e.tags = strings(&["berlin"]);
                e.people = strings(&["alice", "Bob"]);
                e.activities = strings(&["hiking"]);
                e.feelings = strings(&["calm"]);
                e.mood = Some(-2);
                e.starred = true;
                e.location = Some(place("Paris", "France"));
            }),
            entry("work", |e| {
                e.encryption_state = EntryEncryptionState::EncryptedLocked;
                e.tags = strings(&["secret"]);
                e.mood = Some(5);
                e.location = Some(place("Hidden", "Nowhere"));
            }),
        ];
        let mut app = crate::tui::test_support::app_with_journals(&[]);
        app.library.entries = entries;
        app
    }

    /// Run the search a row would launch and return its hit count.
    fn search_count(app: &mut AppModel, scope: &SearchScope, tab: FilterTab, value: &str) -> usize {
        app.search.scope = scope.clone();
        app.search
            .query
            .set_text(&format!("{}:{}", tab.search_prefix(), value));
        app.search_results().len()
    }

    #[test]
    fn row_counts_equal_search_results_for_every_facet() {
        let mut app = fixture_app();
        for scope in [
            SearchScope::AllJournals,
            SearchScope::Journal("work".to_string()),
        ] {
            for tab in FilterTab::ALL {
                for row in app.filter_rows(&scope, tab) {
                    let hits = search_count(&mut app, &scope, tab, &row.search_value);
                    assert_eq!(
                        row.count, hits,
                        "{tab:?} row {:?} under {scope:?}: count {} != results {hits}",
                        row.search_value, row.count
                    );
                }
            }
        }
    }

    #[test]
    fn all_journals_scope_counts_across_journals() {
        let app = fixture_app();
        let tags = app.filter_rows(&SearchScope::AllJournals, FilterTab::Tags);
        // "berlin" is tagged on all three unlocked entries (two journals), folding
        // "Berlin"/"berlin" together; the most-frequent lowercase form wins.
        let berlin = tags
            .iter()
            .find(|r| r.label == "berlin")
            .expect("berlin row");
        assert_eq!(berlin.count, 3);
        // The locked entry's "secret" tag never surfaces.
        assert!(!tags.iter().any(|r| r.label == "secret"));
    }

    #[test]
    fn journal_scope_restricts_to_that_journal() {
        let app = fixture_app();
        let scope = SearchScope::Journal("work".to_string());
        let people = app.filter_rows(&scope, FilterTab::People);
        // Only "work" entries count: Alice (from the first entry); Bob/alice live in
        // "trips" and are excluded.
        assert_eq!(people.len(), 1);
        assert_eq!(people[0].count, 1);
        assert!(people[0].label.eq_ignore_ascii_case("alice"));
    }

    #[test]
    fn location_rows_key_on_settlement_and_country() {
        let app = fixture_app();
        let rows = app.filter_rows(&SearchScope::AllJournals, FilterTab::Locations);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert!(labels.contains(&"Berlin - Germany"));
        assert!(labels.contains(&"Paris - France"));
        let berlin = rows
            .iter()
            .find(|r| r.label == "Berlin - Germany")
            .expect("berlin row");
        assert_eq!(berlin.count, 2);
    }

    #[test]
    fn location_rows_count_matches_free_typed_parts() {
        // A row groups on the coarse "Settlement - Country" bucket, but its count
        // is the launched `location:` search's result count.
        let mut app = crate::tui::test_support::app_with_journals(&[]);
        app.library.entries = vec![
            entry("work", |e| {
                let mut loc = place("Berlin", "Germany");
                loc.suburb = Some("Musterort".to_string());
                e.location = Some(loc);
            }),
            entry("work", |e| e.location = Some(place("Berlin", "Germany"))),
        ];
        let rows = app.filter_rows(&SearchScope::AllJournals, FilterTab::Locations);
        let berlin = rows
            .iter()
            .find(|r| r.label == "Berlin - Germany")
            .expect("berlin row");
        // Both entries share the bucket, but a launched `location:Musterort` finds
        // only the one with that suburb.
        assert_eq!(berlin.count, 2);
        assert_eq!(
            search_count(
                &mut app,
                &SearchScope::AllJournals,
                FilterTab::Locations,
                "Musterort"
            ),
            1
        );
    }

    #[test]
    fn opens_on_the_first_non_empty_tab() {
        // Only feelings populated → the dialog opens on Feelings, not empty Tags.
        let mut app = crate::tui::test_support::app_with_journals(&[]);
        app.library.entries = vec![entry("work", |e| e.feelings = strings(&["calm"]))];
        app.nav.focus = crate::tui::app::Focus::Journals;
        app.begin_filter();
        assert_eq!(app.filter_state().unwrap().tab, FilterTab::Feelings);
    }
}
