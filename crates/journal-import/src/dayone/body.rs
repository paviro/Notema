//! Pure text transforms for Day One entry bodies.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Undo Day One's Markdown escaping: it prefixes literal punctuation with a
/// backslash (e.g. `verdiene\.\.\.`, `\!`, `gaps\.pdf`). We drop the backslash
/// before any ASCII punctuation so the body reads as normal Markdown.
///
/// Heuristic: this also unescapes punctuation inside fenced code blocks, which
/// Day One escapes too. Acceptable given Day One over-escapes.
pub fn unescape_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(&next) = chars.peek()
            && next.is_ascii_punctuation()
        {
            out.push(next);
            chars.next();
            continue;
        }
        out.push(c);
    }
    out
}

/// Convert HTML `<img src="…">` tags (used by older Day One entries, often for
/// remotely-hosted images) into Markdown `![](…)` references so the moment and
/// asset pipeline handles them uniformly. Tags without a `src` are dropped.
///
/// Fenced code blocks (``` / ~~~) are passed through untouched, so `<img>` shown
/// as example markup in a code block is preserved verbatim.
pub fn html_images_to_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_fence = false;
    let mut lines = text.split('\n').peekable();
    while let Some(line) = lines.next() {
        if is_fence(line) {
            in_fence = !in_fence;
            out.push_str(line);
        } else if in_fence {
            out.push_str(line);
        } else {
            out.push_str(&convert_html_images_in_line(line));
        }
        if lines.peek().is_some() {
            out.push('\n');
        }
    }
    out
}

fn is_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

fn convert_html_images_in_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(pos) = find_ci(rest, "<img") {
        out.push_str(&rest[..pos]);
        let tag_rest = &rest[pos..];
        let Some(gt) = tag_rest.find('>') else {
            // Unterminated tag on this line: emit the remainder untouched.
            out.push_str(tag_rest);
            return out;
        };
        let tag = &tag_rest[..=gt];
        if let Some(src) = extract_attr(tag, "src") {
            out.push_str(&format!("![]({src})"));
        }
        rest = &tag_rest[gt + 1..];
    }
    out.push_str(rest);
    out
}

/// Case-insensitive byte search. `needle` must be ASCII (it is, here), so the
/// returned index is a valid char boundary.
fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

/// Extract a (single- or double-quoted) attribute value from an HTML tag.
fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    let key = format!("{attr}=");
    let start = find_ci(tag, &key)? + key.len();
    let after = &tag[start..];
    let quote = after.chars().next()?;
    if quote == '"' || quote == '\'' {
        let after_quote = &after[quote.len_utf8()..];
        let end = after_quote.find(quote)?;
        Some(after_quote[..end].trim().to_string()).filter(|s| !s.is_empty())
    } else {
        None
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct MomentRewrite {
    pub body: String,
    pub skipped_audio: usize,
    pub skipped_video: usize,
    pub skipped_pdf: usize,
    /// Moment identifiers referenced by the body but not found in any of the
    /// entry's media arrays (or missing an md5/type). Left unresolved and
    /// dropped from the body.
    pub unresolved: Vec<String>,
}

impl MomentRewrite {
    pub fn skipped_attachments(&self) -> usize {
        self.skipped_audio + self.skipped_video + self.skipped_pdf
    }
}

/// Rewrite `dayone-moment://` references in a Markdown body.
///
/// Photo moments are rewritten to a local `![alt](<absolute path>)` link so the
/// store's asset ingestion copies them in. Audio/video/pdf moments have no
/// destination yet, so their references are removed and counted. Non-moment
/// images (and any other Markdown) pass through untouched.
///
/// Classification is by identifier membership in the entry's media arrays, not
/// by the moment URL shape, which Day One has spelled inconsistently over time.
pub fn rewrite_moments(
    text: &str,
    photo_paths: &HashMap<String, PathBuf>,
    audio: &HashSet<String>,
    video: &HashSet<String>,
    pdf: &HashSet<String>,
) -> MomentRewrite {
    let mut result = MomentRewrite::default();
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(rel_start) = rest.find("![") {
        let (before, tag_start) = rest.split_at(rel_start);
        match parse_image(tag_start) {
            Some(image) if is_moment(image.target) => {
                out.push_str(before);
                let id = moment_identifier(image.target);
                if let Some(path) = photo_paths.get(id) {
                    out.push_str(&format!("![{}]({})", image.alt, path.display()));
                } else if audio.contains(id) {
                    result.skipped_audio += 1;
                } else if video.contains(id) {
                    result.skipped_video += 1;
                } else if pdf.contains(id) {
                    result.skipped_pdf += 1;
                } else {
                    result.unresolved.push(id.to_string());
                }
                rest = &tag_start[image.len..];
            }
            // Not a moment we handle (regular image or malformed): emit the
            // `![` and keep scanning past it.
            _ => {
                out.push_str(before);
                out.push_str("![");
                rest = &tag_start[2..];
            }
        }
    }
    out.push_str(rest);

    result.body = out;
    result
}

struct ParsedImage<'a> {
    alt: &'a str,
    target: &'a str,
    /// Byte length of the whole `![alt](target)` starting at the `!`.
    len: usize,
}

