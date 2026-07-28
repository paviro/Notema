//! Styling for the search field's own text.
//!
//! The query box holds a small language, and until now it drew as flat text: a
//! mistyped `tag:` looked exactly like a working `tags:` and only the results
//! said otherwise. These spans colour what the parser *recognised*, so the two
//! cases are distinguishable while typing.
//!
//! Output is `(start_byte, end_byte, Style)` for `TextArea::set_syntax_spans`,
//! same contract as `editor_highlight` uses for the entry body.

use ratatui::style::{Modifier, Style};

use crate::tui::features::search::offsets;
use crate::tui::theme::Theme;

/// Style the query's grammar: the prefix that parsed, the `+`/`|` between its
/// alternatives, and the `;` between filters. Everything else is full text and
/// stays unstyled, because that is what it is.
pub(super) fn query_syntax_spans(theme: &Theme, query: &str) -> Vec<(usize, usize, Style)> {
    let syntax = theme.syntax();
    // Plain themes skip the markdown and code highlighters; skipping this too is
    // what keeps them monochrome throughout.
    if !syntax.any_color() {
        return Vec::new();
    }
    let keyword = Style::new().fg(syntax.keyword).add_modifier(Modifier::BOLD);
    let operator = Style::new().fg(syntax.operator);
    let punctuation = Style::new().fg(syntax.punctuation);

    let mut spans = Vec::new();
    for segment in offsets::scan(query) {
        if let Some((_, range)) = segment.prefix {
            spans.push((range.start, range.end, keyword));
        }
        // Every separator in the grammar is a one-byte ASCII character.
        for (_, at) in segment.operators {
            spans.push((at, at + 1, operator));
        }
        if let Some(at) = segment.separator {
            spans.push((at, at + 1, punctuation));
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::{self, Theme};

    /// The styled slices of `query`, in text order.
    fn styled(theme: &Theme, query: &str) -> Vec<String> {
        let mut spans = query_syntax_spans(theme, query);
        spans.sort_by_key(|&(start, ..)| start);
        spans
            .into_iter()
            .map(|(start, end, _)| query[start..end].to_string())
            .collect()
    }

    fn coloured() -> Theme {
        theme::test_theme_from_toml(
            "[markdown.syntax]\nkeyword = \"#ff0000\"\noperator = \"#00ff00\"\npunctuation = \"#0000ff\"",
        )
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
        let theme = coloured();
        assert_eq!(styled(&theme, "tags:\"a+b\""), vec!["tags:"]);
    }

    #[test]
    fn a_theme_without_syntax_colours_styles_nothing() {
        assert!(query_syntax_spans(&Theme::terminal_default(), "tags:a+b; x").is_empty());
    }
}
