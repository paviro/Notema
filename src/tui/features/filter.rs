//! The filter browser: a tabbed overlay that lists every distinct
//! tag/person/activity/feeling/place value in scope with its post
//! count, and launches a filtered search on the chosen row.
//!
//! Scope is captured when the dialog opens — all journals from the journals
//! column, the selected journal from the entries column — and drives both the
//! listed counts and the launched search. The number shown on a row always
//! equals the number of results the search returns: a token row means one value,
//! so its count is a tally of that value and the search it launches asks for
//! exactly it, while a place row keeps containment and is counted by address,
//! which the `location:` predicate is a pure function of.

use std::rc::Rc;

use notema_domain::Entry;

use crate::tui::app::{AppModel, Focus, SearchScope};
use crate::tui::features::facets::{FacetTally, PlaceCounter};
use crate::tui::features::metadata::metadata_values;
use crate::tui::features::search::entry_in_search_scope;
use crate::tui::render::tab_strip::StripTab;
use crate::tui::state::{FilterTab, ListNav, MetadataKind, Overlay, SelectableList};

/// One row of a filter tab: its display `label`, the search payload it
/// launches (`search_value`, placed after the tab's prefix), and its post count.
pub(crate) struct FilterRow {
    pub(crate) search_value: String,
    pub(crate) label: String,
    pub(crate) count: usize,
}

/// Every tab's rows, indexed by [`FilterTab::index`].
pub(crate) type FilterRows = [Vec<FilterRow>; FilterTab::COUNT];

/// State for the filter overlay: the captured scope, the active tab, each
/// tab's rows (built once at open), and the selection/scroll of the active tab.
pub(crate) struct FilterState {
    /// Captured at open — drives the listed counts *and* the launched search.
    pub(crate) scope: SearchScope,
    pub(crate) tab: FilterTab,
    /// Shared with the memo the dialog opened from, so reopening it neither
    /// rebuilds nor copies the rows.
    pub(crate) rows: Rc<FilterRows>,
    pub(crate) list: SelectableList,
}