/// Parse a `![alt](target)` starting at `s[0] == '!'`. Returns `None` if `s`
/// does not begin a well-formed image tag.
fn parse_image(s: &str) -> Option<ParsedImage<'_>> {
    let after_bang = s.strip_prefix("![")?;
    let alt_end = after_bang.find("](")?;
    let alt = &after_bang[..alt_end];
    let after_alt = &after_bang[alt_end + 2..];
    let target_end = after_alt.find(')')?;
    let target = &after_alt[..target_end];
    // 2 (`![`) + alt + 2 (`](`) + target + 1 (`)`)
    let len = 2 + alt_end + 2 + target_end + 1;
    Some(ParsedImage { alt, target, len })
}

fn is_moment(target: &str) -> bool {
    target.trim().starts_with("dayone-moment:")
}

/// Extract the moment identifier from a `dayone-moment:` target, tolerating both
/// `dayone-moment://<id>` and `dayone-moment:/audio/<id>` shapes by taking the
/// last path segment.
fn moment_identifier(target: &str) -> &str {
    let rest = target
        .trim()
        .strip_prefix("dayone-moment:")
        .unwrap_or(target)
        .trim_start_matches('/');
    rest.rsplit('/').next().unwrap_or(rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescape_strips_backslash_before_punctuation() {
        assert_eq!(unescape_markdown(r"verdiene\.\.\."), "verdiene...");
        assert_eq!(unescape_markdown(r"Nice zwei Bilder\!"), "Nice zwei Bilder!");
        assert_eq!(unescape_markdown(r"gaps\.pdf"), "gaps.pdf");
        // Backslash before a non-punctuation char is preserved.
        assert_eq!(unescape_markdown(r"a\b"), r"a\b");
    }

    fn photo_map() -> HashMap<String, PathBuf> {
        let mut m = HashMap::new();
        m.insert("PHOTO1".to_string(), PathBuf::from("/exp/photos/aaa.jpeg"));
        m
    }

    #[test]
    fn rewrites_photo_moment_to_local_path() {
        let photos = photo_map();
        let empty = HashSet::new();
        let out = rewrite_moments(
            "![](dayone-moment://PHOTO1)\n\nHi",
            &photos,
            &empty,
            &empty,
            &empty,
        );
        assert_eq!(out.body, "![](/exp/photos/aaa.jpeg)\n\nHi");
        assert_eq!(out.skipped_attachments(), 0);
        assert!(out.unresolved.is_empty());
    }

    #[test]
    fn drops_and_counts_audio_moment() {
        let photos = HashMap::new();
        let mut audio = HashSet::new();
        audio.insert("AUD1".to_string());
        let empty = HashSet::new();
        let out = rewrite_moments(
            "Before ![](dayone-moment:/audio/AUD1) after",
            &photos,
            &audio,
            &empty,
            &empty,
        );
        assert_eq!(out.body, "Before  after");
        assert_eq!(out.skipped_audio, 1);
    }

    #[test]
    fn unknown_moment_is_recorded_unresolved() {
        let empty_map = HashMap::new();
        let empty = HashSet::new();
        let out = rewrite_moments(
            "![](dayone-moment://GONE)",
            &empty_map,
            &empty,
            &empty,
            &empty,
        );
        assert_eq!(out.unresolved, vec!["GONE".to_string()]);
        assert_eq!(out.body, "");
    }

    #[test]
    fn converts_html_img_tags_to_markdown() {
        // Self-closing remote image (as seen in older Day One exports).
        assert_eq!(
            html_images_to_markdown(r#"a <img src="http://h.local/1.jpeg"/> b"#),
            "a ![](http://h.local/1.jpeg) b"
        );
        // Single quotes, extra attributes, uppercase tag.
        assert_eq!(
            html_images_to_markdown(r#"<IMG width='9' src='x.png'>"#),
            "![](x.png)"
        );
        // An img src pointing at a moment stays a moment link for the next step.
        assert_eq!(
            html_images_to_markdown(r#"<img src="dayone-moment://ID">"#),
            "![](dayone-moment://ID)"
        );
        // No src → tag dropped; surrounding text preserved.
        assert_eq!(html_images_to_markdown("x <img alt='y'> z"), "x  z");
        // Text without tags is unchanged.
        assert_eq!(html_images_to_markdown("no tags here"), "no tags here");
    }

    #[test]
    fn does_not_convert_html_inside_code_blocks() {
        let body = "before\n```\n<img src=\"x.png\">\n```\n<img src=\"y.png\">";
        assert_eq!(
            html_images_to_markdown(body),
            "before\n```\n<img src=\"x.png\">\n```\n![](y.png)"
        );
    }

    #[test]
    fn leaves_regular_images_untouched() {
        let empty_map = HashMap::new();
        let empty = HashSet::new();
        let body = "See ![a cat](cat.png) here";
        let out = rewrite_moments(body, &empty_map, &empty, &empty, &empty);
        assert_eq!(out.body, body);
    }
}
