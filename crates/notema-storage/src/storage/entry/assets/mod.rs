//! Per-entry assets.
//!
//! [`StagedAssets::ingest`] copies/downloads external images (local paths or
//! `http(s)` URLs, in `![alt](target)` tags or bare on their own line) and
//! non-image file attachments (existing local files in `[label](target)` links)
//! into the entry's sibling `<stem>.assets/` folder, age-encrypting when the
//! store is encrypted, and rewrites references to the stored copy. Assets no
//! longer referenced by the rewritten body are deleted by
//! [`StagedAssets::commit`], once the entry that references them is on disk.
//!
//! Stored references are always canonical markdown pointing inside the entry's
//! own asset folder — `![alt](<stem>.assets/<id>.<ext>)` for images,
//! `[label](<stem>.assets/<id>.<ext>)` for attachments (the file on disk carries
//! an extra `.age` suffix when encrypted) — so plaintext entries stay viewable in
//! external markdown tools.

mod net;
mod refs;

pub use refs::{
    SoleStoredImage, resolve_entry_asset_path, sole_stored_image, stored_asset_reference_for,
};
use refs::{
    expand_user, extension_of, is_external_target, is_url, looks_like_image_source,
    next_markdown_image, next_markdown_link, stored_asset_reference, unescape_shell_path,
};

use super::create::EntryAssetOptions;
use super::paths::{entry_assets_dir, entry_assets_dir_name, random_id};
use crate::AppResult;
use anyhow::bail;
use net::{FetchError, fetch_source};
use notema_encryption::{self as crypto, KeyPaths};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{self, Cursor, Write},
    path::{Path, PathBuf},
};

/// Length of the random id used as an asset's filename stem.
const ASSET_ID_LEN: usize = 8;
/// Bounded retry count when allocating a collision-free asset id.
const ASSET_ID_ATTEMPTS: usize = 32;

/// Supported raster image extensions (lowercase, no dot).
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

#[derive(Debug, Default, PartialEq, Eq)]
pub struct AssetReport {
    /// Files copied/downloaded into the asset folder.
    pub stored: usize,
    /// Stored files referenced as attachments rather than image embeds.
    pub attachments_stored: usize,
    /// Orphaned assets deleted during cleanup.
    pub removed: usize,
    /// Sources that could not be ingested, tagged by cause so callers can tell a
    /// benign remote skip from a genuine failure without parsing message text.
    pub failed: Vec<AssetFailure>,
    /// The orphan sweep could not finish after the entry was written. The entry
    /// is saved; unreferenced files were left in the asset folder. Kept separate
    /// from `failed`, whose variants all name a source that did not get stored.
    pub cleanup_failed: Option<String>,
}

/// Why an external asset reference was not stored, carrying enough to report it.
#[derive(Debug, PartialEq, Eq)]
pub enum AssetFailure {
    /// A remote source deliberately not fetched (downloads disabled) or whose
    /// host was unreachable. Benign: the reference is kept, or replaced with the
    /// offline placeholder — not a real ingestion failure.
    RemoteUnavailable { source: String },
    /// A source that should have ingested but errored: missing local file,
    /// unsupported/undecodable image, or a write failure.
    Ingest { source: String, error: String },
    /// A local attachment that could not be read or stored.
    AttachmentIngest { source: String, error: String },
}

impl AssetReport {
    /// Stored files referenced as image embeds.
    pub fn images_stored(&self) -> usize {
        self.stored.saturating_sub(self.attachments_stored)
    }

    /// Image sources that were unavailable or failed ingestion.
    pub fn images_not_stored(&self) -> usize {
        self.failed
            .iter()
            .filter(|failure| !matches!(failure, AssetFailure::AttachmentIngest { .. }))
            .count()
    }

    /// Attachment sources that failed ingestion.
    pub fn attachments_not_stored(&self) -> usize {
        self.failed
            .iter()
            .filter(|failure| matches!(failure, AssetFailure::AttachmentIngest { .. }))
            .count()
    }

    pub fn is_noop(&self) -> bool {
        self.stored == 0
            && self.removed == 0
            && self.failed.is_empty()
            && self.cleanup_failed.is_none()
    }
}

