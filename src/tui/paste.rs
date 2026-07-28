//! Normalizing text pasted in from outside the app.
//!
//! A bracketed-paste block and a system-clipboard read both carry whatever line
//! endings and control bytes their source used. iTerm2 and Terminal.app send `\r`
//! for every newline in a pasted block, and `TextArea::insert_str` splits on `\n`
//! only, so an unnormalized paste collapses onto one line with literal carriage
//! returns baked into the saved markdown. Stray control bytes are a separate
//! hazard: the renderer writes line content to the terminal as-is, so a pasted
//! escape sequence would be executed rather than shown.

use std::borrow::Cow;

/// Rewrite a pasted block into text safe to insert: `\r\n` and a lone `\r` become
/// a single `\n`, newlines and tabs survive, and every other control char is
/// dropped. Borrows when there is nothing to rewrite, and is idempotent, so
/// applying it at more than one layer costs nothing.
pub(crate) fn normalize(text: &str) -> Cow<'_, str> {
    if !text.chars().any(is_rewritten) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                chars.next_if_eq(&'\n');
                out.push('\n');
            }
            '\n' | '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// Whether [`normalize`] touches `c`: every control char except the two the
/// editor renders itself.
fn is_rewritten(c: char) -> bool {
    c.is_control() && !matches!(c, '\n' | '\t')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carriage_returns_become_single_newlines() {
        // A terminal that converts newlines on paste sends `\r`; one that doesn't
        // may still send `\r\n`. Neither may yield a blank line between entries.
        assert_eq!(normalize("a\rb\r\nc\n"), "a\nb\nc\n");
    }

    #[test]
    fn tabs_and_newlines_survive_while_other_controls_are_dropped() {
        assert_eq!(normalize("a\tb\n\x1b[31mc\x07"), "a\tb\n[31mc");
    }

    #[test]
    fn clean_text_is_borrowed() {
        assert!(matches!(normalize("plain\ttext\n"), Cow::Borrowed(_)));
    }

    #[test]
    fn a_control_only_block_normalizes_to_empty() {
        assert!(normalize("\x1b\x07\u{7f}").is_empty());
    }

    #[test]
    fn normalizing_twice_changes_nothing() {
        let once = normalize("a\r\nb\x1bc\td").into_owned();
        assert_eq!(normalize(&once), once);
    }
}
