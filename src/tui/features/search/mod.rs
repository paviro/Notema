//! Search mode: entering and leaving it, and running the query box against the
//! library. The query grammar lives in [`parse`], the entry matchers in
//! [`predicate`]; this module binds a segment's prefix to its predicate and
//! drives the results.

pub(crate) mod offsets;
mod parse;
mod predicate;
mod suggest;

use std::time::Instant;

use chrono::{Local, NaiveDate};
use notema_domain::{DateFilter, DateSpec, Entry, MOOD_RANGE, SearchHit};

use crate::tui::{
    app::{AppModel, Focus, Mode, SearchScope},
    search::search_loaded_entries_where,
    state::MetadataKind,
};

use parse::{parse_starred_value, split_unquoted, unquote};
use predicate::{date_predicate, feeling_predicate, location_predicate, metadata_predicate};

pub(crate) use parse::{Prefix, escape_filter_value, quote_filter_value, split_prefix};

/// Boxed so segments with different predicate types share one `Vec`.
type EntryPredicate = Box<dyn Fn(&Entry) -> bool>;

impl AppModel {
    pub(crate) fn begin_search(&mut self) {
        let scope = if self.nav.focus == Focus::Journals {
            SearchScope::AllJournals
        } else {
            self.current_journal_scope()
        };
        self.enter_search(scope, String::new());
    }

    /// Enter search mode running `query` under `scope`, focusing the entry list
    /// and selecting the first hit.
    ///
    /// The hits are computed here rather than passed in: `search_results` reads
    /// the scope off `self`, so a caller preparing them beforehand runs its query
    /// under whatever scope the last search left behind.
    pub(crate) fn enter_search(&mut self, scope: SearchScope, query: String) {
        self.search.scope = scope;
        self.nav.mode = Mode::Search;
        self.nav.focus = Focus::Entries;
        self.search.query.set_text(&query);
        self.search.hits = self.search_results();
        self.search.suggestions.clear();
        self.commit_search_selection();
        // An all-journals search follows the global theme (see context_journal).
        self.apply_effective_theme();
    }