/// Ingest external image and attachment references, then delete orphaned assets.
#[cfg(test)]
pub(crate) fn ingest_and_cleanup(
    entry_path: &Path,
    body: &str,
    encryption: Option<&KeyPaths>,
    download_remote: bool,
) -> AppResult<(Option<String>, AssetReport)> {
    let mut staged = StagedAssets::for_entry(entry_path);
    let rewritten = staged.ingest(
        body,
        encryption,
        EntryAssetOptions {
            download_remote,
            replace_offline: false,
        },
    )?;
    Ok((rewritten, staged.commit()))
}

/// Where an entry's assets live: the folder, and the name body links use for it.
struct AssetFolder {
    dir: PathBuf,
    name: String,
}

/// Files this operation wrote into an entry's asset folder, held until the entry
/// that references them is on disk.
///
/// Dropping without [`commit`](Self::commit) removes exactly the files this
/// operation created — never anything that was already there, and the folder
/// itself only when this operation created it. Rollback can only ever touch a
/// path obtained through `create_new`, so it cannot remove a file another
/// process wrote.
///
/// Deliberate destruction — emptying an entry, trashing one, deleting a journal
/// — does not go through here and must not be staged.
#[must_use = "commit once the entry is written, or drop to roll the staged files back"]
pub(crate) struct StagedAssets {
    folder: Option<AssetFolder>,
    /// Whether this operation created the asset folder, so rollback knows
    /// whether removing it is its business.
    created_dir: bool,
    staged: Vec<PathBuf>,
    /// The rewritten body, kept so the deferred orphan sweep knows what is still
    /// referenced. `None` until `ingest` runs, so a commit without one sweeps
    /// nothing rather than treating an empty body as "nothing is referenced".
    body: Option<String>,
    report: AssetReport,
    committed: bool,
}

impl StagedAssets {
    /// Arm rollback for `entry_path`'s asset folder, before anything writes into
    /// it.
    pub(crate) fn for_entry(entry_path: &Path) -> Self {
        let folder = entry_assets_dir(entry_path)
            .zip(entry_assets_dir_name(entry_path))
            .map(|(dir, name)| AssetFolder { dir, name });
        let created_dir = folder.as_ref().is_some_and(|folder| !folder.dir.exists());
        Self {
            folder,
            created_dir,
            staged: Vec::new(),
            body: None,
            report: AssetReport::default(),
            committed: false,
        }
    }

    /// Record a file this operation created, so rollback removes it. Call once
    /// the path is reserved and before writing to it.
    pub(crate) fn stage(&mut self, path: PathBuf) {
        self.staged.push(path);
    }

    /// Ingest external image and attachment references, writing new assets but
    /// deleting nothing.
    ///
    /// `encryption` is `Some` when the store encrypts entries (assets get an
    /// `.age` suffix and are age-encrypted); `download_remote` gates fetching
    /// `http(s)` URLs, and `replace_offline` swaps an image that could not be
    /// ingested for an `[Offline Image]` placeholder instead of leaving the dead
    /// link in the body. Returns the rewritten body only when it changed.
    /// Sources that fail to fetch are recorded in the report rather than
    /// aborting.
    pub(crate) fn ingest(
        &mut self,
        body: &str,
        encryption: Option<&KeyPaths>,
        options: EntryAssetOptions,
    ) -> AppResult<Option<String>> {
        let Some(folder) = &self.folder else {
            return Ok(None);
        };
        let encryption = encryption
            .map(crypto::EncryptionRecipients::for_store)
            .transpose()?;
        let (new_body, report) = {
            let mut ctx = IngestContext {
                assets_dir: &folder.dir,
                dir_name: &folder.name,
                encryption,
                download_remote: options.download_remote,
                replace_offline: options.replace_offline,
                asset_ids: existing_asset_ids(&folder.dir)?,
                stored_sources: HashMap::new(),
                staged: &mut self.staged,
                report: AssetReport::default(),
            };
            let new_body = rewrite_body(body, &mut ctx);
            (new_body, ctx.report)
        };

        self.report = report;
        let changed = new_body != body;
        self.body = Some(new_body.clone());
        Ok(changed.then_some(new_body))
    }

    /// The entry bytes are on disk: keep everything staged, and sweep the assets
    /// the saved body no longer references.
    ///
    /// A sweep that fails is reported, not returned. The entry is written, and
    /// an error here would read as a lost save when all that happened is that
    /// some unreferenced files were left behind.
    pub(crate) fn commit(mut self) -> AssetReport {
        // Set first, so a failed sweep still leaves the staged files in place —
        // the entry on disk references them.
        self.committed = true;
        if let (Some(folder), Some(body)) = (&self.folder, &self.body)
            && let Err(error) = cleanup_orphans(&folder.dir, &folder.name, body, &mut self.report)
        {
            self.report.cleanup_failed = Some(error.to_string());
        }
        std::mem::take(&mut self.report)
    }
}

