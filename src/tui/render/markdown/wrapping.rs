use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_width::UnicodeWidthChar;

use super::RichSpan;

pub(super) struct WrappedLine {
    pub(super) spans: Vec<Span<'static>>,
    pub(super) links: Vec<(usize, usize, usize)>,
}

#[derive(Clone, Copy)]
struct StyledCharacter {
    character: char,
    width: usize,
    style: Style,
    link: Option<usize>,
}

/// Wrap link-tagged runs to `width`, returning display lines that carry both the
/// collapsed spans and the link regions the wrap produced. `hard_wrap` breaks
/// anywhere (rules, code); otherwise it breaks at whitespace, splitting a lone
/// token only when it alone exceeds the width.
pub(super) fn wrap_rich(spans: Vec<RichSpan>, width: usize, hard_wrap: bool) -> Vec<WrappedLine> {
    let mut characters = Vec::new();
    for span in spans {
        for character in span.content.chars() {
            characters.push(StyledCharacter {
                character,
                width: character.width().unwrap_or(0),
                style: span.style,
                link: span.link,
            });
        }
    }
    let char_lines = if hard_wrap {
        hard_wrap_chars(characters, width)
    } else {
        soft_wrap_chars(characters, width)
    };
    char_lines.iter().map(|line| finalize_line(line)).collect()
}

/// Word-wrap the table path, which has no link semantics; the surviving caller of
/// the plain-`Line` shape. Preserves the line-level style/alignment tables set.
pub(super) fn wrap_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    let style = line.style;
    let alignment = line.alignment;
    let mut characters = Vec::new();
    for span in line.spans {
        for character in span.content.chars() {
            characters.push(StyledCharacter {
                character,
                width: character.width().unwrap_or(0),
                style: span.style,
                link: None,
            });
        }
    }
    soft_wrap_chars(characters, width)
        .iter()
        .map(|chars| {
            let mut line = Line::from(finalize_line(chars).spans);
            line.style = style;
            line.alignment = alignment;
            line
        })
        .collect()
}

fn soft_wrap_chars(characters: Vec<StyledCharacter>, width: usize) -> Vec<Vec<StyledCharacter>> {
    let mut wrapped: Vec<Vec<StyledCharacter>> = vec![Vec::new()];
    let mut used = 0usize;
    let mut pending_whitespace: Option<&[StyledCharacter]> = None;
    let mut cursor = 0;

    while cursor < characters.len() {
        let whitespace = characters[cursor].character.is_whitespace();
        let end = characters[cursor..]
            .iter()
            .position(|character| character.character.is_whitespace() != whitespace)
            .map_or(characters.len(), |offset| cursor + offset);
        let token = &characters[cursor..end];

        if whitespace {
            pending_whitespace = Some(token);
            cursor = end;
            continue;
        }

        let whitespace_width = pending_whitespace.map_or(0, token_width);
        let word_width = token_width(token);
        if used > 0
            && used
                .saturating_add(whitespace_width)
                .saturating_add(word_width)
                > width
        {
            wrapped.push(Vec::new());
            used = 0;
        } else if let Some(whitespace) = pending_whitespace {
            wrapped
                .last_mut()
                .expect("wrapped output always has a line")
                .extend_from_slice(whitespace);
            used = used.saturating_add(whitespace_width);
        }
        pending_whitespace = None;

        for character in token {
            if used > 0 && used.saturating_add(character.width) > width {
                wrapped.push(Vec::new());
                used = 0;
            }
            wrapped
                .last_mut()
                .expect("wrapped output always has a line")
                .push(*character);
            used = used.saturating_add(character.width);
        }
        cursor = end;
    }

    wrapped
}

fn hard_wrap_chars(characters: Vec<StyledCharacter>, width: usize) -> Vec<Vec<StyledCharacter>> {
    let mut wrapped: Vec<Vec<StyledCharacter>> = vec![Vec::new()];
    let mut used = 0usize;
    for character in characters {
        if used > 0 && used.saturating_add(character.width) > width {
            wrapped.push(Vec::new());
            used = 0;
        }
        wrapped
            .last_mut()
            .expect("wrapped output always has a line")
            .push(character);
        used = used.saturating_add(character.width);
    }
    wrapped
}

/// Collapse a wrapped display line's characters into style-merged spans while
/// recording each contiguous same-id link run's column span.
fn finalize_line(characters: &[StyledCharacter]) -> WrappedLine {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut links = Vec::new();
    let mut column = 0usize;
    let mut run: Option<(usize, usize)> = None;
    for character in characters {
        if let Some(span) = spans.last_mut()
            && span.style == character.style
        {
            span.content.to_mut().push(character.character);
        } else {
            spans.push(Span::styled(
                character.character.to_string(),
                character.style,
            ));
        }
        match (run, character.link) {
            (Some((id, _)), Some(current)) if id == current => {}
            (Some((id, start)), _) => {
                links.push((start, column, id));
                run = character.link.map(|current| (current, column));
            }
            (None, Some(current)) => run = Some((current, column)),
            (None, None) => {}
        }
        column = column.saturating_add(character.width);
    }
    if let Some((id, start)) = run {
        links.push((start, column, id));
    }
    WrappedLine { spans, links }
}

fn token_width(token: &[StyledCharacter]) -> usize {
    token.iter().fold(0usize, |width, character| {
        width.saturating_add(character.width)
    })
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
