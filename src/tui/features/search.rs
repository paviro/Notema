use std::time::Instant;

use chrono::{Local, NaiveDate};
use notema_domain::{
    DateBound, DateFilter, DateSpec, Entry, EntryEncryptionState, MOOD_RANGE, SearchHit,
    entry_group_date, feeling_matches_search,
};

use crate::tui::{
    app::{AppModel, Focus, Mode, SearchScope},
    features::metadata::metadata_values,
    search::search_loaded_entries,
    state::MetadataKind,
};

impl AppModel {
    pub(crate) fn begin_search(&mut self) {
        let scope = if self.nav.focus == Focus::Journals {
            SearchScope::AllJournals
        } else {
            self.current_journal_scope()
        };
        self.enter_search(scope, String::new(), Vec::new());
    }

    /// Enter search mode with a prepared `query`/`hits`, focusing the entry list
    /// and selecting the first hit.
    pub(crate) fn enter_search(&mut self, scope: SearchScope, query: String, hits: Vec<SearchHit>) {
        self.search.scope = scope;
        self.nav.mode = Mode::Search;
        self.nav.focus = Focus::Entries;
        self.search.query.set_text(&query);
        self.search.hits = hits;
        self.commit_search_selection();
        // An all-journals search follows the global theme (see context_journal).
        self.apply_effective_theme();
    }

    pub(crate) fn exit_search(&mut self) {
        self.nav.mode = Mode::Browse;
        self.search.scope = SearchScope::AllJournals;
        self.search.query.clear();
        self.search.hits.clear();
        self.commit_search_selection();
        self.apply_effective_theme();
    }

    pub(crate) fn update_search_results(&mut self) {
        self.search.hits = self.search_results();
        self.commit_search_selection();
    }

    /// The search scope for a metadata/feeling drill-down: the selected journal,
    /// or all journals when none is selected.
    pub(crate) fn current_journal_scope(&self) -> SearchScope {
        self.selected_journal()
            .map(|journal| SearchScope::Journal(journal.name.clone()))
            .unwrap_or(SearchScope::AllJournals)
    }

    /// Shared tail of every search entry/exit: clear the debounce state,
    /// invalidate the row cache, and reset the selection to the first hit.
    fn commit_search_selection(&mut self) {
        self.search.dirty = false;
        self.search.last_edit = None;
        self.caches.bump_rows();
        self.nav.selected_entry_index = (!self.search.hits.is_empty()).then_some(0);
        self.reset_entry_scroll();
    }

    /// Mark the search query as changed without running the (expensive) hit
    /// recompute yet. The event loop calls [`Self::update_search_results`] once
    /// typing pauses, so a fast typist doesn't re-scan the whole corpus per key.
    fn mark_search_dirty(&mut self) {
        self.search.dirty = true;
        self.search.last_edit = Some(Instant::now());
    }

    /// The search field owns the caret only while typing in it.
    pub(crate) fn is_search_input_active(&self) -> bool {
        self.nav.mode == Mode::Search && self.nav.focus == Focus::Entries
    }

    /// Feed a key press to the search field, deferring the hit recompute when
    /// it changed the query (debounce).
    pub(crate) fn search_input_key(&mut self, key: crossterm::event::KeyEvent) {
        if self.search.query.input(key) {
            self.mark_search_dirty();
        }
    }

    /// Insert a pasted block into the search field, deferring the hit recompute
    /// like [`Self::search_input_key`].
    pub(crate) fn search_input_paste(&mut self, text: &str) {
        if self.search.query.paste_str(text) {
            self.mark_search_dirty();
        }
    }

