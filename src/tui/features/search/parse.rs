//! The query grammar: splitting a search query into segments, values and
//! alternatives, and building a query string back out of a stored value.

use std::borrow::Cow;
use std::ops::Range;

use notema_domain::DateBound;

/// A filter prefix the query grammar recognises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Prefix {
    Tags,
    People,
    Activities,
    Feelings,
    Star,
    Location,
    Mood,
    Date(DateBound),
}

/// The whole vocabulary, in match order. One table so that what the parser binds
/// to a predicate and what the query field highlights as a prefix cannot drift:
/// a token the highlighter colours but the parser drops to full text would be
/// worse than no highlighting at all.
const PREFIXES: [(&str, Prefix); 10] = [
    ("tags:", Prefix::Tags),
    ("people:", Prefix::People),
    ("activities:", Prefix::Activities),
    ("feelings:", Prefix::Feelings),
    ("star:", Prefix::Star),
    ("location:", Prefix::Location),
    ("mood:", Prefix::Mood),
    ("date:", Prefix::Date(DateBound::On)),
    ("before:", Prefix::Date(DateBound::Before)),
    ("after:", Prefix::Date(DateBound::After)),
];

/// Split a segment into its filter prefix and the raw text after it; `None` is a
/// full-text segment. Matching is byte-exact, so `Tags:` and `tags :` are text.
pub(crate) fn split_prefix(segment: &str) -> Option<(Prefix, &str)> {
    PREFIXES
        .iter()
        .find_map(|(token, prefix)| segment.strip_prefix(token).map(|rest| (*prefix, rest)))
}

/// Where [`split_unquoted`] cuts. Each range excludes the separator that ended
/// it, so a piece's `end` is that separator's offset — except the last piece,
/// which ends at `input.len()`.
///
/// The quote toggle lives here alone: the query field locates the same pieces to
/// draw them, and a second implementation of the toggle would let the pill it
/// draws disagree with the value the parser matched.
pub(super) fn split_unquoted_ranges(input: &str, sep: char) -> Vec<Range<usize>> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    for (index, character) in input.char_indices() {
        if character == '"' {
            quoted = !quoted;
        } else if character == sep && !quoted {
            pieces.push(start..index);
            start = index + character.len_utf8();
        }
    }
    pieces.push(start..input.len());
    pieces
}

/// Split `input` on `sep`, ignoring separators inside a `"…"` span — how a value
/// carries a structural character (`;`, `+`, `|`) literally.
pub(super) fn split_unquoted(input: &str, sep: char) -> Vec<&str> {
    split_unquoted_ranges(input, sep)
        .into_iter()
        .map(|piece| &input[piece])
        .collect()
}

/// Whether `value` is wrapped in a quote pair — the parser's one signal that a
/// value is meant whole rather than as a fragment to grow. A lone `"` is not a
/// pair.
pub(super) fn is_quoted(value: &str) -> bool {
    value.len() >= 2 && value.starts_with('"') && value.ends_with('"')
}

/// Strip one surrounding pair of quotes from a filter value, undoubling the
/// quotes [`quote_filter_value`] doubled.
pub(super) fn unquote(value: &str) -> Cow<'_, str> {
    if !is_quoted(value) {
        return Cow::Borrowed(value);
    }
    // Both ends are a one-byte `"`, so this slices on character boundaries.
    let inner = &value[1..value.len() - 1];
    if inner.contains('"') {
        Cow::Owned(inner.replace("\"\"", "\""))
    } else {
        Cow::Borrowed(inner)
    }
}

/// The query fragment meaning exactly `value`: wrapped in quotes, with any `"`
/// inside doubled so it can't close the pair early and expose a `;`/`+`/`|` after
/// it to the splitters.
///
/// Everything that builds a query from a value the user already has — the chip
/// searches, the filter browser's rows — goes through this. It quotes
/// unconditionally rather than only when the value holds a structural character,
/// because quoting is what makes a value exact: otherwise `work;shop` would be
/// exact while `work` beside it stayed a substring, and one tab's rows would mean
/// two different things.
pub(crate) fn quote_filter_value(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

/// One `|`-alternative of a filter value: the text to match, lowercased, and how
/// to match it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Needle {
    pub(super) text: String,
    pub(super) exact: bool,
}

impl Needle {
    /// Whether one of an entry's values satisfies this needle. Folds with
    /// `to_lowercase` — not `eq_ignore_ascii_case` — because the filter browser
    /// buckets its rows the same way, and an ASCII-only fold would list `Ärger`
    /// and `ärger` as one row whose own search returned two.
    pub(super) fn matches(&self, value: &str) -> bool {
        let value = value.to_lowercase();
        if self.exact {
            value == self.text
        } else {
            value.contains(&self.text)
        }
    }
}

/// Split a filter value into AND-groups (on `+`) of OR-alternatives (on `|`),
/// lowercased and dropping empties — so `alice+bob|rob` is alice AND (bob OR
/// rob).
///
/// Quotedness is per alternative, because the `|` split runs before unquoting:
/// `"a"|b` is exactly-a or contains-b. Unbalanced quotes are not a pair, so a
/// half-typed `"app` keeps its literal `"` and matches nothing.
pub(super) fn split_values(query: &str) -> Vec<Vec<Needle>> {
    split_unquoted(query, '+')
        .into_iter()
        .map(|group| {
            split_unquoted(group, '|')
                .into_iter()
                .filter_map(|value| {
                    let value = value.trim();
                    let exact = is_quoted(value);
                    // Deliberately not trimmed again after unquoting: a quoted
                    // value is the bytes between the quotes, so a tag stored as
                    // `" work "` stays reachable — and its filter row stays
                    // honest, where trimming would make the row's own search
                    // return the `work` entries too.
                    let text = unquote(value).to_lowercase();
                    (!text.is_empty()).then_some(Needle { text, exact })
                })
                .collect::<Vec<_>>()
        })
        .filter(|group| !group.is_empty())
        .collect()
}

