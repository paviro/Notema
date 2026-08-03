//! The read-only half of asset handling: recognizing a canonical stored
//! reference in an entry body, resolving one to a file on disk, and the small
//! markdown/path parsers both halves share. Nothing here writes.

use super::{IMAGE_EXTENSIONS, entry_assets_dir, entry_assets_dir_name};
use crate::AppResult;
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

/// A canonical asset reference (image or attachment) inside an entry's own asset
/// folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct StoredAssetReference {
    pub file_name: String,
}

/// Parse the exact stored form `<entry-id>.assets/<file>`. Rejects anything
/// absolute, nested, traversal-based, external, or pointing at a different
/// assets directory. Extension-agnostic — it matches images and attachments
/// alike.
pub(super) fn stored_asset_reference(target: &str, dir_name: &str) -> Option<StoredAssetReference> {
    if target.is_empty()
        || is_url(target)
        || target.starts_with('/')
        || target.starts_with('\\')
        || target.contains('\\')
    {
        return None;
    }

    let mut components = Path::new(target).components();
    let Some(Component::Normal(dir)) = components.next() else {
        return None;
    };
    if dir != dir_name {
        return None;
    }
    let Some(Component::Normal(file)) = components.next() else {
        return None;
    };
    if components.next().is_some() {
        return None;
    }
    let file_name = file.to_str()?;
    if file_name.is_empty() || file_name == "." || file_name == ".." {
        return None;
    }
    Some(StoredAssetReference {
        file_name: file_name.to_string(),
    })
}

/// The stored file name if `target` is a canonical reference inside
/// `entry_path`'s own asset folder, else `None`. Pure string check (no
/// filesystem access) so callers on the render hot path can use it freely.
pub fn stored_asset_reference_for(entry_path: &Path, target: &str) -> Option<String> {
    let dir_name = entry_assets_dir_name(entry_path)?;
    stored_asset_reference(target, &dir_name).map(|reference| reference.file_name)
}

/// A body line that is exactly one image stored in the entry's own asset folder,
/// optionally wrapped in a single link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoleStoredImage {
    /// The image's alt text.
    pub alt: String,
    /// The image's file name inside `<stem>.assets/`.
    pub file_name: String,
    /// The wrapping link's target, for the `[![alt](img)](href)` shape web and
    /// Day One imports leave behind. `None` for a bare image line.
    pub link: Option<String>,
}

/// If `line` (ignoring surrounding whitespace) is exactly one markdown image
/// pointing inside `entry_path`'s own `<stem>.assets/` folder — bare, or wrapped
/// in a single link — return it; any other text or a second image rejects it.
///
/// Shared by the entry-view labels and the fullscreen viewer so an image's
/// position (and its `Image N` number) is identical everywhere.
pub fn sole_stored_image(line: &str, entry_path: &Path) -> Option<SoleStoredImage> {
    let dir_name = entry_assets_dir_name(entry_path)?;
    let trimmed = line.trim();
    let image = next_markdown_image(trimmed)?;
    let link = match image.start {
        0 if image.end == trimmed.len() => None,
        1 => Some(wrapping_link_target(trimmed, &image)?.to_string()),
        _ => return None,
    };
    let target = trimmed[image.target_range()].trim();
    let reference = stored_asset_reference(target, &dir_name)?;
    Some(SoleStoredImage {
        alt: image.alt(trimmed).to_string(),
        file_name: reference.file_name,
        link,
    })
}

/// The href of the link wrapping `image`, when `trimmed` is exactly
/// `[<image>](href)`. Deliberately strict: anything looser falls through to the
/// markdown renderer, so a disagreement with the real parser costs a label, never
/// a wrong target.
fn wrapping_link_target<'a>(trimmed: &'a str, image: &MarkdownImage) -> Option<&'a str> {
    let bytes = trimmed.as_bytes();
    if bytes.first() != Some(&b'[') || !trimmed[image.end..].starts_with("](") {
        return None;
    }
    let open = image.end + 2;
    let close = trimmed.len().checked_sub(1)?;
    if open > close || bytes[close] != b')' {
        return None;
    }
    let href = trimmed[open..close].trim();
    // Parens, quotes, and whitespace are what a second inline, a parenthesized
    // URL, and a `"title"` suffix all show up as.
    if href.contains(['(', ')', '"']) || href.chars().any(char::is_whitespace) {
        return None;
    }
    Some(href)
}

/// Resolve a canonical stored asset name to an absolute path, rejecting
/// symlinks and any file that escapes the entry's own asset folder.
pub fn resolve_entry_asset_path(entry_path: &Path, file_name: &str) -> AppResult<Option<PathBuf>> {
    let Some(dir_name) = entry_assets_dir_name(entry_path) else {
        return Ok(None);
    };
    if stored_asset_reference(&format!("{dir_name}/{file_name}"), &dir_name).is_none() {
        return Ok(None);
    }

    let Some(assets_dir) = entry_assets_dir(entry_path) else {
        return Ok(None);
    };

    // A body link is always clean (`<id>.<ext>`); the file on disk carries a
    // `.age` suffix when encrypted. Try the plaintext name, then the encrypted
    // sibling.
    for candidate in asset_name_candidates(file_name) {
        if let Some(path) = resolve_regular_file(&assets_dir, &candidate)? {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// The on-disk names a clean reference `<id>.<ext>` might map to: the plaintext
/// file itself, or its encrypted `.age` sibling.
fn asset_name_candidates(file_name: &str) -> [String; 2] {
    [file_name.to_string(), format!("{file_name}.age")]
}

/// Resolve `file_name` in `assets_dir` to an absolute path if it's a regular
/// file that doesn't escape the folder (rejecting symlinks and traversal).
fn resolve_regular_file(assets_dir: &Path, file_name: &str) -> AppResult<Option<PathBuf>> {
    let path = assets_dir.join(file_name);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !meta.file_type().is_file() || meta.file_type().is_symlink() {
        return Ok(None);
    }

    let assets_dir = fs::canonicalize(assets_dir)?;
    let path = fs::canonicalize(&path)?;
    if !path.starts_with(&assets_dir) {
        return Ok(None);
    }
    Ok(Some(path))
}

/// A located `![alt](target)` span.
pub(super) struct MarkdownImage {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) alt_start: usize,
    pub(super) alt_end: usize,
    pub(super) target_start: usize,
    pub(super) target_end: usize,
}

impl MarkdownImage {
    pub(super) fn alt<'a>(&self, source: &'a str) -> &'a str {
        &source[self.alt_start..self.alt_end]
    }

    pub(super) fn target_range(&self) -> std::ops::Range<usize> {
        self.target_start..self.target_end
    }
}

