//! The vocabulary of a scope: every distinct tag, person, activity, feeling and
//! place in it, with the number of entries carrying each.
//!
//! The filter browser lists these to search by and the search box offers them to
//! complete with, so both read one set of rows ranked one way. A
//! tag/person/activity/feeling row launches an exact search for the value it
//! shows, so its count is a [`FacetTally`] taken while walking the entries once —
//! not a search per row. Place rows keep containment, because a place contains
//! the places around it, so [`PlaceCounter`] answers them by address instead.

use std::collections::{BTreeSet, HashMap};

use notema_domain::{Entry, Location, PlaceGroup};

use crate::tui::app::{AppModel, SearchScope};
use crate::tui::features::filter::FilterTab;
use crate::tui::features::metadata::metadata_values;
use crate::tui::state::MetadataKind;

/// One row of a facet: its display `label`, the search payload it launches
/// (`search_value`, placed after the tab's prefix), and its post count.
pub(crate) struct FilterRow {
    pub(crate) search_value: String,
    pub(crate) label: String,
    pub(crate) count: usize,
}

/// Every facet's rows, indexed by [`FilterTab::index`].
pub(crate) type FilterRows = [Vec<FilterRow>; FilterTab::COUNT];

/// One case-folded value: how many entries carry it, and the stamp that keeps an
/// entry from counting twice.
#[derive(Default)]
struct Bucket {
    entries: usize,
    /// The entry ordinal that last counted this bucket. Ordinals start at 1, so
    /// the default 0 reads as "not yet counted".
    stamp: u32,
}

/// Where one original casing lands, and how often it was written that way.
struct Form {
    bucket: u32,
    votes: usize,
}

/// A tally of one facet's distinct values across the entries in scope.
#[derive(Default)]
pub(crate) struct FacetTally {
    /// Keyed on the value as stored, so the hot path hashes a borrowed `&str` and
    /// the fold below runs once per distinct value rather than once per entry
    /// carrying it.
    forms: HashMap<String, Form>,
    /// Folded value to its bucket. `to_lowercase`, not `eq_ignore_ascii_case`: the
    /// search folds the same way, and an ASCII-only fold would show `Ärger` and
    /// `ärger` as one row whose own search returned both.
    keys: HashMap<String, u32>,
    buckets: Vec<Bucket>,
    ordinal: u32,
}

impl FacetTally {
    /// Add one entry's values for this facet. Call once per entry, including for
    /// entries carrying none, so the ordinals stay distinct.
    pub(crate) fn add_entry(&mut self, values: &[String]) {
        self.ordinal += 1;
        for value in values {
            self.add(value);
        }
    }

    fn add(&mut self, value: &str) {
        if let Some(form) = self.forms.get_mut(value) {
            form.votes += 1;
            let bucket = form.bucket;
            self.count(bucket);
            return;
        }
        let key = value.to_lowercase();
        // A value that folds to nothing gets no row: the parser drops an empty
        // needle, so its own search would match nothing.
        if key.is_empty() {
            return;
        }
        let bucket = match self.keys.get(key.as_str()) {
            Some(&bucket) => bucket,
            None => {
                let bucket = self.buckets.len() as u32;
                self.buckets.push(Bucket::default());
                self.keys.insert(key, bucket);
                bucket
            }
        };
        self.forms
            .insert(value.to_string(), Form { bucket, votes: 1 });
        self.count(bucket);
    }

    fn count(&mut self, bucket: u32) {
        let ordinal = self.ordinal;
        let bucket = &mut self.buckets[bucket as usize];
        // An entry carrying both `Work` and `work` is still one entry.
        if bucket.stamp != ordinal {
            bucket.stamp = ordinal;
            bucket.entries += 1;
        }
    }