impl Drop for StagedAssets {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for path in &self.staged {
            let _ = fs::remove_file(path);
        }
        if self.created_dir
            && let Some(folder) = &self.folder
        {
            // Non-recursive on purpose: this succeeds only once the folder holds
            // nothing but what was just rolled back.
            let _ = fs::remove_dir(&folder.dir);
        }
    }
}

/// Retarget canonical stored-asset references (image embeds and attachment
/// links) when an entry is copied. Text outside Markdown targets, including
/// fenced code, is left byte-for-byte intact.
pub(crate) fn retarget_stored_asset_links(
    body: &str,
    source_dir_name: &str,
    target_dir_name: &str,
) -> String {
    let mut out = String::with_capacity(body.len());
    let mut in_fence = false;
    let mut lines = body.split('\n').peekable();
    while let Some(line) = lines.next() {
        if is_fence(line) {
            in_fence = !in_fence;
            push_line(&mut out, line, lines.peek().is_some());
            continue;
        }
        if in_fence {
            push_line(&mut out, line, lines.peek().is_some());
            continue;
        }

        let rewritten = retarget_line(line, source_dir_name, target_dir_name);
        push_line(&mut out, &rewritten, lines.peek().is_some());
    }
    out
}

/// Retarget every stored image embed then every stored attachment link on a
/// single line, leaving external and already-mismatched targets untouched.
fn retarget_line(line: &str, source_dir_name: &str, target_dir_name: &str) -> String {
    let mut images = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(image) = next_markdown_image(rest) {
        images.push_str(&rest[..image.target_start]);
        images.push_str(&retarget_target(
            &rest[image.target_range()],
            source_dir_name,
            target_dir_name,
        ));
        rest = &rest[image.target_end..];
    }
    images.push_str(rest);

    let mut out = String::with_capacity(images.len());
    let mut rest = images.as_str();
    while let Some(link) = next_markdown_link(rest) {
        out.push_str(&rest[..link.target_start]);
        out.push_str(&retarget_target(
            &rest[link.target_range()],
            source_dir_name,
            target_dir_name,
        ));
        rest = &rest[link.target_end..];
    }
    out.push_str(rest);
    out
}

/// Rewrite a single target to the new asset folder when it is a canonical
/// reference into `source_dir_name`; otherwise return it unchanged.
fn retarget_target(target: &str, source_dir_name: &str, target_dir_name: &str) -> String {
    match stored_asset_reference(target, source_dir_name) {
        Some(reference) => format!("{target_dir_name}/{}", reference.file_name),
        None => target.to_string(),
    }
}

struct IngestContext<'a> {
    assets_dir: &'a Path,
    dir_name: &'a str,
    encryption: Option<crypto::EncryptionRecipients>,
    download_remote: bool,
    replace_offline: bool,
    asset_ids: HashSet<String>,
    stored_sources: HashMap<String, String>,
    staged: &'a mut Vec<PathBuf>,
    report: AssetReport,
}

/// Placeholder substituted for an image that could not be ingested when
/// `replace_offline` is set. Keeps the source URL as the link target so the
/// reference isn't lost — the reader shows a labelled link instead of a dead
/// embed.
fn offline_image_placeholder(source: &str) -> String {
    format!("[Offline Image]({source})")
}

/// Rewrite a body line by line, ingesting external image references. Code
/// fences are passed through untouched.
fn rewrite_body(body: &str, ctx: &mut IngestContext<'_>) -> String {
    let mut out = String::with_capacity(body.len());
    let mut in_fence = false;

    let mut lines = body.split('\n').peekable();
    while let Some(line) = lines.next() {
        if is_fence(line) {
            in_fence = !in_fence;
            push_line(&mut out, line, lines.peek().is_some());
            continue;
        }
        if in_fence {
            push_line(&mut out, line, lines.peek().is_some());
            continue;
        }

        let rewritten = rewrite_markdown_images(line, ctx);
        let rewritten = match rewrite_bare_line(&rewritten, ctx) {
            Some(replacement) => replacement,
            None => rewritten,
        };
        let rewritten = rewrite_markdown_links(&rewritten, ctx);
        push_line(&mut out, &rewritten, lines.peek().is_some());
    }

    out
}

