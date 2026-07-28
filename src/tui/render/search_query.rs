//! Styling for the search field's own text.
//!
//! The query box holds a small language, and until now it drew as flat text: a
//! mistyped `tag:` looked exactly like a working `tags:` and only the results
//! said otherwise. These spans colour what the parser *recognised*, so the two
//! cases are distinguishable while typing.
//!
//! It also draws a quoted facet value as a pill. A chip is not a new kind of
//! thing in the query — it is how `tags:"apple"` *looks*, so the text stays the
//! one source of truth and a hand-typed value chips exactly like a launched one.
//! Delete either quote and the pill dissolves, because the render is a function
//! of the text and nothing else.
//!
//! Output is `(start_byte, end_byte, Style)` for `TextArea::set_syntax_spans`
//! plus `(byte_offset, replacement)` for `TextArea::set_glyph_substitutions`.

use ratatui::style::{Modifier, Style};

use crate::tui::features::search::{Prefix, offsets};
use crate::tui::theme::{PillCategory, PillStyle, Theme};

/// How the query field draws itself: styled ranges, and the delimiters that
/// render as something else.
#[derive(Debug, Default, PartialEq)]
pub(super) struct QueryStyling {
    pub(super) spans: Vec<(usize, usize, Style)>,
    pub(super) substitutions: Vec<(usize, char)>,
}

/// Style the query's grammar — the prefix that parsed, the `+`/`|` between its
/// alternatives, the `;` between filters — and draw each quoted facet value as a
/// pill. Everything else is full text and stays plain, because that is what it is.
pub(super) fn query_styling(theme: &Theme, query: &str) -> QueryStyling {
    let syntax = theme.syntax();
    // Plain themes skip the markdown and code highlighters; skipping this too is
    // what keeps them monochrome throughout. Pills still apply — theirs is the
    // reversed look those themes already use for a chip in the reader.
    let colours = syntax.any_color();
    let keyword = Style::new().fg(syntax.keyword).add_modifier(Modifier::BOLD);
    let operator = Style::new().fg(syntax.operator);
    let punctuation = Style::new().fg(syntax.punctuation);

    let mut styling = QueryStyling::default();
    for segment in offsets::scan(query) {
        if colours {
            if let Some((_, range)) = &segment.prefix {
                styling.spans.push((range.start, range.end, keyword));
            }
            // Every separator in the grammar is a one-byte ASCII character.
            for &(_, at) in &segment.operators {
                styling.spans.push((at, at + 1, operator));
            }
            if let Some(at) = segment.separator {
                styling.spans.push((at, at + 1, punctuation));
            }
        }

        let Some(category) = segment.prefix.and_then(|(prefix, _)| pill_category(prefix)) else {
            continue;
        };
        for alternative in segment.alternatives.iter().filter(|alt| alt.quoted) {
            let (open, close, style) = match theme.pill_style() {
                // `theme.pill` is `Style::default()` here, so the literal brackets
                // carry the affordance — taking the style alone would draw an
                // invisible chip on any bracket-styled theme.
                PillStyle::Bracket => ('[', ']', None),
                _ => (' ', ' ', Some(theme.pill(category))),
            };
            let range = &alternative.range;
            // A quote pair is two one-byte delimiters, and only the outer pair is
            // one: a doubled quote inside the value renders verbatim, because
            // collapsing it would cost a column the caret is counting on.
            styling.substitutions.push((range.start, open));
            styling.substitutions.push((range.end - 1, close));
            if let Some(style) = style {
                styling.spans.push((range.start, range.end, style));
            }
        }
    }
    styling
}

