use std::ops::Range;

use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_linebreak::{BreakClass, BreakOpportunity, break_property, linebreaks};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::RichSpan;

pub(super) struct WrappedLine {
    pub(super) spans: Vec<Span<'static>>,
    pub(super) links: Vec<(usize, usize, usize)>,
}

struct StyledGrapheme<'a> {
    text: &'a str,
    end: usize,
    width: usize,
    style: Style,
    link: Option<usize>,
}

/// Wrap styled text without separating a grapheme, including across input spans.
/// Prose uses Unicode line breaks; code and rules use grapheme boundaries.
pub(super) fn wrap_rich(spans: Vec<RichSpan>, width: usize, hard_wrap: bool) -> Vec<WrappedLine> {
    let text: String = spans.iter().map(|span| span.content.as_str()).collect();
    let mut runs = spans.iter();
    let mut run = runs.next();
    let mut run_end = run.map_or(0, |span| span.content.len());
    let graphemes: Vec<_> = text
        .grapheme_indices(true)
        .map(|(start, text)| {
            while start >= run_end {
                run = runs.next();
                run_end += run.map_or(0, |span| span.content.len());
            }
            let run = run.expect("each grapheme belongs to a source span");
            StyledGrapheme {
                text,
                end: start + text.len(),
                width: text.width(),
                style: run.style,
                link: run.link,
            }
        })
        .collect();
    let width = width.max(1);
    let mut rows = Vec::new();
    if hard_wrap {
        split_graphemes(&graphemes, 0..graphemes.len(), width, &mut rows);
    } else {
        rows = prose_rows(&text, &graphemes, width);
    }
    if rows.is_empty() {
        rows.push(0..0);
    }
    rows.into_iter()
        .map(|mut row| {
            if !hard_wrap {
                while row.end > row.start && is_break_space(graphemes[row.end - 1].text) {
                    row.end -= 1;
                }
            }
            finalize_line(&graphemes[row])
        })
        .collect()
}

/// Wrap a table cell while retaining its line-level style and alignment.
pub(super) fn wrap_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    let style = line.style;
    let alignment = line.alignment;
    wrap_rich(rich_from_line(line), width, false)
        .into_iter()
        .map(|wrapped| {
            let mut line = Line::from(wrapped.spans);
            line.style = style;
            line.alignment = alignment;
            line
        })
        .collect()
}

fn is_break_space(text: &str) -> bool {
    text.chars().all(|c| {
        c == '\t'
            || matches!(
                break_property(c as u32),
                BreakClass::Space
                    | BreakClass::Mandatory
                    | BreakClass::CarriageReturn
                    | BreakClass::LineFeed
                    | BreakClass::NextLine
            )
    })
}