fn push_line(out: &mut String, line: &str, more: bool) {
    out.push_str(line);
    if more {
        out.push('\n');
    }
}

fn is_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

/// Replace every external `![alt](target)` in a line with a canonical stored
/// reference.
fn rewrite_markdown_images(line: &str, ctx: &mut IngestContext<'_>) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;

    while let Some(image) = next_markdown_image(rest) {
        out.push_str(&rest[..image.start]);
        let target = rest[image.target_range()].trim();
        if is_external_target(target, ctx.dir_name) {
            match store_source(target, image.alt(rest), ctx) {
                Some(link) => out.push_str(&link),
                None if ctx.replace_offline => {
                    out.push_str(&offline_image_placeholder(target));
                }
                None => out.push_str(&rest[image.start..image.end]),
            }
        } else {
            out.push_str(&rest[image.start..image.end]);
        }
        rest = &rest[image.end..];
    }

    out.push_str(rest);
    out
}

/// If the whole trimmed line is a single bare path (or image URL), ingest it:
/// image sources become `![](…)` embeds, any other existing local file becomes a
/// `[<filename>](…)` attachment link. This is what makes dragging a file onto the
/// terminal — which pastes its path — attach it.
fn rewrite_bare_line(line: &str, ctx: &mut IngestContext<'_>) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Interpret the line the way a shell would (unquote, unescape `\ ` → ` `):
    // dragging or pasting a path into a terminal escapes spaces and other
    // special characters, e.g. `/a/IMG\ 2.jpeg`.
    let source = if is_url(trimmed) {
        trimmed.to_string()
    } else {
        unescape_shell_path(trimmed)
    };
    // Rely on `is_file()` (not a whitespace heuristic) to reject prose: a real
    // path may contain spaces, e.g. `.../Photos Library.photoslibrary/foo.jpeg`.
    if !is_external_target(&source, ctx.dir_name) {
        return None;
    }

    let indent = &line[..line.len() - line.trim_start().len()];
    if looks_like_image_source(&source) {
        return match store_source(&source, "", ctx) {
            Some(link) => Some(format!("{indent}{link}")),
            None if ctx.replace_offline => {
                Some(format!("{indent}{}", offline_image_placeholder(&source)))
            }
            None => None,
        };
    }
    // A bare non-image line only attaches an existing *local* file; a remote URL
    // to some other file type is left as prose (attachments are never fetched).
    if is_url(&source) {
        return None;
    }
    let label = bare_attachment_label(&source);
    store_file_source(&source, &label, ctx).map(|link| format!("{indent}{link}"))
}

/// A human label for a bare-pasted attachment: the source's file name (without
/// any Markdown-breaking `[`/`]`), falling back to `attachment`.
fn bare_attachment_label(source: &str) -> String {
    Path::new(source)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.replace(['[', ']'], ""))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "attachment".to_string())
}

/// Replace every external file `[label](target)` in a line with a canonical
/// stored attachment reference. Only existing local files are ingested; URLs,
/// `#anchors`, `data:` URIs, and references already inside the asset folder pass
/// through untouched — attachments are never downloaded from remote hosts.
fn rewrite_markdown_links(line: &str, ctx: &mut IngestContext<'_>) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;

    while let Some(link) = next_markdown_link(rest) {
        out.push_str(&rest[..link.start]);
        let target = rest[link.target_range()].trim();
        if !is_url(target) && is_external_target(target, ctx.dir_name) {
            match store_file_source(target, link.text(rest), ctx) {
                Some(replacement) => out.push_str(&replacement),
                None => out.push_str(&rest[link.start..link.end]),
            }
        } else {
            out.push_str(&rest[link.start..link.end]);
        }
        rest = &rest[link.end..];
    }

    out.push_str(rest);
    out
}

/// Copy a local file into the asset folder (encrypted when configured) and
/// return the canonical `[label](<dir>/<file>)` link. Identical sources are
/// stored once and reused. Returns `None` on failure, recording it in the
/// report.
fn store_file_source(source: &str, label: &str, ctx: &mut IngestContext<'_>) -> Option<String> {
    if let Some(file_name) = ctx.stored_sources.get(source) {
        return Some(markdown_link(label, ctx.dir_name, file_name));
    }

    let ext = attachment_extension(source);
    store_asset(
        source,
        AssetReference::Attachment(label),
        AssetData::File(expand_user(source)),
        &ext,
        ctx,
    )
}