    /// Each distinct value as `(display casing, entries)`, unordered.
    ///
    /// The casing is the most-written one, ties going to the lexicographically
    /// smallest form, so `Work` beats `work` at equal use. Votes count
    /// occurrences rather than entries: they only choose a label, and counting
    /// them per occurrence is what keeps every label where the per-tab counter
    /// had it. The number on the row is `entries`, which is entry-deduped.
    pub(crate) fn rows(self) -> Vec<(String, usize)> {
        let mut labels: Vec<Option<(String, usize)>> = vec![None; self.buckets.len()];
        for (form, Form { bucket, votes }) in self.forms {
            let slot = &mut labels[bucket as usize];
            let better = match slot {
                Some((best, best_votes)) => {
                    votes > *best_votes || (votes == *best_votes && form < *best)
                }
                None => true,
            };
            if better {
                *slot = Some((form, votes));
            }
        }
        labels
            .into_iter()
            .zip(self.buckets)
            .map(|(label, bucket)| {
                let (label, _) = label.expect("every bucket was created by a form");
                (label, bucket.entries)
            })
            .collect()
    }
}

/// Counts for the locations tab, whose rows keep containment: a row matches an
/// entry when every word of its query occurs in that entry's address haystack.
///
/// `location_predicate` reads nothing but that haystack, so entries sharing an
/// address are interchangeable to it. Interning them into one weighted address is
/// therefore exactly equivalent, and turns rows x entries substring tests into
/// distinct-words x distinct-addresses — 625 rows over 25k entries becomes ~630
/// words over ~5k addresses on the wide bench corpus.
#[derive(Default)]
pub(crate) struct PlaceCounter {
    groups: BTreeSet<PlaceGroup>,
    /// Distinct address haystack to the number of entries carrying it.
    addresses: HashMap<String, usize>,
}

impl PlaceCounter {
    /// Add one entry by its location. An entry without one contributes nothing
    /// and gets no row, as a predicate requiring a location does.
    pub(crate) fn add_entry(&mut self, location: Option<&Location>) {
        let Some(location) = location else {
            return;
        };
        if let Some(group) = location.place_group() {
            self.groups.insert(group);
        }
        // One `search_haystack` per entry, where a scan per row spent one per
        // entry per row. The string it builds becomes the key, so interning it
        // costs no second allocation.
        *self
            .addresses
            .entry(location.search_haystack())
            .or_default() += 1;
    }

    /// Each place group with the number of entries its `location:` search returns.
    pub(crate) fn counts(self) -> Vec<(PlaceGroup, usize)> {
        let groups: Vec<PlaceGroup> = self.groups.into_iter().collect();

        // Intern the words across every row's query: a country word is shared by
        // every settlement in it, so the distinct set is far smaller than the rows.
        let mut words: Vec<String> = Vec::new();
        let mut ids: HashMap<String, u32> = HashMap::new();
        let per_group: Vec<Vec<u32>> = groups
            .iter()
            .map(|group| {
                let mut group_ids: Vec<u32> = Location::search_tokens(&group.search_query())
                    .into_iter()
                    .map(|word| {
                        *ids.entry(word).or_insert_with_key(|word| {
                            words.push(word.clone());
                            (words.len() - 1) as u32
                        })
                    })
                    .collect();
                group_ids.sort_unstable();
                group_ids.dedup();
                group_ids
            })
            .collect();

        let mut counts = vec![0usize; groups.len()];
        let mut present = vec![false; words.len()];
        for (address, weight) in &self.addresses {
            for (id, word) in words.iter().enumerate() {
                present[id] = address.contains(word);
            }
            for (group, group_ids) in per_group.iter().enumerate() {
                // A query tokenizing to nothing matches nothing, never
                // everything: `all` over an empty set would invert the rule.
                if !group_ids.is_empty() && group_ids.iter().all(|&id| present[id as usize]) {
                    counts[group] += weight;
                }
            }
        }
        groups.into_iter().zip(counts).collect()
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

    /// The unlocked, in-scope entries a search under `scope` would see — the same
    /// set [`search_results_matching`](Self::search_results_matching) filters to.
    fn scoped_entries<'a>(
        &'a self,
        scope: &'a SearchScope,
    ) -> impl Iterator<Item = &'a Entry> + 'a {
        self.library
            .entries
            .iter()
            .filter(move |entry| scope.covers(entry))
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
}

#[cfg(test)]
mod row_tests {
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