fn cells(graphemes: &[StyledGrapheme<'_>]) -> usize {
    graphemes.iter().map(|g| g.width).sum()
}

fn prose_rows(text: &str, graphemes: &[StyledGrapheme<'_>], width: usize) -> Vec<Range<usize>> {
    let mut rows = Vec::new();
    let mut cursor = 0;
    let mut chunk_start = 0;
    let mut row_start = 0;
    let mut row_width = 0;
    for (byte_end, opportunity) in linebreaks(text) {
        while cursor < graphemes.len() && graphemes[cursor].end < byte_end {
            cursor += 1;
        }
        if cursor == graphemes.len() || graphemes[cursor].end != byte_end {
            continue;
        }
        let end = cursor + 1;
        let mut content_end = end;
        while content_end > chunk_start && is_break_space(graphemes[content_end - 1].text) {
            content_end -= 1;
        }
        let content_width = cells(&graphemes[chunk_start..content_end]);
        if row_width + content_width > width && chunk_start > row_start {
            rows.push(row_start..chunk_start);
            row_start = chunk_start;
            row_width = 0;
        }
        if content_width > width {
            split_word(graphemes, chunk_start..content_end, width, &mut rows);
            let last = rows.pop().expect("an oversized segment produces a row");
            row_start = last.start;
            row_width = cells(&graphemes[row_start..end]);
        } else {
            row_width += cells(&graphemes[chunk_start..end]);
        }
        chunk_start = end;
        if opportunity == BreakOpportunity::Mandatory {
            rows.push(row_start..end);
            row_start = end;
            row_width = 0;
        }
    }
    if row_start < chunk_start {
        rows.push(row_start..chunk_start);
    }
    rows
}

fn split_word(
    graphemes: &[StyledGrapheme<'_>],
    range: Range<usize>,
    width: usize,
    rows: &mut Vec<Range<usize>>,
) {
    let mut tail = range.end;
    for index in range.clone().rev() {
        let punctuation = graphemes[index].text.chars().next().is_some_and(|c| {
            matches!(
                break_property(c as u32),
                BreakClass::ClosePunctuation
                    | BreakClass::CloseParenthesis
                    | BreakClass::Exclamation
                    | BreakClass::InfixSeparator
                    | BreakClass::Inseparable
                    | BreakClass::NonStarter
                    | BreakClass::Quotation
                    | BreakClass::Postfix
                    | BreakClass::Symbol
            )
        });
        if punctuation {
            tail = index;
        } else {
            if tail < range.end && !graphemes[index].text.chars().all(char::is_whitespace) {
                tail = index;
            }
            break;
        }
    }
    if tail == range.end || cells(&graphemes[tail..range.end]) > width {
        split_graphemes(graphemes, range, width, rows);
        return;
    }
    let first = rows.len();
    split_graphemes(graphemes, range.start..tail, width, rows);
    if rows.len() > first {
        let last = rows.last_mut().expect("the prefix produced a row");
        if cells(&graphemes[last.start..range.end]) <= width {
            last.end = range.end;
            return;
        }
    }
    rows.push(tail..range.end);
}

fn split_graphemes(
    graphemes: &[StyledGrapheme<'_>],
    range: Range<usize>,
    width: usize,
    rows: &mut Vec<Range<usize>>,
) {
    let mut start = range.start;
    let mut used = 0;
    for index in range.clone() {
        if index > start && used + graphemes[index].width > width {
            rows.push(start..index);
            start = index;
            used = 0;
        }
        used += graphemes[index].width;
    }
    if start < range.end {
        rows.push(start..range.end);
    }
}

fn finalize_line(graphemes: &[StyledGrapheme<'_>]) -> WrappedLine {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut links = Vec::new();
    let mut column = 0;
    let mut run = None;
    let mut previous_link = None;
    for grapheme in graphemes {
        if let Some(span) = spans.last_mut()
            && span.style == grapheme.style
            && previous_link == grapheme.link
        {
            span.content.to_mut().push_str(grapheme.text);
        } else {
            spans.push(Span::styled(grapheme.text.to_owned(), grapheme.style));
        }
        previous_link = grapheme.link;
        match (run, grapheme.link) {
            (Some((id, _)), Some(current)) if id == current => {}
            (Some((id, start)), _) => {
                if start < column {
                    links.push((start, column, id));
                }
                run = grapheme.link.map(|current| (current, column));
            }
            (None, Some(current)) => run = Some((current, column)),
            (None, None) => {}
        }
        column += grapheme.width;
    }
    if let Some((id, start)) = run
        && start < column
    {
        links.push((start, column, id));
    }
    WrappedLine { spans, links }
}

/// Convert a plain `Line` (code, rules, stacked-table rows — none carry link
/// semantics) into the renderer's tagged runs for emission.
pub(super) fn rich_from_line(line: Line<'static>) -> Vec<RichSpan> {
    line.spans
        .into_iter()
        .map(|span| RichSpan {
            content: span.content.into_owned(),
            style: span.style,
            link: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{buffer::Buffer, layout::Rect, style::Modifier, widgets::Widget};

    fn visible(text: &str, width: usize) -> Vec<String> {
        wrap_rich(
            vec![RichSpan {
                content: text.to_owned(),
                style: Style::default(),
                link: None,
            }],
            width,
            false,
        )
        .into_iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect()
    }

    #[test]
    fn unicode_breaks_and_punctuation_match_editor_policy() {
        for (text, width, expected) in [
            ("a (word) end", 7, vec!["a", "(word)", "end"]),
            ("你好，世界。", 6, vec!["你好，", "世界。"]),
            ("你好，世界。", 4, vec!["你", "好，", "世", "界。"]),
            ("hello,", 5, vec!["hell", "o,"]),
            ("abcdef?!", 3, vec!["abc", "de", "f?!"]),
            ("abcdef?! next", 6, vec!["abcde", "f?!", "next"]),
            ("abcde\u{301}!", 5, vec!["abcd", "e\u{301}!"]),
            ("hello,", 1, vec!["h", "e", "l", "l", "o", ","]),
            ("abc?!", 2, vec!["ab", "c?", "!"]),
            ("hello   world", 5, vec!["hello", "world"]),
            ("", 0, vec![""]),
            ("    ", 0, vec![""]),
        ] {
            assert_eq!(visible(text, width), expected, "{text:?} at {width}");
        }
        for punctuation in [
            ",", ".", "!", "?", ":", ";", "…", "?!", "...", ")", "]", "}", "\"", "”", "’", "。」",
        ] {
            let word = format!("word{punctuation}");
            assert_eq!(
                visible(&format!("a {word} next"), word.width().max(6)),
                ["a".to_owned(), word, "next".to_owned()]
            );
        }
    }

    #[test]
    fn nonbreaking_spaces_and_explicit_breaks_survive() {
        for joined in ["ab\u{a0}cd", "ab\u{202f}cd", "ab\u{2060}cd"] {
            assert_eq!(visible(&format!("x {joined}"), 5), ["x", joined]);
        }
        assert_eq!(visible("ab\u{200b}cd", 2), ["ab\u{200b}", "cd"]);
        for separator in ["\n", "\r\n", "\u{85}", "\u{2028}", "\u{2029}"] {
            assert_eq!(
                visible(&format!("one{separator}two{separator}"), 80),
                ["one", "two"]
            );
        }
    }

    #[test]
    fn style_boundaries_cannot_split_a_grapheme_or_its_link() {
        let style = Style::new().add_modifier(Modifier::BOLD);
        let rows = wrap_rich(
            vec![
                RichSpan {
                    content: "a👩".into(),
                    style,
                    link: Some(0),
                },
                RichSpan {
                    content: "🏽‍💻".into(),
                    style: Style::default(),
                    link: Some(1),
                },
                RichSpan {
                    content: "b".into(),
                    style: Style::default(),
                    link: Some(1),
                },
            ],
            3,
            false,
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].links, [(0, 3, 0)]);
        assert_eq!(rows[1].links, [(0, 1, 1)]);
        let area = Rect::new(0, 0, 3, 1);
        let mut buffer = Buffer::empty(area);
        Line::from(rows[0].spans.clone()).render(area, &mut buffer);
        assert_eq!(buffer[(1, 0)].symbol(), "👩🏽‍💻");
        assert_eq!(buffer[(1, 0)].style().add_modifier, Modifier::BOLD);
    }

    #[test]
    fn same_style_different_links_keep_separate_spans() {
        let rows = wrap_rich(
            vec![
                RichSpan {
                    content: "👩🏽‍💻".into(),
                    style: Style::default(),
                    link: Some(0),
                },
                RichSpan {
                    content: "🇩🇪".into(),
                    style: Style::default(),
                    link: Some(1),
                },
            ],
            4,
            false,
        );
        assert_eq!(rows[0].links, [(0, 2, 0), (2, 4, 1)]);
        assert_eq!(rows[0].spans.len(), 2);
    }

    #[test]
    fn hard_wrapping_preserves_text_and_all_wraps_preserve_clusters() {
        for text in [
            "a e\u{301}! 👩🏽‍💻 🇩🇪 end",
            "❤\u{fe0f} 1️⃣!",
            "你好，世界。",
            "   x  ",
        ] {
            let boundaries: Vec<_> = text
                .grapheme_indices(true)
                .map(|(i, _)| i)
                .chain([text.len()])
                .collect();
            for width in 0..=20 {
                for hard in [false, true] {
                    let rows = wrap_rich(
                        vec![RichSpan {
                            content: text.into(),
                            style: Style::default(),
                            link: None,
                        }],
                        width,
                        hard,
                    );
                    let mut offset = 0;
                    for row in rows {
                        let rendered: String =
                            row.spans.iter().map(|s| s.content.as_ref()).collect();
                        if !hard {
                            // Soft wrapping may consume spaces after the preceding row.
                            while !text[offset..].starts_with(&rendered)
                                && text[offset..].starts_with(' ')
                            {
                                offset += 1;
                            }
                        }
                        assert!(text[offset..].starts_with(&rendered));
                        assert!(boundaries.contains(&offset));
                        offset += rendered.len();
                        assert!(boundaries.contains(&offset));
                        assert!(
                            rendered.width() <= width.max(1)
                                || rendered.graphemes(true).count() == 1
                        );
                    }
                    if hard {
                        assert_eq!(offset, text.len());
                    }
                }
            }
        }
    }

    #[test]
    fn table_cell_retains_alignment_and_grapheme_width() {
        let style = Style::new().bold();
        let rows = wrap_line(Line::from("👩🏽‍💻🇩🇪").style(style).right_aligned(), 2);
        assert_eq!(rows.len(), 2);
        for row in rows {
            assert_eq!(row.width(), 2);
            assert_eq!(row.style, style);
            assert_eq!(row.alignment, Some(ratatui::layout::Alignment::Right));
        }
    }
}
