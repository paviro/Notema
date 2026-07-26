use std::{borrow::Cow, time::Instant};

use chrono::{Local, NaiveDate};
use notema_domain::{
    DateBound, DateFilter, DateSpec, Entry, EntryEncryptionState, MOOD_RANGE, SearchHit,
    entry_group_date, feeling_matches_search,
};

use crate::tui::{
    app::{AppModel, Focus, Mode, SearchScope},
    features::metadata::metadata_values,
    search::search_loaded_entries_where,
    state::MetadataKind,
};

/// Boxed so segments with different predicate types share one `Vec`.
type EntryPredicate = Box<dyn Fn(&Entry) -> bool>;

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
                Segment::Filter(predicate) => predicates.push(predicate),
                // A known filter with an unreadable value zeroes the whole query,
                // as a lone unreadable filter always has.
                Segment::NoMatch => return Vec::new(),
                Segment::Text(text) => text_parts.push(text),
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
}

/// One `;`-separated piece of a search query.
enum Segment<'a> {
    /// A recognized `prefix:` filter, as a predicate to AND with the others.
    Filter(EntryPredicate),
    /// A recognized filter whose value couldn't be read (e.g. `date:garbage`);
    /// it zeroes the whole query rather than matching everything.
    NoMatch,
    /// No known prefix: full-text, to be AND-ed with the filter predicates.
    Text(&'a str),
}

/// Classify one query segment. `today` is threaded in so date matching stays pure.
fn classify_segment(segment: &str, today: NaiveDate) -> Segment<'_> {
    if let Some(tag) = segment.strip_prefix("tags:") {
        Segment::Filter(Box::new(metadata_predicate(MetadataKind::Tags, tag.trim())))
    } else if let Some(person) = segment.strip_prefix("people:") {
        Segment::Filter(Box::new(metadata_predicate(
            MetadataKind::People,
            person.trim(),
        )))
    } else if let Some(activity) = segment.strip_prefix("activities:") {
        Segment::Filter(Box::new(metadata_predicate(
            MetadataKind::Activities,
            activity.trim(),
        )))
    } else if let Some(feeling) = segment.strip_prefix("feelings:") {
        Segment::Filter(Box::new(feeling_predicate(feeling.trim())))
    } else if let Some(value) = segment.strip_prefix("star:") {
        match parse_starred_value(&unquote(value.trim())) {
            Some(want) => Segment::Filter(Box::new(move |entry: &Entry| entry.starred == want)),
            None => Segment::NoMatch,
        }
    } else if let Some(place) = segment.strip_prefix("location:") {
        Segment::Filter(Box::new(location_predicate(&unquote(place.trim()))))
    } else if let Some(mood) = segment.strip_prefix("mood:") {
        match unquote(mood.trim()).parse::<i8>() {
            Ok(score) if MOOD_RANGE.contains(&score) => {
                Segment::Filter(Box::new(move |entry: &Entry| entry.mood == Some(score)))
            }
            _ => Segment::NoMatch,
        }
    } else if let Some((bound, value)) = strip_date_prefix(segment) {
        match DateSpec::parse(&value) {
            Some(spec) => {
                Segment::Filter(Box::new(date_predicate(DateFilter { bound, spec }, today)))
            }
            None => Segment::NoMatch,
        }
    } else {
        Segment::Text(segment)
    }
}

/// Split a `date:`/`before:`/`after:` query into its bound and value.
fn strip_date_prefix(query: &str) -> Option<(DateBound, Cow<'_, str>)> {
    [
        ("date:", DateBound::On),
        ("before:", DateBound::Before),
        ("after:", DateBound::After),
    ]
    .into_iter()
    .find_map(|(prefix, bound)| {
        query
            .strip_prefix(prefix)
            .map(|value| (bound, unquote(value.trim())))
    })
}

/// Split `input` on `sep`, ignoring separators inside a `"…"` span — how a value
/// carries a structural character (`;`, `+`, `|`) literally.
fn split_unquoted(input: &str, sep: char) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    for (index, character) in input.char_indices() {
        if character == '"' {
            quoted = !quoted;
        } else if character == sep && !quoted {
            pieces.push(&input[start..index]);
            start = index + character.len_utf8();
        }
    }
    pieces.push(&input[start..]);
    pieces
}