    pub(crate) fn search_results(&self) -> Vec<SearchHit> {
        let query = self.search.query.as_str();
        if let Some(tag) = query.strip_prefix("tags:") {
            self.search_results_by_metadata(MetadataKind::Tags, tag.trim())
        } else if let Some(person) = query.strip_prefix("people:") {
            self.search_results_by_metadata(MetadataKind::People, person.trim())
        } else if let Some(activity) = query.strip_prefix("activities:") {
            self.search_results_by_metadata(MetadataKind::Activities, activity.trim())
        } else if let Some(feeling) = query.strip_prefix("feelings:") {
            self.search_results_by_feeling(feeling.trim())
        } else if let Some(value) = query.strip_prefix("star:") {
            match parse_starred_value(value) {
                Some(want) => self.search_results_by_starred(want),
                // An unparseable flag (e.g. `star:maybe`) matches nothing,
                // mirroring how an unknown `feelings:` value yields no hits.
                None => Vec::new(),
            }
        } else if let Some(place) = query.strip_prefix("location:") {
            self.search_results_by_location(place.trim())
        } else if let Some(mood) = query.strip_prefix("mood:") {
            match mood.trim().parse::<i8>() {
                Ok(score) if MOOD_RANGE.contains(&score) => self.search_results_by_mood(score),
                // Unparseable or out-of-range matches nothing, like an unknown `star:`.
                _ => Vec::new(),
            }
        } else if let Some((bound, value)) = strip_date_prefix(query) {
            match DateSpec::parse(value) {
                Some(spec) => self.search_results_by_date(DateFilter { bound, spec }),
                // An unreadable date matches nothing rather than falling through
                // to a fuzzy search for the literal `date:…` text.
                None => Vec::new(),
            }
        } else {
            search_loaded_entries(&self.library.entries, query, &self.search.scope)
        }
    }

    /// Build hits from the in-scope, unlocked entries matching `predicate`.
    fn search_results_matching(&self, predicate: impl Fn(&Entry) -> bool) -> Vec<SearchHit> {
        self.library
            .entries
            .iter()
            .filter(|entry| entry_in_search_scope(entry, &self.search.scope) && predicate(entry))
            .map(SearchHit::from_entry)
            .collect()
    }

    pub(crate) fn search_results_by_metadata(
        &self,
        kind: MetadataKind,
        query: &str,
    ) -> Vec<SearchHit> {
        self.search_results_matching(metadata_predicate(kind, query))
    }

    pub(crate) fn search_results_by_feeling(&self, feeling: &str) -> Vec<SearchHit> {
        self.search_results_matching(feeling_predicate(feeling))
    }

    pub(crate) fn search_results_by_starred(&self, want: bool) -> Vec<SearchHit> {
        self.search_results_matching(|entry| entry.starred == want)
    }

    /// Entries whose location matches `query` (see [`location_predicate`]).
    pub(crate) fn search_results_by_location(&self, query: &str) -> Vec<SearchHit> {
        self.search_results_matching(location_predicate(query))
    }

    pub(crate) fn search_results_by_mood(&self, score: i8) -> Vec<SearchHit> {
        self.search_results_matching(|entry| entry.mood == Some(score))
    }

    /// Entries whose date satisfies `filter` (see [`date_predicate`]).
    pub(crate) fn search_results_by_date(&self, filter: DateFilter) -> Vec<SearchHit> {
        self.search_results_matching(date_predicate(filter, Local::now().date_naive()))
    }
}

/// Split a `date:`/`before:`/`after:` query into its bound and value.
fn strip_date_prefix(query: &str) -> Option<(DateBound, &str)> {
    [
        ("date:", DateBound::On),
        ("before:", DateBound::Before),
        ("after:", DateBound::After),
    ]
    .into_iter()
    .find_map(|(prefix, bound)| {
        query
            .strip_prefix(prefix)
            .map(|value| (bound, value.trim()))
    })
}

/// Whether an entry's date satisfies `filter`. Entries with neither a creation
/// timestamp nor a dated filename have no date to compare, so they never match.
pub(crate) fn date_predicate(
    filter: DateFilter,
    today: NaiveDate,
) -> impl Fn(&Entry) -> bool + use<> {
    move |entry| entry_group_date(entry).is_some_and(|date| filter.matches(date, today))
}

/// Whether an entry matches a `tags:`/`people:`/`activities:` search: any of its
/// `kind` values contains `query`, case-insensitively. The filter browser's row
/// counter and the launched search share this predicate (and the location/feeling
/// ones below), so a row's count always equals the hits its search returns.
pub(crate) fn metadata_predicate(
    kind: MetadataKind,
    query: &str,
) -> impl Fn(&Entry) -> bool + use<> {
    let needle = query.to_lowercase();
    move |entry| {
        metadata_values(entry, kind)
            .iter()
            .any(|value| value.to_lowercase().contains(&needle))
    }
}