/// The pill category a prefix's values belong to, for the four facets that have
/// one. Every value a filter row or a chip launches is quoted, `location:`
/// included, so a quoted span alone does not mean exact — only these four carry
/// that meaning, and a pill on the others would claim it falsely.
fn pill_category(prefix: Prefix) -> Option<PillCategory> {
    match prefix {
        Prefix::Tags => Some(PillCategory::Tags),
        Prefix::People => Some(PillCategory::People),
        Prefix::Activities => Some(PillCategory::Activities),
        Prefix::Feelings => Some(PillCategory::Feelings),
        Prefix::Star | Prefix::Location | Prefix::Mood | Prefix::Date(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::{self, Theme};

    /// The styled slices of `query`, in text order.
    fn styled(theme: &Theme, query: &str) -> Vec<String> {
        let mut spans = query_styling(theme, query).spans;
        spans.sort_by_key(|&(start, ..)| start);
        spans
            .into_iter()
            .map(|(start, end, _)| query[start..end].to_string())
            .collect()
    }

    /// `query` as it draws: every substituted delimiter swapped for its glyph.
    fn drawn(theme: &Theme, query: &str) -> String {
        let substitutions = query_styling(theme, query).substitutions;
        query
            .char_indices()
            .map(|(at, character)| {
                substitutions
                    .iter()
                    .find_map(|&(offset, glyph)| (offset == at).then_some(glyph))
                    .unwrap_or(character)
            })
            .collect()
    }

    fn coloured() -> Theme {
        theme::test_theme_from_toml(
            "[markdown.syntax]\nkeyword = \"#ff0000\"\noperator = \"#00ff00\"\npunctuation = \"#0000ff\"",
        )
    }

    fn bracket() -> Theme {
        theme::test_theme_from_toml("[metadata.pills]\nstyle = \"bracket\"")
    }

    #[test]
    fn a_recognised_prefix_is_styled_and_a_mistyped_one_is_not() {
        let theme = coloured();
        assert_eq!(styled(&theme, "tags:work"), vec!["tags:"]);
        // The whole point: one `s` short, and nothing lights up.
        assert!(styled(&theme, "tag:work").is_empty());
        assert!(styled(&theme, "Tags:work").is_empty());
    }

    #[test]
    fn operators_and_separators_are_styled_under_a_prefix() {
        let theme = coloured();
        assert_eq!(
            styled(&theme, "tags:a+b|c; people:d"),
            vec!["tags:", "+", "|", ";", "people:"]
        );
        // In full text the same characters are just characters.
        assert!(styled(&theme, "a+b|c").is_empty());
    }

    #[test]
    fn a_quoted_value_hides_its_operators_from_the_styling() {
        // The `+` is inside the value, so it is a character rather than an
        // operator — the whole quoted run is one pill, and nothing splits it.
        assert_eq!(
            styled(&coloured(), "tags:\"a+b\""),
            vec!["tags:", "\"a+b\""]
        );
        // A bracket theme carries the pill in its glyphs and has no syntax
        // palette here, so it styles nothing at all.
        assert!(styled(&bracket(), "tags:\"a+b\"").is_empty());
    }

    #[test]
    fn a_theme_without_syntax_colours_styles_nothing() {
        let plain = Theme::terminal_default();
        assert!(query_styling(&plain, "tags:a+b; x").spans.is_empty());
        // Pills are not syntax colour, so they survive on a plain theme — as
        // reversed text, exactly as the reader draws a chip there.
        assert!(!query_styling(&plain, "tags:\"a\"").spans.is_empty());
    }

    #[test]
    fn a_balanced_quoted_facet_value_draws_as_a_pill() {
        let theme = bracket();
        assert_eq!(drawn(&theme, "tags:\"apple\""), "tags:[apple]");
        assert_eq!(
            drawn(&theme, "tags:\"apple\"+\"banana\""),
            "tags:[apple]+[banana]"
        );
        // Under the default look the delimiters go blank and the run inverts.
        assert_eq!(drawn(&Theme::terminal_default(), "tags:\"a\""), "tags: a ");
    }

    /// Both directions of the transition, with no state anywhere but the text.
    #[test]
    fn deleting_either_delimiter_brings_the_quote_back() {
        let theme = bracket();
        assert_eq!(drawn(&theme, "tags:\"apple"), "tags:\"apple");
        assert_eq!(drawn(&theme, "tags:apple\""), "tags:apple\"");
        assert_eq!(drawn(&theme, "tags:\"apple\""), "tags:[apple]");
    }

    /// The pill's reach is the parser's reach. Deleting apple's closing quote
    /// leaves one alternative whose needle is `apple+"banana`, so one pill
    /// swallows the `+` — alarming to look at, and exactly right.
    #[test]
    fn an_unbalanced_quote_draws_one_pill_over_the_operator() {
        assert_eq!(
            drawn(&bracket(), "tags:\"apple+\"banana\""),
            "tags:[apple+\"banana]"
        );
    }

    #[test]
    fn only_the_four_facet_prefixes_get_a_pill() {
        let theme = bracket();
        for query in [
            "people:\"alice\"",
            "activities:\"run\"",
            "feelings:\"calm\"",
        ] {
            assert!(drawn(&theme, query).contains('['), "{query}");
        }
        // Every launched value is quoted, so a quoted one of these is not a
        // claim of exactness and must not look like one.
        for query in [
            "location:\"berlin\"",
            "mood:\"3\"",
            "star:\"true\"",
            "date:\"2026\"",
        ] {
            assert_eq!(drawn(&theme, query), query);
        }
        // Neither is a quoted run of plain text.
        assert_eq!(drawn(&theme, "\"apple\""), "\"apple\"");
    }

    #[test]
    fn a_doubled_quote_inside_a_value_renders_verbatim() {
        // Only the outer pair is a delimiter. Reclaiming the inner column would
        // be the width remapping this whole design avoids.
        assert_eq!(
            drawn(&bracket(), "tags:\"say \"\"hi\"\"\""),
            "tags:[say \"\"hi\"\"]"
        );
    }

    /// A chip is decoration, so nothing guards it. These assert the *absence* of
    /// a guard, and stand as the fence against reintroducing one piecemeal.
    #[test]
    fn stock_editing_keys_still_reach_a_chip() {
        use crate::tui::text_input::TextInput;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let theme = bracket();
        let mut input = TextInput::from("tags:\"apple\"".to_string());
        assert_eq!(query_styling(&theme, input.as_str()).substitutions.len(), 2);

        // A word delete at the chip's right edge takes the closing quote alone,
        // and the pill dissolves rather than the key being swallowed.
        input.input(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(input.as_str(), "tags:\"apple");
        assert!(
            query_styling(&theme, input.as_str())
                .substitutions
                .is_empty()
        );

        // Double-click inside the value grabs it without its delimiters, so the
        // natural "replace this chip" gesture keeps the chip.
        let mut input = TextInput::from("tags:\"apple\"".to_string());
        input.select_word_at(8);
        let (start, end) = input.selection_range().expect("a word is selected");
        assert_eq!((start.1, end.1), (6, 11));
    }

    /// Launching a filter row builds `tags:"value"` through `quote_filter_value`,
    /// and that has to chip with no special-casing of where the text came from.
    #[test]
    fn a_launched_filter_value_round_trips_to_a_chip() {
        use crate::tui::features::search::quote_filter_value;
        use crate::tui::state::FilterTab;

        let theme = bracket();
        for tab in FilterTab::ALL {
            let query = format!("{}:{}", tab.search_prefix(), quote_filter_value("apple"));
            let chipped = !query_styling(&theme, &query).substitutions.is_empty();
            assert_eq!(chipped, tab != FilterTab::Locations, "{query}");
        }
    }

    #[test]
    fn a_multibyte_value_substitutes_only_its_delimiters() {
        let theme = bracket();
        assert_eq!(drawn(&theme, "tags:\"Ärger\""), "tags:[Ärger]");
        for (offset, _) in query_styling(&theme, "tags:\"Ärger\"").substitutions {
            assert_eq!(&"tags:\"Ärger\""[offset..offset + 1], "\"");
        }
    }
}