fn markdown_link(label: &str, dir_name: &str, file_name: &str) -> String {
    format!("[{label}]({dir_name}/{file_name})")
}

/// The lowercased file extension of an attachment source, defaulting to `bin`
/// when the path carries none.
fn attachment_extension(source: &str) -> String {
    extension_of(source).unwrap_or_else(|| "bin".to_string())
}

/// Fetch a source, store it in the asset folder (encrypted when configured),
/// and return the canonical reference. Identical sources are stored once and
/// reused. Returns `None` on failure, recording it in the report.
fn store_source(source: &str, alt: &str, ctx: &mut IngestContext<'_>) -> Option<String> {
    if let Some(file_name) = ctx.stored_sources.get(source) {
        return Some(markdown_image(alt, ctx.dir_name, file_name));
    }

    let (bytes, ext) = match fetch_source(source, ctx.download_remote) {
        Ok(value) => value,
        Err(FetchError::RemoteUnavailable) => {
            ctx.report.failed.push(AssetFailure::RemoteUnavailable {
                source: source.to_string(),
            });
            return None;
        }
        Err(FetchError::Ingest(error)) => {
            ctx.report.failed.push(AssetFailure::Ingest {
                source: source.to_string(),
                error,
            });
            return None;
        }
    };

    store_asset(
        source,
        AssetReference::Image(alt),
        AssetData::Bytes(bytes),
        &ext,
        ctx,
    )
}

fn markdown_image(alt: &str, dir_name: &str, file_name: &str) -> String {
    format!("![{alt}]({dir_name}/{file_name})")
}

enum AssetReference<'a> {
    Image(&'a str),
    Attachment(&'a str),
}

impl AssetReference<'_> {
    fn render(&self, dir_name: &str, file_name: &str) -> String {
        match self {
            Self::Image(alt) => markdown_image(alt, dir_name, file_name),
            Self::Attachment(label) => markdown_link(label, dir_name, file_name),
        }
    }

    fn failure(&self, source: String, error: String) -> AssetFailure {
        match self {
            Self::Image(_) => AssetFailure::Ingest { source, error },
            Self::Attachment(_) => AssetFailure::AttachmentIngest { source, error },
        }
    }

    fn is_attachment(&self) -> bool {
        matches!(self, Self::Attachment(_))
    }
}

enum AssetData {
    Bytes(Vec<u8>),
    File(PathBuf),
}

impl AssetData {
    fn write_to(
        &self,
        output: &mut fs::File,
        encryption: Option<&crypto::EncryptionRecipients>,
    ) -> AppResult<()> {
        match (self, encryption) {
            (Self::Bytes(bytes), Some(recipients)) => {
                recipients.encrypt_reader(Cursor::new(bytes), output)?;
            }
            (Self::File(path), Some(recipients)) => {
                recipients.encrypt_reader(fs::File::open(path)?, output)?;
            }
            (Self::Bytes(bytes), None) => output.write_all(bytes)?,
            (Self::File(path), None) => {
                io::copy(&mut fs::File::open(path)?, output)?;
            }
        }
        Ok(())
    }
}

fn store_asset(
    source: &str,
    reference: AssetReference<'_>,
    data: AssetData,
    ext: &str,
    ctx: &mut IngestContext<'_>,
) -> Option<String> {
    match write_asset(ctx, &data, ext) {
        Ok(file_name) => {
            ctx.report.stored += 1;
            if reference.is_attachment() {
                ctx.report.attachments_stored += 1;
            }
            ctx.stored_sources
                .insert(source.to_string(), file_name.clone());
            Some(reference.render(ctx.dir_name, &file_name))
        }
        Err(error) => {
            ctx.report
                .failed
                .push(reference.failure(source.to_string(), error.to_string()));
            None
        }
    }
}