/// Strip one surrounding pair of quotes from a filter value, undoubling the
/// quotes [`quote_filter_value`] doubled.
fn unquote(value: &str) -> Cow<'_, str> {
    match value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        Some(inner) if inner.contains('"') => Cow::Owned(inner.replace("\"\"", "\"")),
        Some(inner) => Cow::Borrowed(inner),
        None => Cow::Borrowed(value),
    }
}

/// Quote `value` if it holds a character the parser would read as structure.
/// Everything building a query from a stored value — the chip searches, the
/// filter browser's rows *and* its counts — goes through this, so what a row
/// counts, launches, and displays stay the same set.
///
/// A `"` inside the value is doubled, so it can't close the wrapping pair early
/// and expose a `;`/`+`/`|` after it to the splitters.
pub(crate) fn quote_filter_value(value: &str) -> Cow<'_, str> {
    if value.contains([';', '+', '|', '"']) {
        Cow::Owned(format!("\"{}\"", value.replace('"', "\"\"")))
    } else {
        Cow::Borrowed(value)
    }
}

/// Whether an entry's date satisfies `filter`. Entries with neither a creation
/// timestamp nor a dated filename have no date to compare, so they never match.
pub(crate) fn date_predicate(
    filter: DateFilter,
    today: NaiveDate,
) -> impl Fn(&Entry) -> bool + use<> {
    move |entry| entry_group_date(entry).is_some_and(|date| filter.matches(date, today))
}

/// Whether an entry matches a `tags:`/`people:`/`activities:` search: every
/// `+`-group in `query` must match, where a group matches if *any* of its
/// `|`-alternatives is contained (case-insensitively) in one of the entry's
/// `kind` values. The filter browser's row counter and the launched search share
/// this predicate (and the location/feeling ones below), so a row's count always
/// equals the hits its search returns; the value split lives here, not in the
/// parser, to keep that true.
pub(crate) fn metadata_predicate(
    kind: MetadataKind,
    query: &str,
) -> impl Fn(&Entry) -> bool + use<> {
    let groups = split_values(query);
    move |entry| {
        let values = metadata_values(entry, kind);
        !groups.is_empty()
            && groups.iter().all(|alternatives| {
                alternatives.iter().any(|needle| {
                    values
                        .iter()
                        .any(|value| value.to_lowercase().contains(needle))
                })
            })
    }
}

/// Split a filter value into AND-groups (on `+`) of OR-alternatives (on `|`),
/// unquoted, lowercased and trimmed, dropping empties — so `alice+bob|rob` is
/// alice AND (bob OR rob), and a quoted `"C++"` is one literal needle.
fn split_values(query: &str) -> Vec<Vec<String>> {
    split_unquoted(query, '+')
        .into_iter()
        .map(|group| {
            split_unquoted(group, '|')
                .into_iter()
                .map(|value| unquote(value.trim()).trim().to_lowercase())
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|group| !group.is_empty())
        .collect()
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

/// Whether an entry matches a `feelings:` search: the [`split_values`] groups
/// applied to the entry's feelings through [`feeling_matches_search`], which is
/// alias-aware and lowercases its own query.
pub(crate) fn feeling_predicate(feeling: &str) -> impl Fn(&Entry) -> bool + use<> {
    let groups = split_values(feeling);
    move |entry| {
        !groups.is_empty()
            && groups.iter().all(|alternatives| {
                alternatives.iter().any(|needle| {
                    entry
                        .feelings
                        .iter()
                        .any(|entry_feeling| feeling_matches_search(entry_feeling, needle))
                })
            })
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

    #[test]
    fn a_bare_prefix_matches_nothing() {
        let mut app = app_with(vec![rich_entry("", &["work"], &[], None)]);

        // No value to match on, so the filter is empty rather than universal.
        assert_eq!(run(&mut app, "tags:"), 0);
        assert_eq!(run(&mut app, "feelings:"), 0);
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
    fn quote_filter_value_quotes_only_when_needed() {
        assert_eq!(quote_filter_value("berlin"), "berlin");
        assert_eq!(quote_filter_value("Berlin, Germany"), "Berlin, Germany");
        assert_eq!(quote_filter_value("c++"), "\"c++\"");
        assert_eq!(quote_filter_value("r&d|ops"), "\"r&d|ops\"");
        assert_eq!(quote_filter_value("berlin; mitte"), "\"berlin; mitte\"");
        // A quote is itself structural: it has to be doubled, so it can't close
        // the wrapping pair and let a later separator split the value.
        assert_eq!(quote_filter_value("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(quote_filter_value("a\";b"), "\"a\"\";b\"");
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
