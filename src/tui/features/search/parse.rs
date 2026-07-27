//! The query grammar: splitting a search query into segments, values and
//! alternatives, and building a query string back out of a stored value.

use std::borrow::Cow;

use notema_domain::DateBound;

/// Split a `date:`/`before:`/`after:` query into its bound and value.
pub(super) fn strip_date_prefix(query: &str) -> Option<(DateBound, Cow<'_, str>)> {
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
pub(super) fn split_unquoted(input: &str, sep: char) -> Vec<&str> {
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
pub(super) fn unquote(value: &str) -> Cow<'_, str> {
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

/// Split a filter value into AND-groups (on `+`) of OR-alternatives (on `|`),
/// unquoted, lowercased and trimmed, dropping empties — so `alice+bob|rob` is
/// alice AND (bob OR rob), and a quoted `"C++"` is one literal needle.
pub(super) fn split_values(query: &str) -> Vec<Vec<String>> {
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
}