/// Find the next `![alt](target)` in `source` (no nested parens in target).
pub(super) fn next_markdown_image(source: &str) -> Option<MarkdownImage> {
    // First `![` immediately followed by a parenthesized target wins.
    let mut base = 0;
    loop {
        let start = base + source[base..].find("![")?;
        if let Some(span) = notema_domain::parse_inline_at(&source[start..]) {
            return Some(MarkdownImage {
                start,
                end: start + span.span.end,
                alt_start: start + span.text.start,
                alt_end: start + span.text.end,
                target_start: start + span.target.start,
                target_end: start + span.target.end,
            });
        }
        base = start + 2;
    }
}

/// A located `[text](target)` link span (never an image).
pub(super) struct MarkdownLink {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) text_start: usize,
    pub(super) text_end: usize,
    pub(super) target_start: usize,
    pub(super) target_end: usize,
}

impl MarkdownLink {
    pub(super) fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.text_start..self.text_end]
    }

    pub(super) fn target_range(&self) -> std::ops::Range<usize> {
        self.target_start..self.target_end
    }
}

/// Find the next `[text](target)` link in `source`. A `[` preceded by `!` is an
/// image marker and is skipped so it stays the image pass's responsibility.
pub(super) fn next_markdown_link(source: &str) -> Option<MarkdownLink> {
    let bytes = source.as_bytes();
    let mut base = 0;
    loop {
        let start = base + source[base..].find('[')?;
        if start > 0 && bytes[start - 1] == b'!' {
            base = start + 1;
            continue;
        }
        if let Some(span) = notema_domain::parse_inline_at(&source[start..])
            && !span.is_image
        {
            return Some(MarkdownLink {
                start,
                end: start + span.span.end,
                text_start: start + span.text.start,
                text_end: start + span.text.end,
                target_start: start + span.target.start,
                target_end: start + span.target.end,
            });
        }
        base = start + 1;
    }
}

pub(super) fn is_url(target: &str) -> bool {
    target.starts_with("http://") || target.starts_with("https://")
}

/// Strip the query/fragment from a URL, leaving the path portion.
pub(super) fn url_path(url: &str) -> &str {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    &url[..end]
}

/// True when a target should be ingested: a URL, or an existing local file not
/// already inside this entry's asset folder. `data:` URIs and internal
/// references are left untouched.
pub(super) fn is_external_target(target: &str, dir_name: &str) -> bool {
    if target.is_empty() || target.starts_with("data:") {
        return false;
    }
    if is_url(target) {
        return true;
    }
    if target.starts_with(&format!("{dir_name}/")) {
        return false;
    }
    expand_user(target).is_file()
}

/// Whether a bare source looks like an image by its extension.
pub(super) fn looks_like_image_source(source: &str) -> bool {
    let path = if is_url(source) {
        url_path(source)
    } else {
        source
    };
    extension_of(path).is_some_and(|ext| IMAGE_EXTENSIONS.contains(&ext.as_str()))
}

/// Resolve the image extension from the source name, falling back to sniffing
/// magic bytes.
pub(super) fn image_extension(name: &str, bytes: &[u8]) -> Option<String> {
    if let Some(ext) = extension_of(name)
        && IMAGE_EXTENSIONS.contains(&ext.as_str())
    {
        return Some(if ext == "jpeg" {
            "jpg".to_string()
        } else {
            ext
        });
    }
    sniff_extension(bytes).map(str::to_string)
}

pub(super) fn extension_of(name: &str) -> Option<String> {
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
}

/// Identify a supported image format from its magic bytes.
fn sniff_extension(bytes: &[u8]) -> Option<&'static str> {
    use image::ImageFormat;
    match image::guess_format(bytes).ok()? {
        ImageFormat::Png => Some("png"),
        ImageFormat::Jpeg => Some("jpg"),
        ImageFormat::Gif => Some("gif"),
        ImageFormat::WebP => Some("webp"),
        ImageFormat::Bmp => Some("bmp"),
        _ => None,
    }
}

/// Interpret a pasted/dragged path the way a shell would: strip a single layer
/// of surrounding quotes and remove backslash escapes (`\ ` → ` `, `\(` → `(`,
/// …). Terminals add these when a path with spaces or special characters is
/// dragged in. On Unix a backslash is never a path separator, so a lone `\x`
/// collapses to `x`.
pub(super) fn unescape_shell_path(raw: &str) -> String {
    let inner = if raw.len() >= 2
        && ((raw.starts_with('\'') && raw.ends_with('\''))
            || (raw.starts_with('"') && raw.ends_with('"')))
    {
        &raw[1..raw.len() - 1]
    } else {
        raw
    };

    if !inner.contains('\\') {
        return inner.to_string();
    }

    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Expand a leading `~/` to the user's home directory.
pub(super) fn expand_user(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}
