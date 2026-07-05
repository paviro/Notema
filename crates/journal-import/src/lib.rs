//! Import external journals into the store.
//!
//! Currently supports [Day One](https://dayoneapp.com/) JSON exports via
//! [`import_dayone`]. Each importer maps an external format onto the store's
//! entry model, records provenance (`import_id`) so re-runs skip already-imported
//! entries, and preserves original timestamps.

mod dayone;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use journal_storage::{AppResult, EntryMetadata, JournalStore};

use dayone::body::{html_images_to_markdown, rewrite_moments, unescape_markdown};
use dayone::model::{DayOneEntry, DayOneExport, Moment};

/// Summary of a Day One import, printed to the user.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Entries created.
    pub imported: usize,
    /// Entries skipped because their `import_id` was already present.
    pub skipped_duplicate: usize,
    /// Photos copied into entry asset folders.
    pub images_stored: usize,
    /// Photos that could not be ingested (missing file, decode failure, …).
    pub images_failed: usize,
    /// Remote `http(s)` images that were not fetched. When downloading was on,
    /// these were unreachable and are replaced in the body with `[Offline
    /// Image]`; when off, they are left as links to fetch later. Not failures.
    pub remote_images_skipped: usize,
    /// Non-image attachments (audio/video/pdf) referenced but not imported.
    pub attachments_skipped: usize,
    /// Human-readable per-entry problems that did not abort the import.
    pub failures: Vec<String>,
}

/// Import every entry from a Day One JSON export at `json_path` into `journal`,
/// creating the journal if it does not exist. Media folders (e.g. `photos/`) are
/// resolved relative to the JSON file. Entries whose Day One UUID was already
/// imported are skipped.
///
/// `download_remote` gates fetching `http(s)` image links found in entry bodies
/// (Day One entries can embed remote images, distinct from local `photos`);
/// pass the store's configured preference, mirroring `journal log`.
pub fn import_dayone(
    store: &JournalStore,
    journal: &str,
    json_path: &Path,
    download_remote: bool,
) -> AppResult<ImportReport> {
    let raw = fs::read_to_string(json_path)
        .map_err(|error| format!("could not read {}: {error}", json_path.display()))?;
    let export: DayOneExport = serde_json::from_str(&raw)
        .map_err(|error| format!("could not parse Day One export: {error}"))?;
    let media_root = json_path.parent().unwrap_or_else(|| Path::new("."));

    if !store.list_journals()?.iter().any(|j| j.name == journal) {
        store.create_journal(journal)?;
    }

    let mut seen: HashSet<String> = store
        .scan_entries()?
        .into_iter()
        .filter_map(|entry| entry.import_id)
        .collect();

    let mut report = ImportReport::default();
    let empty: &[String] = &[];

    for entry in &export.entries {
        let import_id = format!("dayone:{}", entry.uuid);
        if seen.contains(&import_id) {
            report.skipped_duplicate += 1;
            continue;
        }

        let Some(created_at) = entry.creation_date.as_deref().and_then(parse_date) else {
            report
                .failures
                .push(format!("{}: missing or invalid creationDate", entry.uuid));
            continue;
        };
        let updated_at = entry
            .modified_date
            .as_deref()
            .and_then(parse_date)
            .unwrap_or(created_at);

        let media = MediaIndex::build(entry, media_root);
        let text = entry.text.as_deref().unwrap_or_default();
        // Day One bodies mix Markdown escaping, HTML `<img>` tags (older entries),
        // and `dayone-moment://` links. Normalize in that order so every image
        // ends up as a Markdown reference the moment/asset steps understand.
        let normalized = html_images_to_markdown(&unescape_markdown(text));
        let rewrite = rewrite_moments(
            &normalized,
            &media.photos,
            &media.audio,
            &media.video,
            &media.pdf,
        );

        let tags = entry.tags.clone();
        let metadata = EntryMetadata {
            tags: &tags,
            people: empty,
            activities: empty,
            feelings: empty,
            mood: None,
        };

        let path =
            store.create_imported_entry(journal, &rewrite.body, metadata, created_at, updated_at, &import_id)?;
        // Replace un-fetchable images with a placeholder only when we actually
        // tried to download — otherwise remote links are kept so they can be
        // fetched by a later `--download-images` run.
        let assets = store.process_entry_assets(&path, download_remote, download_remote)?;

        report.images_stored += assets.stored;
        for failure in assets.failed {
            // A remote link we chose not to (or couldn't) fetch — download off,
            // or the host is gone — is left in the body as a link, not a failure.
            // (Messages are prefixed with the source URL, so match by substring.)
            if failure.contains("remote downloads disabled")
                || failure.contains("host unreachable")
            {
                report.remote_images_skipped += 1;
            } else {
                report.images_failed += 1;
                report.failures.push(format!("{}: {failure}", entry.uuid));
            }
        }
        for id in &rewrite.unresolved {
            report
                .failures
                .push(format!("{}: unresolved photo moment {id}", entry.uuid));
        }
        report.attachments_skipped += rewrite.skipped_attachments();
        report.imported += 1;
        seen.insert(import_id);
    }

    Ok(report)
}

/// Identifier → on-disk location for an entry's media, split by kind. Photos map
/// to an absolute path (only files that exist); the other kinds are identifier
/// sets used to classify and count skipped attachments.
struct MediaIndex {
    photos: std::collections::HashMap<String, PathBuf>,
    audio: HashSet<String>,
    video: HashSet<String>,
    pdf: HashSet<String>,
}

impl MediaIndex {
    fn build(entry: &DayOneEntry, media_root: &Path) -> Self {
        let mut photos = std::collections::HashMap::new();
        for photo in &entry.photos {
            if let Some(path) = moment_path(photo, media_root, "photos")
                && path.is_file()
            {
                photos.insert(photo.identifier.clone(), path);
            }
        }
        Self {
            photos,
            audio: identifier_set(&entry.audios),
            video: identifier_set(&entry.videos),
            pdf: identifier_set(&entry.pdf_attachments),
        }
    }
}

fn identifier_set(moments: &[Moment]) -> HashSet<String> {
    moments.iter().map(|m| m.identifier.clone()).collect()
}

/// The on-disk path for a moment: `<media_root>/<folder>/<md5>.<type>`.
fn moment_path(moment: &Moment, media_root: &Path, folder: &str) -> Option<PathBuf> {
    let md5 = moment.md5.as_ref()?;
    let kind = moment.kind.as_ref()?;
    Some(media_root.join(folder).join(format!("{md5}.{kind}")))
}

fn parse_date(value: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Local))
}