    pub(crate) fn exit_search(&mut self) {
        self.nav.mode = Mode::Browse;
        self.search.scope = SearchScope::AllJournals;
        self.search.query.clear();
        self.search.hits.clear();
        self.search.suggestions.clear();
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
    ///
    /// The suggestions refresh either way, and are not on the debounce: a caret
    /// move changes which value is being completed without changing the text, and
    /// the candidates come from a memo rather than a walk.
    pub(crate) fn search_input_key(&mut self, key: crossterm::event::KeyEvent) {
        if self.search.query.input(key) {
            self.mark_search_dirty();
        }
        self.refresh_search_suggestions();
    }

    /// Insert a pasted block into the search field, deferring the hit recompute
    /// like [`Self::search_input_key`].
    pub(crate) fn search_input_paste(&mut self, text: &str) {
        if self.search.query.paste_str(text) {
            self.mark_search_dirty();
        }
        self.refresh_search_suggestions();
    }

    /// Run the search query. Filters chain on `;` and all must match; a segment
    /// with no known prefix is full-text, AND-ed with the filters and supplying
    /// the ranking.
    pub(crate) fn search_results(&self) -> Vec<SearchHit> {
        let today = Local::now().date_naive();

        let mut predicates: Vec<EntryPredicate> = Vec::new();
        let mut text_parts: Vec<&str> = Vec::new();
        for segment in split_unquoted(self.search.query.as_str(), ';') {
            let segment = segment.trim();
            if segment.is_empty() {
                continue;
            }
            match classify_segment(segment, today) {
                SegmentKind::Filter(predicate) => predicates.push(predicate),
                // A known filter with an unreadable value zeroes the whole query,
                // as a lone unreadable filter always has.
                SegmentKind::NoMatch => return Vec::new(),
                SegmentKind::Text(text) => text_parts.push(text),
            }
        }

        let has_filters = !predicates.is_empty();
        let matches_all = move |entry: &Entry| predicates.iter().all(|predicate| predicate(entry));
        if text_parts.is_empty() {
            // A wholly empty query (no filters, no text) matches nothing, rather
            // than every entry (which an all-of-nothing predicate would return).
            if !has_filters {
                return Vec::new();
            }
            self.search_results_matching(matches_all)
        } else {
            search_loaded_entries_where(
                &self.library.entries,
                &text_parts.join(" "),
                &self.search.scope,
                matches_all,
            )
        }
    }

    /// Build hits from the in-scope, unlocked entries matching `predicate`.
    fn search_results_matching(&self, predicate: impl Fn(&Entry) -> bool) -> Vec<SearchHit> {
        self.library
            .entries
            .iter()
            .filter(|entry| self.search.scope.covers(entry) && predicate(entry))
            .map(SearchHit::from_entry)
            .collect()
    }
}

/// One `;`-separated piece of a search query.
enum SegmentKind<'a> {
    /// A recognized `prefix:` filter, as a predicate to AND with the others.
    Filter(EntryPredicate),
    /// A recognized filter whose value couldn't be read (e.g. `date:garbage`);
    /// it zeroes the whole query rather than matching everything.
    NoMatch,
    /// No known prefix: full-text, to be AND-ed with the filter predicates.
    Text(&'a str),
}

/// Classify one query segment. `today` is threaded in so date matching stays pure.
fn classify_segment(segment: &str, today: NaiveDate) -> SegmentKind<'_> {
    let Some((prefix, value)) = split_prefix(segment) else {
        return SegmentKind::Text(segment);
    };
    let value = value.trim();
    match prefix {
        Prefix::Tags => {
            SegmentKind::Filter(Box::new(metadata_predicate(MetadataKind::Tags, value)))
        }
        Prefix::People => {
            SegmentKind::Filter(Box::new(metadata_predicate(MetadataKind::People, value)))
        }
        Prefix::Activities => SegmentKind::Filter(Box::new(metadata_predicate(
            MetadataKind::Activities,
            value,
        ))),
        Prefix::Feelings => SegmentKind::Filter(Box::new(feeling_predicate(value))),
        Prefix::Star => match parse_starred_value(&unquote(value)) {
            Some(want) => SegmentKind::Filter(Box::new(move |entry: &Entry| entry.starred == want)),
            None => SegmentKind::NoMatch,
        },
        // Raw: the `|` split runs before unquoting, so `"a|b"|c` is the one place
        // `a|b` and the place `c`, exactly as it reads.
        Prefix::Location => SegmentKind::Filter(Box::new(location_predicate(value))),
        Prefix::Mood => match unquote(value).parse::<i8>() {
            Ok(score) if MOOD_RANGE.contains(&score) => {
                SegmentKind::Filter(Box::new(move |entry: &Entry| entry.mood == Some(score)))
            }
            _ => SegmentKind::NoMatch,
        },
        Prefix::Date(bound) => match DateSpec::parse(&unquote(value)) {
            Some(spec) => {
                SegmentKind::Filter(Box::new(date_predicate(DateFilter { bound, spec }, today)))
            }
            None => SegmentKind::NoMatch,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notema_domain::{EntryEncryptionState, Location};

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

    /// `|` is the one operator a place takes: an entry has a single location, so
    /// alternatives are the only combination that can match anything.
    #[test]
    fn a_location_takes_alternatives_but_reads_a_plus_literally() {
        let mut app = app_with(vec![
            located_entry("Berlin", "Germany", None),
            located_entry("Paris", "France", None),
            located_entry("Rock + Roll", "USA", None),
        ]);

        assert_eq!(run(&mut app, "location:Berlin|Paris"), 2);
        assert_eq!(run(&mut app, "location:Berlin | Paris"), 2);
        assert_eq!(run(&mut app, "location:Berlin|Tokyo"), 1);
        assert_eq!(run(&mut app, "location:Tokyo|Kyoto"), 0);
        // Each alternative still requires all of its own words.
        assert_eq!(run(&mut app, "location:Berlin France|Paris France"), 1);
        // An empty alternative narrows nothing rather than matching everything.
        assert_eq!(run(&mut app, "location:Berlin|"), 1);

        // `+` is a character in a place name, not an AND.
        assert_eq!(run(&mut app, "location:Rock + Roll"), 1);
        assert_eq!(run(&mut app, "location:Berlin+Paris"), 0);

        // Quoting keeps a `|` inside one alternative, and is still not exactness:
        // the country alone reaches the entry either way.
        let mut app = app_with(vec![located_entry("A|B", "Germany", None)]);
        assert_eq!(run(&mut app, "location:\"A|B\""), 1);
        assert_eq!(run(&mut app, "location:\"A|B\"|Tokyo"), 1);
        assert_eq!(run(&mut app, "location:Germany"), 1);
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

    // --- Chained filters ---------------------------------------------------

    /// An entry carrying `body`, `tags`, `people`, and a date, with the search
    /// haystack built from body + metadata so full-text mixing works.
    fn rich_entry(body: &str, tags: &[&str], people: &[&str], date: Option<&str>) -> Entry {
        let mut entry = located_entry("Berlin", "Germany", None);
        entry.tags = tags.iter().map(|value| value.to_string()).collect();
        entry.people = people.iter().map(|value| value.to_string()).collect();
        entry.created_at =
            date.map(|day| notema_domain::Timestamp::parse(format!("{day}T10:00:00+02:00")));
        entry.body = body.to_string();
        let metadata = notema_domain::Metadata {
            tags: entry.tags.clone(),
            people: entry.people.clone(),
            ..Default::default()
        };
        entry.search_haystack = notema_domain::build_search_haystack(body, &metadata);
        entry
    }

    #[test]
    fn chained_filters_all_must_match() {
        let mut app = app_with(vec![
            rich_entry("", &["work"], &["alice"], None),
            rich_entry("", &["work"], &["bob"], None),
            rich_entry("", &["home"], &["alice"], None),
        ]);

        // Each filter alone is looser than the two chained together.
        assert_eq!(run(&mut app, "tags:work"), 2);
        assert_eq!(run(&mut app, "people:alice"), 2);
        assert_eq!(run(&mut app, "tags:work; people:alice"), 1);
        // Whitespace around the separator is ignored.
        assert_eq!(run(&mut app, "tags:work ; people:alice"), 1);
    }

    #[test]
    fn plus_requires_every_value() {
        let mut app = app_with(vec![
            rich_entry("", &[], &["alice", "bob"], None),
            rich_entry("", &[], &["alice"], None),
        ]);

        // Both must be present, each matching partially.
        assert_eq!(run(&mut app, "people:alice+bob"), 1);
        assert_eq!(run(&mut app, "people:ali+bo"), 1);
        assert_eq!(run(&mut app, "people:alice"), 2);
        assert_eq!(run(&mut app, "people:alice+carol"), 0);
    }

    #[test]
    fn pipe_accepts_any_alternative() {
        let mut app = app_with(vec![
            rich_entry("", &["work"], &[], None),
            rich_entry("", &["home"], &[], None),
            rich_entry("", &["travel"], &[], None),
        ]);

        assert_eq!(run(&mut app, "tags:work|home"), 2);
        assert_eq!(run(&mut app, "tags:work|home|travel"), 3);
        assert_eq!(run(&mut app, "tags:work|missing"), 1);
    }

    #[test]
    fn plus_binds_looser_than_pipe() {
        let mut app = app_with(vec![
            rich_entry("", &["work", "berlin"], &[], None),
            rich_entry("", &["work", "paris"], &[], None),
            rich_entry("", &["home", "berlin"], &[], None),
        ]);

        // `work+berlin|paris` is work AND (berlin OR paris): the two work entries.
        assert_eq!(run(&mut app, "tags:work+berlin|paris"), 2);
        assert_eq!(run(&mut app, "tags:home+berlin|paris"), 1);
    }

    #[test]
    fn before_and_after_bracket_a_range() {
        let mut app = app_with(vec![
            rich_entry("", &[], &[], Some("2025-01-15")),
            rich_entry("", &[], &[], Some("2025-03-10")),
            rich_entry("", &[], &[], Some("2025-05-20")),
            rich_entry("", &[], &[], Some("2025-06-01")),
            rich_entry("", &[], &[], Some("2026-03-01")),
        ]);

        // Strictly after January and strictly before June leaves Feb–May 2025.
        assert_eq!(run(&mut app, "after:2025-01; before:2025-06"), 2);
    }

    #[test]
    fn free_text_and_filter_intersect() {
        let mut app = app_with(vec![
            rich_entry("beach trip", &["travel"], &[], None),
            rich_entry("beach cleanup", &["work"], &[], None),
            rich_entry("mountain trip", &["travel"], &[], None),
        ]);

        // The bare word is full-text; the tag narrows it further.
        assert_eq!(run(&mut app, "beach"), 2);
        assert_eq!(run(&mut app, "beach; tags:travel"), 1);
    }

    #[test]
    fn an_unreadable_segment_zeroes_the_whole_query() {
        let mut app = app_with(vec![rich_entry("", &["work"], &[], None)]);

        // `tags:work` alone would match; the bad date segment kills the query.
        assert_eq!(run(&mut app, "tags:work"), 1);
        assert_eq!(run(&mut app, "tags:work; date:garbage"), 0);
    }

    /// A scalar has no exact mode, so quoting one is only ever structural — and
    /// an unreadable value zeroes the whole query, so an unclosed quote used to
    /// take the filters beside it down as it was typed.
    #[test]
    fn an_unclosed_quote_leaves_a_scalar_filter_readable() {
        let mut app = app_with(vec![
            rich_entry("", &["work"], &[], Some("2026-07-25")),
            rich_entry("", &["home"], &[], Some("2026-07-25")),
        ]);

        assert_eq!(run(&mut app, "date:\"2026-07-25"), 2);
        assert_eq!(run(&mut app, "tags:work; date:\"2026-07-25"), 1);
    }

    #[test]
    fn a_bare_prefix_matches_nothing() {
        let mut app = app_with(vec![rich_entry("", &["work"], &[], None)]);

        // No value to match on, so the filter is empty rather than universal.
        assert_eq!(run(&mut app, "tags:"), 0);
        assert_eq!(run(&mut app, "feelings:"), 0);
        // An empty quoted value is empty too, not "match everything exactly".
        assert_eq!(run(&mut app, "tags:\"\""), 0);
        assert_eq!(run(&mut app, "feelings:\"\""), 0);
    }

    // --- Exactness ---------------------------------------------------------

    #[test]
    fn quoting_matches_exactly_while_typing_still_narrows() {
        let mut app = app_with(vec![
            rich_entry("", &["app"], &[], None),
            rich_entry("", &["apple"], &[], None),
            rich_entry("", &["pineapple"], &[], None),
        ]);

        // Unquoted stays a substring, so the list narrows as the value is typed.
        assert_eq!(run(&mut app, "tags:app"), 3);
        assert_eq!(run(&mut app, "tags:appl"), 2);
        // Quoted is the value itself — what a chip or a filter row commits.
        assert_eq!(run(&mut app, "tags:\"app\""), 1);
        assert_eq!(run(&mut app, "tags:\"apple\""), 1);
        assert_eq!(run(&mut app, "tags:\"pineapple\""), 1);
        // The opening quote is typed before the value it will wrap, so until it
        // has a partner the value narrows exactly as the bare one does — the
        // closing quote is what makes it exact.
        assert_eq!(run(&mut app, "tags:\"app"), 3);
        assert_eq!(run(&mut app, "tags:\"appl"), 2);
    }

    /// A quoted value means the bytes between the quotes, so a tag stored with
    /// surrounding whitespace is reachable and distinct from the trimmed one.
    #[test]
    fn a_quoted_value_keeps_its_surrounding_whitespace() {
        let mut app = app_with(vec![
            rich_entry("", &[" work "], &[], None),
            rich_entry("", &["work"], &[], None),
        ]);

        assert_eq!(run(&mut app, "tags:\" work \""), 1);
        assert_eq!(run(&mut app, "tags:\"work\""), 1);
        assert_eq!(run(&mut app, "tags:work"), 2);
    }

    #[test]
    fn mixed_quoting_within_one_group() {
        let mut app = app_with(vec![
            rich_entry("", &["a"], &[], None),
            rich_entry("", &["ab"], &[], None),
            rich_entry("", &["bc"], &[], None),
            rich_entry("", &["z"], &[], None),
        ]);

        // Exactly-a, or anything containing b.
        assert_eq!(run(&mut app, "tags:\"a\"|b"), 3);
        assert_eq!(run(&mut app, "tags:\"a\""), 1);
        assert_eq!(run(&mut app, "tags:a"), 2);
        // Unbalanced quotes are not a pair, so the `"` stays literal.
        assert_eq!(run(&mut app, "tags:\"a\"b"), 0);
    }

    /// `happy` and `unhappy` are both canonical feelings, one holding the other's
    /// name. A committed value must separate them; a typed one must not.
    #[test]
    fn feelings_match_exactly_or_through_an_alias() {
        let mut happy = rich_entry("", &[], &[], None);
        happy.feelings = vec!["happy".to_string()];
        let mut unhappy = rich_entry("", &[], &[], None);
        unhappy.feelings = vec!["unhappy".to_string()];
        let mut joyful = rich_entry("", &[], &[], None);
        joyful.feelings = vec!["joyful".to_string()];
        let mut app = app_with(vec![happy, unhappy, joyful]);

        assert_eq!(run(&mut app, "feelings:happy"), 2);
        assert_eq!(run(&mut app, "feelings:\"happy\""), 1);
        assert_eq!(run(&mut app, "feelings:\"unhappy\""), 1);
        // An alias is a synonym for a whole value, so it survives exactness.
        assert_eq!(run(&mut app, "feelings:\"joyous\""), 1);
        // `joyful` is a canonical feeling in its own right, never an alias of
        // `happy`, so it reaches only itself.
        assert_eq!(run(&mut app, "feelings:\"joyful\""), 1);
    }

    // --- Quoted values -----------------------------------------------------

    #[test]
    fn quoting_matches_a_value_holding_an_operator() {
        let mut app = app_with(vec![
            rich_entry("", &["c++"], &[], None),
            rich_entry("", &["cooking"], &[], None),
        ]);

        // Unquoted, `+` splits and the leftover `c` matches both tags.
        assert_eq!(run(&mut app, "tags:c++"), 2);
        assert_eq!(run(&mut app, "tags:\"c++\""), 1);
    }

    #[test]
    fn a_quoted_value_may_hold_the_chain_separator() {
        let mut app = app_with(vec![
            rich_entry("", &["berlin; mitte"], &[], None),
            rich_entry("", &["berlin"], &[], None),
        ]);

        assert_eq!(run(&mut app, "tags:\"berlin; mitte\""), 1);
        // The quotes only shield what they wrap — chaining still works after them.
        assert_eq!(run(&mut app, "tags:\"berlin; mitte\"; tags:berlin"), 1);
    }

    #[test]
    fn a_quoted_value_may_hold_a_quote() {
        let mut app = app_with(vec![
            rich_entry("", &["a\";b"], &[], None),
            rich_entry("", &["a"], &[], None),
        ]);

        // What the chip and the filter browser build round-trips to the one tag.
        let query = format!("tags:{}", quote_filter_value("a\";b"));
        assert_eq!(run(&mut app, &query), 1);
    }
}
