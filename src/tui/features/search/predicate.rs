//! Entry matchers: what a parsed filter value selects.

use chrono::NaiveDate;
use notema_domain::{
    DateFilter, Entry, EntryEncryptionState, entry_group_date, feeling_matches_search,
};

use super::parse::split_values;
use crate::tui::{app::SearchScope, features::metadata::metadata_values, state::MetadataKind};

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