/// Whether an entry matches a `location:` search: every word in `query` appears
/// (case-insensitively, any order) somewhere in the location's
/// [`search_haystack`](notema_domain::Location::search_haystack). Words split on any
/// non-alphanumeric character, so `"Berlin, Germany"` and `"Berlin - Germany"`
/// tokenize alike.
pub(crate) fn location_predicate(query: &str) -> impl Fn(&Entry) -> bool + use<> {
    let needles: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    move |entry| {
        // An empty or punctuation-only query matches nothing.
        !needles.is_empty()
            && entry.location.as_ref().is_some_and(|location| {
                let haystack = location.search_haystack();
                needles.iter().all(|needle| haystack.contains(needle))
            })
    }
}

/// Whether an entry matches a `feelings:` search for `feeling`.
pub(crate) fn feeling_predicate(feeling: &str) -> impl Fn(&Entry) -> bool + use<> {
    let feeling = feeling.to_string();
    move |entry| {
        entry
            .feelings
            .iter()
            .any(|entry_feeling| feeling_matches_search(entry_feeling, &feeling))
    }
}

/// Whether `entry` is visible to a search under `scope`: unlocked (locked and
/// unreadable encrypted entries never match) and inside the scope.
pub(crate) fn entry_in_search_scope(entry: &Entry, scope: &SearchScope) -> bool {
    !matches!(
        entry.encryption_state,
        EntryEncryptionState::EncryptedLocked | EntryEncryptionState::EncryptedUnreadable
    ) && match scope {
        SearchScope::AllJournals => true,
        SearchScope::Journal(journal) => entry.journal == *journal,
    }
}