/// Parse the value of a `star:` query. An empty value is the friendlier
/// `true` (the common intent when filtering for favorites); `true`/`1` and
/// `false`/`0` are accepted case-insensitively; anything else is `None` (no
/// match).
pub(super) fn parse_starred_value(value: &str) -> Option<bool> {
    match value.trim().to_lowercase().as_str() {
        "" | "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::state::{FilterTab, MetadataKind};

    /// The one needle a value must parse back to, or `None` if it produced no
    /// group at all.
    fn only_needle(query: &str) -> Option<Needle> {
        let mut groups = split_values(query);
        assert!(
            groups.len() <= 1,
            "{query:?} split into {} groups",
            groups.len()
        );
        let mut alternatives = groups.pop()?;
        assert_eq!(alternatives.len(), 1, "{query:?} split into alternatives");
        alternatives.pop()
    }

    fn exact(text: &str) -> Option<Needle> {
        Some(Needle {
            text: text.to_string(),
            exact: true,
        })
    }

    fn substring(text: &str) -> Option<Needle> {
        Some(Needle {
            text: text.to_string(),
            exact: false,
        })
    }

    #[test]
    fn a_prefix_is_matched_byte_exactly() {
        assert_eq!(split_prefix("tags:work"), Some((Prefix::Tags, "work")));
        assert_eq!(
            split_prefix("before:2026"),
            Some((Prefix::Date(DateBound::Before), "2026"))
        );
        // Case and a space before the colon both fall through to full text.
        assert_eq!(split_prefix("Tags:work"), None);
        assert_eq!(split_prefix("tags :work"), None);
        assert_eq!(split_prefix("tag:work"), None);
        // The value is handed over raw; trimming and unquoting are the caller's.
        assert_eq!(
            split_prefix("tags: \"a\" "),
            Some((Prefix::Tags, " \"a\" "))
        );
    }

    /// The two enums that *build* queries must name prefixes the parser reads
    /// back. They hold their own `&'static str`, so nothing but this stops a
    /// filter row from launching a search that silently ran as full text.
    #[test]
    fn every_produced_prefix_parses_back() {
        for kind in [
            MetadataKind::Tags,
            MetadataKind::People,
            MetadataKind::Activities,
        ] {
            let segment = format!("{}:x", kind.search_prefix());
            assert!(
                split_prefix(&segment).is_some(),
                "MetadataKind::{kind:?} produces {segment:?}"
            );
        }
        for tab in FilterTab::ALL {
            let segment = format!("{}:x", tab.search_prefix());
            assert!(
                split_prefix(&segment).is_some(),
                "FilterTab::{tab:?} produces {segment:?}"
            );
        }
    }

    #[test]
    fn quote_filter_value_always_quotes() {
        // Plain values are quoted too: quoting is what makes a value exact, so a
        // tab whose rows quoted only the awkward ones would mean two things.
        assert_eq!(quote_filter_value("berlin"), "\"berlin\"");
        assert_eq!(quote_filter_value("Berlin, Germany"), "\"Berlin, Germany\"");
        assert_eq!(quote_filter_value("c++"), "\"c++\"");
        assert_eq!(quote_filter_value("r&d|ops"), "\"r&d|ops\"");
        assert_eq!(quote_filter_value("berlin; mitte"), "\"berlin; mitte\"");
        // A quote is itself structural: it has to be doubled, so it can't close
        // the wrapping pair and let a later separator split the value.
        assert_eq!(quote_filter_value("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(quote_filter_value("a\";b"), "\"a\"\";b\"");
    }

    /// What the filter browser and the chips build has to parse back to the one
    /// value they were built from. Everything else here rests on this.
    #[test]
    fn quoting_round_trips_a_value_to_one_exact_needle() {
        for value in [
            "berlin",
            "Berlin, Germany",
            "c++",
            "r&d|ops",
            "berlin; mitte",
            "say \"hi\"",
            "a\";b",
            " work ",
            "Ärger",
            "work;shop",
        ] {
            assert_eq!(
                only_needle(&quote_filter_value(value)),
                exact(&value.to_lowercase()),
                "round trip of {value:?}"
            );
        }
        // A value that folds to nothing produces no group, so its search matches
        // nothing rather than everything.
        assert_eq!(
            split_values(&quote_filter_value("")),
            Vec::<Vec<Needle>>::new()
        );
    }

    #[test]
    fn an_unquoted_value_is_trimmed_and_a_quoted_one_is_not() {
        // The outer trim handles the unquoted case…
        assert_eq!(only_needle(" work "), substring("work"));
        // …but a quoted value is exactly the bytes between the quotes, which is
        // what keeps a tag stored as `" work "` distinguishable from `work`.
        assert_eq!(only_needle("\" work \""), exact(" work "));
    }

    #[test]
    fn quoting_is_per_alternative_and_needs_a_pair() {
        let groups = split_values("\"a\"|b");
        assert_eq!(
            groups,
            vec![vec![
                Needle {
                    text: "a".to_string(),
                    exact: true
                },
                Needle {
                    text: "b".to_string(),
                    exact: false
                },
            ]]
        );
        // Unbalanced quotes are not a pair, so the `"` stays in the needle and
        // matches nothing — including while a value is half-typed.
        assert_eq!(only_needle("\"a\"b"), substring("\"a\"b"));
        assert_eq!(only_needle("\"app"), substring("\"app"));
    }
}