/// Write asset data under a collision-free random id. Encrypted files gain an
/// on-disk `.age` suffix while body references stay unchanged.
fn write_asset(ctx: &mut IngestContext<'_>, data: &AssetData, ext: &str) -> AppResult<String> {
    fs::create_dir_all(ctx.assets_dir)?;

    for _ in 0..ASSET_ID_ATTEMPTS {
        let id = random_id(ASSET_ID_LEN);
        if !ctx.asset_ids.insert(id.clone()) {
            continue;
        }
        let link_name = format!("{id}.{ext}");
        let disk_name = match ctx.encryption {
            Some(_) => format!("{link_name}.age"),
            None => link_name.clone(),
        };
        let path = ctx.assets_dir.join(&disk_name);
        let mut output = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        };
        // `create_new` reserved the path, so it is ours to roll back. Record it
        // before writing, so a failure mid-write is covered too.
        ctx.staged.push(path.clone());
        // A fresh asset writes straight to its final name (no atomic rename), so
        // fsync the file and its directory to match the entry-write durability.
        let write_result = data
            .write_to(&mut output, ctx.encryption.as_ref())
            .and_then(|()| output.sync_all().map_err(Into::into));
        if let Err(error) = write_result {
            drop(output);
            let _ = fs::remove_file(&path);
            return Err(error);
        }
        crypto::sync_parent_dir(&path);
        return Ok(link_name);
    }

    bail!("could not allocate a unique asset id")
}

fn existing_asset_ids(assets_dir: &Path) -> AppResult<HashSet<String>> {
    let mut ids = HashSet::new();
    if !assets_dir.exists() {
        return Ok(ids);
    }
    for item in fs::read_dir(assets_dir)? {
        let item = item?;
        if let Some(name) = item.file_name().to_str()
            && let Some((id, _)) = name.split_once('.')
        {
            ids.insert(id.to_string());
        }
    }
    Ok(ids)
}

/// Delete assets in the folder not referenced by any in-folder link in `body`.
fn cleanup_orphans(
    assets_dir: &Path,
    dir_name: &str,
    body: &str,
    report: &mut AssetReport,
) -> AppResult<()> {
    if !assets_dir.exists() {
        return Ok(());
    }

    let referenced = referenced_asset_files(body, dir_name);
    let mut remaining = 0usize;
    for item in fs::read_dir(assets_dir)? {
        let item = item?;
        if !item.file_type()?.is_file() {
            remaining += 1;
            continue;
        }
        // Body links are clean, but the file may carry a `.age` suffix — compare
        // by the clean key so a referenced encrypted asset isn't seen as orphaned.
        let name = item.file_name().to_string_lossy().to_string();
        let key = name.strip_suffix(".age").unwrap_or(&name);
        if referenced.contains(key) {
            remaining += 1;
        } else {
            fs::remove_file(item.path())?;
            report.removed += 1;
        }
    }

    if remaining == 0 {
        let _ = fs::remove_dir(assets_dir);
    }

    Ok(())
}

/// Collect the file names referenced by canonical in-folder references — both
/// `![...](<dir_name>/<file>)` image embeds and `[...](<dir_name>/<file>)`
/// attachment links — so neither kind is pruned as an orphan.
fn referenced_asset_files(body: &str, dir_name: &str) -> HashSet<String> {
    let mut files = HashSet::new();
    let mut rest = body;
    while let Some(image) = next_markdown_image(rest) {
        let target = rest[image.target_range()].trim();
        if let Some(reference) = stored_asset_reference(target, dir_name) {
            files.insert(reference.file_name);
        }
        rest = &rest[image.end..];
    }
    let mut rest = body;
    while let Some(link) = next_markdown_link(rest) {
        let target = rest[link.target_range()].trim();
        if let Some(reference) = stored_asset_reference(target, dir_name) {
            files.insert(reference.file_name);
        }
        rest = &rest[link.end..];
    }
    collect_pathlike_references(body, dir_name, &mut files);
    files
}

/// Also honour references the canonical markdown passes above miss — reference-
/// style definitions (`[id]: <dir>/<file>`) and raw HTML (`<img src="<dir>/<file>">`)
/// — by scanning for any `<dir_name>/<file>` token. Conservative on purpose: it
/// only ever keeps files, so a hand-written link never gets its asset deleted.
fn collect_pathlike_references(body: &str, dir_name: &str, files: &mut HashSet<String>) {
    let needle = format!("{dir_name}/");
    let mut search = body;
    while let Some(pos) = search.find(&needle) {
        let after = &search[pos + needle.len()..];
        let end = after
            .find(|c: char| {
                c.is_whitespace()
                    || matches!(
                        c,
                        '"' | '\'' | '(' | ')' | '[' | ']' | '<' | '>' | '`' | '/' | '?' | '#'
                    )
            })
            .unwrap_or(after.len());
        let file = &after[..end];
        if !file.is_empty() && file != "." && file != ".." {
            files.insert(file.to_string());
        }
        search = &after[end..];
    }
}

#[cfg(test)]
mod tests;