/// Parse the value of a `star:` query. An empty value is the friendlier
/// `true` (the common intent when filtering for favorites); `true`/`1` and
/// `false`/`0` are accepted case-insensitively; anything else is `None` (no
/// match).
fn parse_starred_value(value: &str) -> Option<bool> {
    match value.trim().to_lowercase().as_str() {
        "" | "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notema_domain::Location;

    fn located_entry(city: &str, country: &str, mood: Option<i8>) -> Entry {
        let mut entry = notema_domain::Entry {
            id: "id".to_string(),
            journal: "work".to_string(),
            path: std::path::PathBuf::from("work/e.md"),
            encryption_state: EntryEncryptionState::Plain,
            created_at: None,
            edited_at: None,
            preview: String::new(),
            activities: Vec::new(),
            feelings: Vec::new(),
            people: Vec::new(),
            tags: Vec::new(),
            mood,
            starred: false,
            location: Some(Location {
                city: Some(city.to_string()),
                country: Some(country.to_string()),
                ..Location::default()
            }),
            weather: None,
            celestial: None,
            air_quality: None,
            import: None,
            body: String::new(),
            word_count: 0,
            search_haystack: String::new(),
            warning: None,
        };
        entry.location.as_mut().unwrap().name = None;
        entry
    }

    fn app_with(entries: Vec<Entry>) -> AppModel {
        let mut app = crate::tui::test_support::app_with_journals(&[]);
        app.library.entries = entries;
        app
    }

    fn run(app: &mut AppModel, query: &str) -> usize {
        app.search.scope = SearchScope::AllJournals;
        app.search.query.set_text(query);
        app.search_results().len()
    }

    #[test]
    fn location_filter_matches_place_group_and_any_named_part() {
        let mut detailed = located_entry("Berlin", "Germany", None);
        {
            let loc = detailed.location.as_mut().unwrap();
            loc.road = Some("Musterstraße".to_string());
            loc.house_number = Some("12".to_string());
            loc.suburb = Some("Musterort".to_string());
            loc.postcode = Some("12345".to_string());
        }
        let mut app = app_with(vec![
            detailed,
            located_entry("Berlin", "Germany", None),
            located_entry("Paris", "France", None),
        ]);

        // The index's "City, Country" query and a typed "City - Country" tokenize
        // the same (the separator is dropped), so both words must appear.
        assert_eq!(run(&mut app, "location:Berlin, Germany"), 2);
        assert_eq!(run(&mut app, "location:Berlin - Germany"), 2);
        assert_eq!(run(&mut app, "location:Paris, France"), 1);

        // Any named part matches, case-insensitively.
        assert_eq!(run(&mut app, "location:berlin"), 2);
        assert_eq!(run(&mut app, "location:Musterstraße"), 1);
        assert_eq!(run(&mut app, "location:Musterstraße 12"), 1);
        assert_eq!(run(&mut app, "location:12345"), 1);
        assert_eq!(run(&mut app, "location:Musterort"), 1);

        // Order-independent, and words may come from different address parts.
        assert_eq!(run(&mut app, "location:Germany Berlin"), 2);
        assert_eq!(run(&mut app, "location:12 Musterstraße"), 1);
        assert_eq!(run(&mut app, "location:Musterort Berlin"), 1);

        // Every word must match, so a bogus or absent part finds nothing.
        assert_eq!(run(&mut app, "location:Berlin Tokyo"), 0);
        assert_eq!(run(&mut app, "location:Tokyo"), 0);
    }

    fn dated_entry(created_at: Option<&str>, path: &str) -> Entry {
        let mut entry = located_entry("Berlin", "Germany", None);
        entry.created_at = created_at.map(notema_domain::Timestamp::parse);
        entry.path = std::path::PathBuf::from(path);
        entry
    }

    fn app_with_dates(dates: &[&str]) -> AppModel {
        app_with(
            dates
                .iter()
                .map(|date| dated_entry(Some(&format!("{date}T10:00:00+02:00")), "work/e.md"))
                .collect(),
        )
    }

    #[test]
    fn date_filter_narrows_as_components_are_added() {
        let mut app = app_with_dates(&["2025-07-25", "2026-07-25", "2026-07-04", "2026-09-01"]);

        assert_eq!(run(&mut app, "date:2026"), 3);
        assert_eq!(run(&mut app, "date:2026-07"), 2);
        assert_eq!(run(&mut app, "date:2026-07-25"), 1);
    }

    /// Results must not blank out between keystrokes while a date is half-typed.
    #[test]
    fn half_typed_date_keeps_the_wider_result_set() {
        let mut app = app_with_dates(&["2026-07-25", "2026-09-01", "2025-07-25"]);

        assert_eq!(run(&mut app, "date:2026"), 2);
        assert_eq!(run(&mut app, "date:2026-"), 2);
        assert_eq!(run(&mut app, "date:2026-0"), 2);
        assert_eq!(run(&mut app, "date:2026-07"), 1);
        assert_eq!(run(&mut app, "date:2026-07-"), 1);
    }

    #[test]
    fn wildcard_opens_a_component() {
        let mut app = app_with_dates(&["2024-07-25", "2025-07-25", "2026-07-25", "2026-08-25"]);

        assert_eq!(run(&mut app, "date:*-07-25"), 3);
        assert_eq!(run(&mut app, "date:2026-*-25"), 2);
    }

    #[test]
    fn before_and_after_bracket_the_named_span() {
        let mut app = app_with_dates(&["2025-12-31", "2026-07-25", "2027-01-01"]);

        assert_eq!(run(&mut app, "before:2026"), 1);
        assert_eq!(run(&mut app, "after:2026"), 1);
        assert_eq!(run(&mut app, "date:2026"), 1);
        // An open component recurs, so there is no side to be on.
        assert_eq!(run(&mut app, "before:*-07-25"), 0);
    }

    /// An entry with neither a creation timestamp nor a dated filename has no
    /// date to compare against.
    #[test]
    fn undated_entries_never_match() {
        let mut app = app_with(vec![dated_entry(None, "work/no-date.md")]);

        assert_eq!(run(&mut app, "date:2026"), 0);
        assert_eq!(run(&mut app, "before:2026"), 0);
    }

    /// Falls back to the filename date, the same rule the entry list groups by.
    #[test]
    fn date_filter_falls_back_to_the_filename_date() {
        let mut app = app_with(vec![dated_entry(
            None,
            "work/2026/07/25/2026-07-25T10-00-00-id.md",
        )]);

        assert_eq!(run(&mut app, "date:2026-07-25"), 1);
    }

    #[test]
    fn unreadable_date_matches_nothing_rather_than_searching_the_text() {
        let mut app = app_with_dates(&["2026-07-25"]);

        // Without the guard these would fall through to a fuzzy search for the
        // literal `date:…` text.
        assert_eq!(run(&mut app, "date:garbage"), 0);
        assert_eq!(run(&mut app, "date:"), 0);
    }

    #[test]
    fn mood_filter_parses_clamps_and_rejects_junk() {
        let mut app = app_with(vec![
            located_entry("Berlin", "Germany", Some(3)),
            located_entry("Berlin", "Germany", Some(-2)),
        ]);
        assert_eq!(run(&mut app, "mood:3"), 1);
        assert_eq!(run(&mut app, "mood:-2"), 1);
        // Unparseable or out-of-range values match nothing.
        assert_eq!(run(&mut app, "mood:x"), 0);
        assert_eq!(run(&mut app, "mood:9"), 0);
    }
}
