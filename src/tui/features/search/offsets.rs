//! Where a query's pieces sit in its text.
//!
//! [`parse`](super::parse) answers what a query *means*; this answers where each
//! part of it is, as byte ranges into the query string, so the field can draw
//! the grammar it is about to run. Both walk the same splitters, which is what
//! keeps a pill's reach equal to the value the parser matched.
//!
//! The [`Segment`] here is a location, not the classification `super::Segment`
//! makes of the same piece.

use std::iter::once;
use std::ops::Range;

use super::parse::{Prefix, ValueGrammar, is_quoted, split_prefix, split_unquoted_ranges};

/// One `;`-separated piece of a query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Segment {
    /// The piece as split, separators excluded and untrimmed.
    pub(crate) range: Range<usize>,
    /// The `;` that ended it, if it is not the last piece.
    pub(crate) separator: Option<usize>,
    /// The recognised prefix and the token's range. `None` is a full-text piece,
    /// which has no operators or alternatives either — `+` and `|` are only
    /// structural under a prefix.
    pub(crate) prefix: Option<(Prefix, Range<usize>)>,
    /// The `+` and `|` between the alternatives, in text order.
    pub(crate) operators: Vec<(char, usize)>,
    /// The `|`-alternatives of the value, in text order, flattened across the
    /// `+`-groups: both operators separate alternatives, and the difference
    /// between all-of and any-of does not change how one is drawn.
    pub(crate) alternatives: Vec<Alternative>,
}

/// One alternative of a filter value — the unit quoting makes exact or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Alternative {
    /// The trimmed text, which is what the parser tests for quotedness.
    pub(crate) range: Range<usize>,
    /// Wrapped in a quote pair, i.e. matched exactly rather than as a fragment.
    pub(crate) quoted: bool,
}

/// Locate every segment of `query`.
pub(crate) fn scan(query: &str) -> Vec<Segment> {
    let pieces = split_unquoted_ranges(query, ';');
    let last = pieces.len() - 1;
    pieces
        .into_iter()
        .enumerate()
        .map(|(index, piece)| {
            let separator = (index < last).then_some(piece.end);
            scan_segment(query, piece, separator)
        })
        .collect()
}

fn scan_segment(query: &str, piece: Range<usize>, separator: Option<usize>) -> Segment {
    let trimmed = trim_range(query, piece.clone());
    let plain = Segment {
        range: piece,
        separator,
        prefix: None,
        operators: Vec::new(),
        alternatives: Vec::new(),
    };
    let Some((prefix, rest)) = split_prefix(&query[trimmed.clone()]) else {
        return plain;
    };

    let value = trimmed.end - rest.len()..trimmed.end;
    let mut operators = Vec::new();
    let mut alternatives = Vec::new();
    let grammar = prefix.value_grammar();
    // A prefix that does not group is scanned as a single group, so the `|` pass
    // below is the only one that cuts it; one that takes no operators at all is
    // one alternative spanning the whole value.
    let groups = match grammar {
        ValueGrammar::Groups => split_unquoted_ranges(&query[value.clone()], '+'),
        _ => once(0..value.len()).collect(),
    };
    let last_group = groups.len() - 1;
    for (index, group) in groups.into_iter().enumerate() {
        let group = shift(group, value.start);
        if index < last_group {
            operators.push(('+', group.end));
        }
        let alts = match grammar {
            ValueGrammar::Whole => once(0..group.len()).collect(),
            _ => split_unquoted_ranges(&query[group.clone()], '|'),
        };
        let last_alt = alts.len() - 1;
        for (index, alt) in alts.into_iter().enumerate() {
            let alt = shift(alt, group.start);
            if index < last_alt {
                operators.push(('|', alt.end));
            }
            let range = trim_range(query, alt);
            alternatives.push(Alternative {
                quoted: is_quoted(&query[range.clone()]),
                range,
            });
        }
    }

    Segment {
        prefix: Some((prefix, trimmed.start..value.start)),
        operators,
        alternatives,
        ..plain
    }
}

/// The filter value the caret is inside: which prefix it is under, where the
/// value sits, and whether it is already a balanced quoted pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Caret {
    pub(crate) prefix: Prefix,
    /// The alternative's trimmed range — what completing it would replace.
    pub(crate) value: Range<usize>,
    pub(crate) quoted: bool,
}

/// The filter value at byte offset `at`, or `None` when the caret is in a
/// full-text segment or under an unrecognised prefix.
///
/// A range is matched inclusively at both ends, so a caret sitting just past the
/// last character still belongs to the value it is extending — which is where it
/// is for all of typing. Under a prefix there is always at least one alternative
/// (a bare `tags:` yields an empty one at the value start), so an empty value
/// needs no special case.
pub(crate) fn caret_context(query: &str, at: usize) -> Option<Caret> {
    let segment = scan(query)
        .into_iter()
        .find(|segment| segment.range.contains(&at) || segment.range.end == at)?;
    let (prefix, _) = segment.prefix?;
    let alternative = segment
        .alternatives
        .into_iter()
        .find(|alt| alt.range.contains(&at) || alt.range.end == at)?;
    Some(Caret {
        prefix,
        value: alternative.range,
        quoted: alternative.quoted,
    })
}

