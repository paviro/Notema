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

impl Prefix {
    /// The token that parses back to this prefix, colon included. Read out of
    /// the same table [`split_prefix`] matches against, so anything writing a
    /// query writes one the parser recognizes.
    pub(crate) fn token(self) -> &'static str {
        PREFIXES
            .iter()
            .find_map(|(token, prefix)| (*prefix == self).then_some(*token))
            .unwrap_or_default()
    }
}

/// Split a segment into its filter prefix and the raw text after it; `None` is a
/// full-text segment. Matching is byte-exact, so `Tags:` and `tags :` are text.
pub(crate) fn split_prefix(segment: &str) -> Option<(Prefix, &str)> {
    PREFIXES
        .iter()
        .find_map(|(token, prefix)| segment.strip_prefix(token).map(|rest| (*prefix, rest)))
}

/// How a prefix builds its value out of parts. The query field colours operators
/// from this and the predicates split on it, so the field cannot paint an
/// operator over a character its own filter reads literally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ValueGrammar {
    /// `+`-groups of `|`-alternatives: all groups match, any alternative in one.
    Groups,
    /// `|`-alternatives alone.
    Alternatives,
    /// One value, whole.
    Whole,
}

impl Prefix {
    /// The shape of this prefix's value.
    ///
    /// `location:` takes alternatives but no `+`: an entry has one location, so
    /// ANDing two places could only ever match nothing, which leaves `+` free to
    /// be a character in a place name. The scalars take neither — they parse the
    /// whole value as a date, a score or a flag.
    pub(super) fn value_grammar(self) -> ValueGrammar {
        match self {
            Self::Tags | Self::People | Self::Activities | Self::Feelings => ValueGrammar::Groups,
            Self::Location => ValueGrammar::Alternatives,
            Self::Star | Self::Mood | Self::Date(_) => ValueGrammar::Whole,
        }
    }
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

/// Whether `value` opens a quote span that never closes — every quoted value
/// part-way through being typed. An odd count is where the toggle in
/// [`split_unquoted_ranges`] ends up, and a leading `"` is what makes the
/// unclosed one the value's own.
fn opens_unclosed_quote(value: &str) -> bool {
    value.starts_with('"') && value.matches('"').count() % 2 == 1
}

/// Strip a filter value's surrounding quotes, undoubling the quotes
/// [`quote_filter_value`] doubled.
///
/// A pair with no closing quote yet is closed here, so a value reads as the one
/// it is growing into: `"app` is `app`. Exactness stays [`is_quoted`]'s alone,
/// which is what makes typing that closing quote narrow a search rather than
/// replace it.
pub(super) fn unquote(value: &str) -> Cow<'_, str> {
    // Both ends are a one-byte `"`, so these slice on character boundaries.
    let inner = if is_quoted(value) {
        &value[1..value.len() - 1]
    } else if opens_unclosed_quote(value) {
        &value[1..]
    } else {
        return Cow::Borrowed(value);
    };
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
/// For the prefixes that *have* an exact mode — the four [`split_values`] facets.
/// It quotes unconditionally rather than only when the value holds a structural
/// character, because quoting is what makes a value exact: otherwise `work;shop`
/// would be exact while `work` beside it stayed a substring, and one tab's rows
/// would mean two different things. [`escape_filter_value`] carries a value under
/// the prefixes that have no exact mode.
pub(crate) fn quote_filter_value(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

/// The query fragment carrying `value` under a prefix with no exact mode —
/// `location:`, which tokenizes its value either way. Quotes there would claim an
/// exactness the predicate does not have, so they appear only when the grammar
/// would otherwise reach into the value: a bare `Berlin, Germany` stays bare.
///
/// The trigger is every character that would cut *this* value — the `;` between
/// filters and the `|` between alternatives, plus the `"` that would open a span
/// of its own. Not `+`, which [`ValueGrammar::Alternatives`] leaves as a
/// character, so a place with one in its name still arrives bare.
pub(crate) fn escape_filter_value(value: &str) -> Cow<'_, str> {
    if value.contains([';', '|', '"']) {
        Cow::Owned(quote_filter_value(value))
    } else {
        Cow::Borrowed(value)
    }
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
/// `"a"|b` is exactly-a or contains-b. An unclosed quote is no pair, so a
/// half-typed `"app` is the substring `app` — the reading `app` already has, and
/// one the closing quote narrows instead of discarding.
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
    use crate::tui::features::filter::FilterTab;
    use crate::tui::state::MetadataKind;

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
    /// back. Both take their token from `PREFIXES` now, so this mostly guards
    /// `Prefix::token`'s empty fallback and the round trip through
    /// `from_prefix`, which is how the search box knows whose values to offer.
    #[test]
    fn every_produced_prefix_parses_back() {
        for kind in [
            MetadataKind::Tags,
            MetadataKind::People,
            MetadataKind::Activities,
        ] {
            let segment = format!("{}x", kind.search_prefix());
            assert!(
                split_prefix(&segment).is_some(),
                "MetadataKind::{kind:?} produces {segment:?}"
            );
        }
        for tab in FilterTab::ALL {
            // The whole launched query, not just the prefix: a tab that quotes
            // its value has to leave the prefix reachable in front of it.
            let segment = tab.launch_query("x");
            let parsed = split_prefix(&segment);
            assert!(parsed.is_some(), "FilterTab::{tab:?} produces {segment:?}");
            // And back to the tab, which is how the search box knows whose
            // values to suggest for the prefix being typed.
            assert_eq!(
                parsed.and_then(|(prefix, _)| FilterTab::from_prefix(prefix)),
                Some(tab),
                "FilterTab::{tab:?} produces {segment:?}"
            );
        }
    }

    #[test]
    fn quote_filter_value_always_quotes() {
        // Plain values are quoted too: quoting is what makes a value exact, so a
        // tab whose rows quoted only the awkward ones would mean two things.
        assert_eq!(quote_filter_value("berlin"), "\"berlin\"");
        assert_eq!(quote_filter_value("c++"), "\"c++\"");
        assert_eq!(quote_filter_value("r&d|ops"), "\"r&d|ops\"");
        assert_eq!(quote_filter_value("berlin; mitte"), "\"berlin; mitte\"");
        // A quote is itself structural: it has to be doubled, so it can't close
        // the wrapping pair and let a later separator split the value.
        assert_eq!(quote_filter_value("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(quote_filter_value("a\";b"), "\"a\"\";b\"");
    }

    #[test]
    fn escaping_quotes_only_what_the_grammar_would_cut() {
        // The shapes a place name actually has: commas, spaces and hyphens are
        // nothing to the grammar, and quoting them would read as exactness.
        assert_eq!(escape_filter_value("Berlin, Germany"), "Berlin, Germany");
        assert_eq!(escape_filter_value("Clermont-Ferrand"), "Clermont-Ferrand");
        // `+` is not an operator under `location:`, so a place keeps it bare.
        assert_eq!(escape_filter_value("Rock + Roll"), "Rock + Roll");
        // A separator survives as a character instead of splitting the query.
        assert_eq!(escape_filter_value("Ville; Sud"), "\"Ville; Sud\"");
        assert_eq!(escape_filter_value("a|b"), "\"a|b\"");
        assert_eq!(escape_filter_value("say \"hi\""), "\"say \"\"hi\"\"\"");
        // Whichever branch ran, the value has to come back out whole.
        for value in ["Berlin, Germany", "Ville; Sud", "a|b", "say \"hi\""] {
            assert_eq!(unquote(&escape_filter_value(value)), value);
        }
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
        // A pair wraps a value; `"a"b` wraps nothing, so its quotes stay in the
        // needle as the characters they are.
        assert_eq!(only_needle("\"a\"b"), substring("\"a\"b"));
    }

    /// A quote with no partner is a value mid-typing, and reading it literally
    /// left a dead query in the middle of typing every quoted value. It reads as
    /// the value it is growing into instead.
    #[test]
    fn an_unclosed_quote_reads_as_the_value_it_is_growing_into() {
        // The same substring `app` alone means, so closing the quote narrows the
        // search rather than replacing it.
        assert_eq!(only_needle("\"app"), substring("app"));
        assert_eq!(only_needle("\"app\""), exact("app"));
        // Doubling is undone before the pair closes too, so the value the
        // fragment means does not change under it.
        assert_eq!(only_needle("\"say \"\"hi"), substring("say \"hi"));
        assert_eq!(only_needle("\"say \"\"hi\""), exact("say \"hi"));
        // Not trimmed inside, exactly like the pair it is becoming.
        assert_eq!(only_needle("\" wo"), substring(" wo"));
        // Only a leading quote opens a value: elsewhere it is a character, and
        // an even count closed whatever it opened.
        assert_eq!(only_needle("app\""), substring("app\""));
        assert_eq!(only_needle("\"\"app"), substring("\"\"app"));
        // A quote and nothing else is no needle at all, rather than one every
        // entry satisfies.
        assert_eq!(split_values("\""), Vec::<Vec<Needle>>::new());
    }
}