impl FilterState {
    fn new(scope: SearchScope, rows: Rc<FilterRows>) -> Self {
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

/// The values a token facet reads off an entry, or `None` for locations, which
/// are counted by address rather than by value.
fn facet_values(entry: &Entry, tab: FilterTab) -> Option<&[String]> {
    match tab {
        FilterTab::Tags => Some(metadata_values(entry, MetadataKind::Tags)),
        FilterTab::People => Some(metadata_values(entry, MetadataKind::People)),
        FilterTab::Activities => Some(metadata_values(entry, MetadataKind::Activities)),
        FilterTab::Feelings => Some(&entry.feelings),
        FilterTab::Locations => None,
    }
}

/// The locations tab's rows. A row shows the group's display label but launches
/// — and counts through — its search query, so the count equals the results.
fn place_rows(places: PlaceCounter) -> Vec<FilterRow> {
    let mut rows: Vec<FilterRow> = places
        .counts()
        .into_iter()
        .map(|(group, count)| FilterRow {
            label: group.display_label(),
            search_value: group.search_query(),
            count,
        })
        .collect();
    sort_by_count(&mut rows);
    rows
}

/// A token facet's rows. The row launches an exact search for the value it
/// shows, so the label and the search value are one string.
fn facet_rows(tally: FacetTally) -> Vec<FilterRow> {
    let mut rows: Vec<FilterRow> = tally
        .rows()
        .into_iter()
        .map(|(label, count)| FilterRow {
            search_value: label.clone(),
            label,
            count,
        })
        .collect();
    sort_by_count(&mut rows);
    rows
}

impl AppModel {
    pub(crate) fn begin_filter(&mut self) {
        let scope = if self.nav.focus == Focus::Journals {
            SearchScope::AllJournals
        } else {
            self.current_journal_scope()
        };
        let rows = self.cached_filter_rows(&scope);
        self.overlay = Overlay::Filter(Box::new(FilterState::new(scope, rows)));
    }

    /// Every tab's rows from one walk of the entries in scope.
    ///
    /// Reached through [`cached_filter_rows`](Self::cached_filter_rows), which is
    /// what keeps the walk to once per scope per change to the entries.
    pub(crate) fn all_filter_rows(&self, scope: &SearchScope) -> FilterRows {
        let mut tallies: [FacetTally; FilterTab::COUNT] = Default::default();
        let mut places = PlaceCounter::default();
        for entry in self.scoped_entries(scope) {
            for tab in FilterTab::ALL {
                if let Some(values) = facet_values(entry, tab) {
                    tallies[tab.index()].add_entry(values);
                }
            }
            places.add_entry(entry.location.as_ref());
        }
        let mut rows: FilterRows = Default::default();
        for (index, tally) in tallies.into_iter().enumerate() {
            rows[index] = facet_rows(tally);
        }
        rows[FilterTab::Locations.index()] = place_rows(places);
        rows
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

    /// One tab's rows. The dialog opens through
    /// [`all_filter_rows`](Self::all_filter_rows) instead, so this exists for the
    /// per-tab benchmark and the tests, which need one facet's cost and rows on
    /// their own.
    #[cfg(any(test, feature = "bench"))]
    pub(crate) fn filter_rows(&self, scope: &SearchScope, tab: FilterTab) -> Vec<FilterRow> {
        if tab == FilterTab::Locations {
            let mut places = PlaceCounter::default();
            for entry in self.scoped_entries(scope) {
                places.add_entry(entry.location.as_ref());
            }
            return place_rows(places);
        }
        let mut tally = FacetTally::default();
        for entry in self.scoped_entries(scope) {
            if let Some(values) = facet_values(entry, tab) {
                tally.add_entry(values);
            }
        }
        facet_rows(tally)
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
        let query = state.tab.launch_query(&row.search_value);
        self.close_overlay();
        self.enter_search(scope, query);
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
    ///
    /// The tags carry the shapes a row's value has to survive: one value holding
    /// another (`work`/`homework`), a casing pair both across entries and within
    /// one, a value with surrounding whitespace, and characters the query parser
    /// reads as structure — as does one place name. The feelings carry
    /// `happy`/`unhappy`, which are two canonical feelings, one holding the
    /// other's name.
    fn fixture_app() -> AppModel {
        let entries = vec![
            entry("work", |e| {
                e.tags = strings(&["berlin", "work", "Work"]);
                e.people = strings(&["Alice"]);
                e.activities = strings(&["coding"]);
                e.feelings = strings(&["happy"]);
                e.mood = Some(3);
                e.starred = true;
                e.location = Some(place("Berlin", "Germany"));
            }),
            entry("work", |e| {
                e.tags = strings(&["Berlin", "homework", "work"]);
                e.feelings = strings(&["happy", "calm"]);
                e.mood = Some(3);
                e.location = Some(place("Berlin", "Germany"));
            }),
            entry("trips", |e| {
                // Values holding characters the query parser reads as structure,
                // plus one whose whitespace is part of the value.
                e.tags = strings(&["berlin", "c++", "kreuzberg; mitte", "say \"hi\"", " work "]);
                e.people = strings(&["alice", "Bob"]);
                e.activities = strings(&["hiking", "r&d|ops", "a\";b"]);
                e.feelings = strings(&["calm", "unhappy"]);
                e.mood = Some(-2);
                e.starred = true;
                e.location = Some(place("Paris", "France"));
            }),
            entry("trips", |e| {
                // Place names holding a query separator. A location is not quoted
                // on the way out, so these are the rows whose counts only survive
                // if the launcher still keeps a separator literal — `;` between
                // filters, `|` between a location's alternatives.
                e.location = Some(place("Ville; Sud", "France"));
            }),
            entry("trips", |e| {
                e.location = Some(place("Rock | Roll", "USA"));
            }),
            // Shares a word with one half of `Rock | Roll`, so an unescaped `|`
            // in that row's launched query would reach this entry too and its
            // count would stop matching its results.
            entry("trips", |e| {
                e.location = Some(place("Roll", "USA"));
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

    /// Run the search a row would launch and return its hit count. Goes through
    /// `launch_query` rather than rebuilding it, so a change to how a tab quotes
    /// cannot pass here while breaking the browser.
    fn search_count(app: &mut AppModel, scope: &SearchScope, tab: FilterTab, value: &str) -> usize {
        app.search.scope = scope.clone();
        app.search.query.set_text(&tab.launch_query(value));
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

    /// The premise of the whole tab: a row selects the value it displays. `work`
    /// and `homework` are separate rows because a click on one must not return the
    /// other's entries.
    #[test]
    fn a_row_counts_its_value_and_not_the_ones_holding_it() {
        let mut app = fixture_app();
        let scope = SearchScope::AllJournals;
        let rows = app.filter_rows(&scope, FilterTab::Tags);
        let count_of = |label: &str| {
            rows.iter()
                .find(|r| r.label == label)
                .unwrap_or_else(|| panic!("{label:?} row"))
                .count
        };

        // Two entries are tagged `work`; a third is tagged `homework` and a fourth
        // `" work "`, and neither belongs to the `work` row.
        assert_eq!(count_of("work"), 2);
        assert_eq!(count_of("homework"), 1);
        assert_eq!(count_of(" work "), 1);

        // Typed by hand, the value is still a substring — which is what keeps the
        // results narrowing while it is being typed. It reaches all three unlocked
        // entries, where the `work` row reaches two.
        app.search.scope = scope.clone();
        app.search.query.set_text("tags:work");
        assert_eq!(app.search_results().len(), 3);
    }

    /// `happy` and `unhappy` are both canonical feelings, so before rows meant
    /// themselves the `happy` row counted the `unhappy` entry too.
    #[test]
    fn a_feeling_row_excludes_the_feelings_holding_its_name() {
        let mut app = fixture_app();
        let scope = SearchScope::AllJournals;
        let rows = app.filter_rows(&scope, FilterTab::Feelings);
        let count_of = |label: &str| {
            rows.iter()
                .find(|r| r.label == label)
                .unwrap_or_else(|| panic!("{label:?} row"))
                .count
        };

        assert_eq!(count_of("happy"), 2);
        assert_eq!(count_of("unhappy"), 1);

        // The other half of the guarantee: a hand-typed value still reaches both,
        // so exactness did not simply replace the substring rule.
        app.search.scope = scope.clone();
        app.search.query.set_text("feelings:happy");
        assert_eq!(app.search_results().len(), 3);
    }

    #[test]
    fn a_case_pair_is_one_row_and_an_entry_holding_both_counts_once() {
        let app = fixture_app();
        let rows = app.filter_rows(&SearchScope::AllJournals, FilterTab::Tags);
        // `Work` and `work` fold together, and the most-used casing labels the row.
        let work: Vec<&FilterRow> = rows
            .iter()
            .filter(|r| r.label.eq_ignore_ascii_case("work"))
            .collect();
        assert_eq!(work.len(), 1);
        assert_eq!(work[0].label, "work");
        // The first entry carries both casings and is still one entry.
        assert_eq!(work[0].count, 2);
    }

    /// The parser drops an empty needle, so `tags:""` matches nothing. A row for
    /// it would show a count its own search could never return.
    #[test]
    fn a_value_that_folds_to_nothing_gets_no_row() {
        let mut app = crate::tui::test_support::app_with_journals(&[]);
        app.library.entries = vec![entry("work", |e| e.tags = strings(&["", "work"]))];
        let rows = app.filter_rows(&SearchScope::AllJournals, FilterTab::Tags);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "work");
    }

    /// Folding with `to_lowercase` rather than `eq_ignore_ascii_case` is what keeps
    /// a non-ASCII casing pair one row whose own search returns both its entries.
    #[test]
    fn a_non_ascii_case_pair_is_one_row() {
        let mut app = crate::tui::test_support::app_with_journals(&[]);
        app.library.entries = vec![
            entry("work", |e| e.tags = strings(&["Ärger"])),
            entry("work", |e| e.tags = strings(&["ärger"])),
        ];
        let scope = SearchScope::AllJournals;
        let rows = app.filter_rows(&scope, FilterTab::Tags);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].count, 2);
        assert_eq!(
            search_count(&mut app, &scope, FilterTab::Tags, &rows[0].search_value),
            2
        );
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
