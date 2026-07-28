//! Entry matchers: what a parsed filter value selects.

use chrono::NaiveDate;
use notema_domain::{
    DateFilter, Entry, FeelingMatch, Location, entry_group_date, feeling_matches_search,
};

use super::parse::{split_unquoted, split_values, unquote};
use crate::tui::{features::metadata::metadata_values, state::MetadataKind};

/// Whether an entry's date satisfies `filter`. Entries with neither a creation
/// timestamp nor a dated filename have no date to compare, so they never match.
pub(super) fn date_predicate(
    filter: DateFilter,
    today: NaiveDate,
) -> impl Fn(&Entry) -> bool + use<> {
    move |entry| entry_group_date(entry).is_some_and(|date| filter.matches(date, today))
}

/// Whether an entry matches a `tags:`/`people:`/`activities:` search: every
/// `+`-group in `query` must match, where a group matches if *any* of its
/// `|`-alternatives matches one of the entry's `kind` values.
///
/// An alternative is a substring unless it was quoted, so `tags:app` keeps
/// narrowing to `apple` as it is typed while `tags:"app"` — what a chip or a
/// filter row commits — means that tag and no other. Quoting is the closed pair:
/// `tags:"app` is the substring still, so a value narrows while it is typed
/// whichever way it is being written.
pub(super) fn metadata_predicate(
    kind: MetadataKind,
    query: &str,
) -> impl Fn(&Entry) -> bool + use<> {
    let groups = split_values(query);
    move |entry| {
        let values = metadata_values(entry, kind);
        !groups.is_empty()
            && groups.iter().all(|alternatives| {
                alternatives
                    .iter()
                    .any(|needle| values.iter().any(|value| needle.matches(value)))
            })
    }
}

/// Whether an entry matches a `location:` search: every word of some
/// `|`-alternative appears (case-insensitively, any order) somewhere in the
/// location's [`search_haystack`](notema_domain::Location::search_haystack).
///
/// Alternatives only — there is no `+` counterpart, because an entry has one
/// location and requiring two places at once could only ever match nothing.
/// Quotedness is not exactness here (a place still contains the places around
/// it); it only keeps a `;` or `|` in a name from cutting the value, so the
/// unquoting is per alternative, after the split.
pub(super) fn location_predicate(query: &str) -> impl Fn(&Entry) -> bool + use<> {
    let alternatives: Vec<Vec<String>> = split_unquoted(query, '|')
        .into_iter()
        .map(|alternative| Location::search_tokens(&unquote(alternative.trim())))
        // An empty or punctuation-only alternative matches nothing, rather than
        // everything — `location:berlin|` is still just Berlin.
        .filter(|needles| !needles.is_empty())
        .collect();
    move |entry| {
        !alternatives.is_empty()
            && entry.location.as_ref().is_some_and(|location| {
                let haystack = location.search_haystack();
                alternatives
                    .iter()
                    .any(|needles| needles.iter().all(|needle| haystack.contains(needle)))
            })
    }
}

/// Whether an entry matches a `feelings:` search: the [`split_values`] groups
/// applied to the entry's feelings through [`feeling_matches_search`], which
/// resolves aliases so `feelings:"joyous"` reaches `happy`.
///
/// Exactness matters more here than elsewhere, because `happy`/`unhappy` and
/// `interested`/`uninterested` are each two canonical feelings, one holding the
/// other's name.
pub(super) fn feeling_predicate(feeling: &str) -> impl Fn(&Entry) -> bool + use<> {
    // The mode is a property of the needle, so resolve it once rather than once
    // per entry per feeling.
    let groups: Vec<Vec<(String, FeelingMatch)>> = split_values(feeling)
        .into_iter()
        .map(|alternatives| {
            alternatives
                .into_iter()
                .map(|needle| {
                    let mode = if needle.exact {
                        FeelingMatch::Exact
                    } else {
                        FeelingMatch::Contains
                    };
                    (needle.text, mode)
                })
                .collect()
        })
        .collect();
    move |entry| {
        !groups.is_empty()
            && groups.iter().all(|alternatives| {
                alternatives.iter().any(|(needle, mode)| {
                    entry
                        .feelings
                        .iter()
                        .any(|entry_feeling| feeling_matches_search(entry_feeling, needle, *mode))
                })
            })
    }
}