fn shift(range: Range<usize>, by: usize) -> Range<usize> {
    range.start + by..range.end + by
}

/// `range` narrowed to its trimmed content, or an empty range at its start when
/// there is nothing but whitespace in it.
fn trim_range(text: &str, range: Range<usize>) -> Range<usize> {
    let slice = &text[range.clone()];
    let trimmed = slice.trim();
    if trimmed.is_empty() {
        return range.start..range.start;
    }
    let start = range.start + (slice.len() - slice.trim_start().len());
    start..start + trimmed.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One segment as text: its prefix token, its operators, and its
    /// alternatives with their quotedness.
    type Parts<'a> = (Option<&'a str>, Vec<char>, Vec<(&'a str, bool)>);

    /// Every segment of `query` as [`Parts`], so the expectations below read the
    /// way the query does.
    fn parts(query: &str) -> Vec<Parts<'_>> {
        scan(query)
            .into_iter()
            .map(|segment| {
                (
                    segment.prefix.map(|(_, range)| &query[range]),
                    segment.operators.iter().map(|&(op, _)| op).collect(),
                    segment
                        .alternatives
                        .iter()
                        .map(|alt| (&query[alt.range.clone()], alt.quoted))
                        .collect(),
                )
            })
            .collect()
    }

    #[test]
    fn a_prefixed_segment_splits_into_alternatives() {
        assert_eq!(
            parts("tags:work"),
            vec![(Some("tags:"), vec![], vec![("work", false)])]
        );
        assert_eq!(
            parts("tags:a+b|c"),
            vec![(
                Some("tags:"),
                vec!['+', '|'],
                vec![("a", false), ("b", false), ("c", false)]
            )]
        );
    }

    #[test]
    fn a_segment_without_a_prefix_has_no_operators() {
        // `+` and `|` are only structural under a prefix; in full text they are
        // characters, and drawing them as operators would say otherwise.
        assert_eq!(parts("beach+sun"), vec![(None, vec![], vec![])]);
        assert_eq!(
            parts("beach; tags:x"),
            vec![
                (None, vec![], vec![]),
                (Some("tags:"), vec![], vec![("x", false)]),
            ]
        );
    }

    #[test]
    fn a_quoted_alternative_is_marked_and_keeps_its_quotes() {
        // The range covers the quotes: they are the alternative's own bytes, and
        // the field decides how to draw them.
        assert_eq!(
            parts("tags:\"apple\"+\"banana\""),
            vec![(
                Some("tags:"),
                vec!['+'],
                vec![("\"apple\"", true), ("\"banana\"", true)]
            )]
        );
        // Half-typed, and unbalanced: not a pair, so not exact.
        assert_eq!(
            parts("tags:\"app"),
            vec![(Some("tags:"), vec![], vec![("\"app", false)])]
        );
    }

    /// Deleting one delimiter of a chip re-reads the rest of the value as part
    /// of it. The field draws one pill over the lot because that is what the
    /// parser now matches — the two agree because they share the splitter.
    #[test]
    fn an_unbalanced_quote_swallows_the_operator_after_it() {
        assert_eq!(
            parts("tags:\"apple+\"banana\""),
            vec![(Some("tags:"), vec![], vec![("\"apple+\"banana\"", true)])]
        );
    }

    #[test]
    fn a_structural_character_inside_quotes_is_not_a_separator() {
        assert_eq!(
            parts("tags:\"a;b\""),
            vec![(Some("tags:"), vec![], vec![("\"a;b\"", true)])]
        );
        assert_eq!(
            parts("tags:\"a+b\"|c"),
            vec![(
                Some("tags:"),
                vec!['|'],
                vec![("\"a+b\"", true), ("c", false)]
            )]
        );
    }

    #[test]
    fn whitespace_is_trimmed_off_the_located_ranges() {
        let query = "  tags: a | b  ";
        assert_eq!(
            parts(query),
            vec![(Some("tags:"), vec!['|'], vec![("a", false), ("b", false)])]
        );
        // A wholly empty value locates one empty alternative rather than none.
        assert_eq!(
            parts("tags:  "),
            vec![(Some("tags:"), vec![], vec![("", false)])]
        );
    }

    #[test]
    fn separators_point_at_the_semicolons() {
        let query = "tags:x; people:y";
        let segments = scan(query);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].separator, Some(6));
        assert_eq!(&query[6..7], ";");
        assert_eq!(segments[1].separator, None);
    }

    #[test]
    fn operator_offsets_point_at_the_operators() {
        let query = "tags:a+b|c";
        let segment = &scan(query)[0];
        for &(operator, offset) in &segment.operators {
            assert_eq!(query[offset..].chars().next(), Some(operator));
        }
    }

    /// A prefix is only scanned for the operators it actually splits on, so the
    /// query field cannot colour a character its own filter matches literally.
    #[test]
    fn a_prefix_is_scanned_for_its_own_operators_only() {
        // `location:` takes alternatives but no groups: the `|` is an operator
        // and cuts, the `+` is part of the place name.
        let segment = &scan("location:berlin|paris")[0];
        assert_eq!(segment.operators, vec![('|', 15)]);
        assert_eq!(
            alternatives("location:berlin|paris"),
            vec!["berlin", "paris"]
        );

        let segment = &scan("location:Rock + Roll")[0];
        assert!(segment.operators.is_empty());
        assert_eq!(alternatives("location:Rock + Roll"), vec!["Rock + Roll"]);

        // The scalars take neither — they parse the whole value.
        for query in ["mood:3|4", "star:a+b", "date:2026|2027"] {
            assert!(scan(query)[0].operators.is_empty(), "{query}");
        }
        assert_eq!(alternatives("mood:3|4"), vec!["3|4"]);

        // The token facets are unchanged: `+` groups, `|` alternates.
        let segment = &scan("tags:a+b|c")[0];
        assert_eq!(segment.operators, vec![('+', 6), ('|', 8)]);
    }

    /// The value the caret is in, as `(text, quoted)`.
    fn at(query: &str, caret: usize) -> Option<(&str, bool)> {
        caret_context(query, caret).map(|found| (&query[found.value], found.quoted))
    }

    /// What the suggestion list completes is the value the caret is inside, so
    /// the caret has to find it from every position typing puts it in.
    #[test]
    fn the_caret_finds_the_value_it_is_extending() {
        // Just after the prefix: an empty value, which is what opens the list on
        // the whole vocabulary.
        assert_eq!(at("tags:", 5), Some(("", false)));
        // Mid-value and at its end — the position for all of typing.
        assert_eq!(at("tags:app", 6), Some(("app", false)));
        assert_eq!(at("tags:app", 8), Some(("app", false)));
        // Whitespace after the prefix is not part of the value.
        assert_eq!(at("tags: app", 9), Some(("app", false)));

        // A committed value: the caret lands past the closing quote and the pair
        // is balanced, which is what tells the list to stay shut.
        assert_eq!(at("tags:\"apple\"", 12), Some(("\"apple\"", true)));
        // Break the pair and it is an ordinary fragment again.
        assert_eq!(at("tags:\"apple", 11), Some(("\"apple", false)));

        // Between two alternatives, each side belongs to the value it touches.
        assert_eq!(at("tags:a|b", 6), Some(("a", false)));
        assert_eq!(at("tags:a|b", 7), Some(("b", false)));
        // And across a `;`, the second filter is its own value.
        assert_eq!(at("tags:a; people:bo", 17), Some(("bo", false)));

        // `location:` takes `|` but not `+`, so a place keeps its plus.
        assert_eq!(at("location:Rock + Roll", 20), Some(("Rock + Roll", false)));
        assert_eq!(at("location:berlin|par", 19), Some(("par", false)));

        // Nothing to complete: full text, and a prefix the parser does not know.
        assert_eq!(at("apple", 5), None);
        assert_eq!(at("tag:apple", 9), None);
    }

    /// The trimmed text of each alternative, in order.
    fn alternatives(query: &str) -> Vec<&str> {
        scan(query)[0]
            .alternatives
            .iter()
            .map(|alt| &query[alt.range.clone()])
            .collect()
    }

    /// Every offset feeds a byte-range API that slices the query, and a slice
    /// through the middle of a char panics rather than misdraws.
    #[test]
    fn every_offset_lands_on_a_char_boundary() {
        for query in [
            "tags:\"Ärger\"+\"日本\"|ü",
            "location:Köln; tags:\"süß\"",
            "Ärger",
            "tags:\"Ä",
        ] {
            for segment in scan(query) {
                let mut offsets = vec![segment.range.start, segment.range.end];
                offsets.extend(segment.separator);
                if let Some((_, range)) = &segment.prefix {
                    offsets.extend([range.start, range.end]);
                }
                offsets.extend(segment.operators.iter().map(|&(_, at)| at));
                offsets.extend(
                    segment
                        .alternatives
                        .iter()
                        .flat_map(|alt| [alt.range.start, alt.range.end]),
                );
                for offset in offsets {
                    assert!(
                        query.is_char_boundary(offset),
                        "{offset} splits a char of {query:?}"
                    );
                }
            }
        }
    }
}
