use super::paths::{entry_id, is_assets_dir, is_encrypted_entry_file, is_entry_file};
use super::{Entry, EntryEncryptionState, EntryPath, ImportSource, Metadata, Timestamp};
use crate::storage::{journals::is_hidden_name, list_journals};
use crate::{
    AppResult, EntryRevision,
    library::{DiscoveredEntry, FileStamp},
    markdown::{FrontMatter, display_preview, split_front_matter},
};
use anyhow::Context;
use notema_domain::build_search_haystack;
use notema_domain::normalize_feelings;
use notema_encryption as crypto;
use rayon::prelude::*;
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
    time::SystemTime,
};

pub(crate) fn scan_import_sources(
    root: &Path,
    identity: Option<&crypto::UnlockedIdentity>,
) -> AppResult<Vec<ImportSource>> {
    collect_entry_paths(root)?
        .par_iter()
        .filter_map(
            |entry| match read_entry_import_source(&entry.path, identity) {
                Ok(source) => source.map(Ok),
                Err(error) => Some(Err(error)),
            },
        )
        .collect()
}

/// Walk the journal tree once and collect every entry file path without reading
/// any file contents. Skips hidden directories.
pub(crate) fn collect_entry_paths(root: &Path) -> AppResult<Vec<EntryPath>> {
    Ok(collect_discovered_entries(&list_journals(root)?)?
        .into_iter()
        .map(|entry| entry.source)
        .collect())
}

pub(crate) fn collect_discovered_entries(
    journals: &[crate::Journal],
) -> AppResult<Vec<DiscoveredEntry>> {
    collect_discovered_entries_with_progress(journals, None)
}

pub(crate) fn collect_discovered_entries_with_progress(
    journals: &[crate::Journal],
    progress: Option<&(dyn Fn(usize) + Sync)>,
) -> AppResult<Vec<DiscoveredEntry>> {
    // One clock read for the whole walk, taken before the first stat. Every
    // file is therefore judged against a time no later than the one it was
    // actually statted at, so the racy test errs toward distrust.
    let walk_started = SystemTime::now();
    let mut entries = Vec::new();
    for journal in journals {
        collect_paths(
            &journal.name,
            &journal.path,
            walk_started,
            &mut entries,
            progress,
        )?;
    }
    Ok(entries)
}

fn collect_paths(
    journal: &str,
    dir: &Path,
    walk_started: SystemTime,
    entries: &mut Vec<DiscoveredEntry>,
    progress: Option<&(dyn Fn(usize) + Sync)>,
) -> AppResult<()> {
    if !dir.exists() {
        return Ok(());
    }

    for item in fs::read_dir(dir)? {
        let item = item?;
        let path = item.path();
        let name = item.file_name().to_string_lossy().to_string();
        if item.file_type()?.is_dir() {
            if !is_hidden_name(&name) && !is_assets_dir(&path) {
                collect_paths(journal, &path, walk_started, entries, progress)?;
            }
            continue;
        }

        if is_entry_file(&path) {
            let stamp = FileStamp::from_metadata(&item.metadata()?);
            entries.push(DiscoveredEntry {
                source: EntryPath {
                    journal: journal.to_string(),
                    path,
                },
                stamp_trusted: stamp.is_trustworthy_at(walk_started),
                stamp,
            });
            if let Some(progress) = progress {
                progress(entries.len());
            }
        }
    }

    Ok(())
}

/// Read and parse (and, when encrypted, decrypt) the given entry paths in
/// parallel, returning them sorted newest-first alongside a message for every
/// entry that had to be degraded to an unreadable placeholder.
pub(crate) fn read_entries(
    paths: Vec<EntryPath>,
    identity: Option<&crypto::UnlockedIdentity>,
) -> AppResult<(Vec<Entry>, Vec<String>)> {
    read_entries_with_progress(paths, identity, None)
}

pub(crate) fn read_entries_with_progress(
    paths: Vec<EntryPath>,
    identity: Option<&crypto::UnlockedIdentity>,
    progress: Option<&(dyn Fn(usize, usize) + Sync)>,
) -> AppResult<(Vec<Entry>, Vec<String>)> {
    let total = paths.len();
    let completed = AtomicUsize::new(0);
    let outcomes = paths
        .par_iter()
        .map(|entry| {
            let outcome = read_scan_entry(&entry.journal, &entry.path, identity);
            let current = completed.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(progress) = progress {
                progress(current, total);
            }
            outcome
        })
        .collect::<Vec<ScanOutcome>>();

    let mut entries = Vec::with_capacity(outcomes.len());
    let mut failures = Vec::new();
    for outcome in outcomes {
        match outcome {
            ScanOutcome::Loaded(entry) => entries.push(entry),
            ScanOutcome::Failed { entry, message } => {
                if let Some(entry) = entry {
                    entries.push(entry);
                }
                failures.push(message);
            }
        }
    }
    entries.sort_by(|a, b| b.path.cmp(&a.path));
    Ok((entries, failures))
}

/// One entry's scan result: a usable entry (real or a locked/unreadable
/// placeholder) or a hard per-file failure degraded to a placeholder plus a
/// message aggregated into the load report.
enum ScanOutcome {
    Loaded(Entry),
    Failed {
        entry: Option<Entry>,
        message: String,
    },
}

/// Read one entry for a scan. A per-file read/parse failure degrades to an
/// unreadable placeholder rather than aborting the whole scan — mirroring how a
/// locked or undecryptable encrypted entry already degrades. The edit path
/// [`read_entry_with_revision`] keeps hard-failing: a real edit must not open a
/// placeholder.
fn read_scan_entry(
    journal: &str,
    path: &Path,
    identity: Option<&crypto::UnlockedIdentity>,
) -> ScanOutcome {
    match read_entry(journal, path, identity) {
        Ok(entry) => ScanOutcome::Loaded(entry),
        Err(error) => {
            let message = format!("{}: {error:#}", path.display());
            // A placeholder needs the entry id; if even that is unavailable the
            // entry is dropped, but the failure is still reported.
            let entry = plaintext_unreadable_entry(journal, path).ok();
            ScanOutcome::Failed { entry, message }
        }
    }
}

#[cfg(test)]
pub(super) fn scan_entries(
    root: &Path,
    identity: Option<&crypto::UnlockedIdentity>,
) -> AppResult<Vec<Entry>> {
    read_entries(collect_entry_paths(root)?, identity).map(|(entries, _)| entries)
}

#[cfg(test)]
pub(super) fn scan_entries_with_failures(
    root: &Path,
    identity: Option<&crypto::UnlockedIdentity>,
) -> AppResult<(Vec<Entry>, Vec<String>)> {
    read_entries(collect_entry_paths(root)?, identity)
}

/// The state an entry file's bytes would decode to, or `None` when it is an
/// encrypted entry this device holds no identity for. Decided from the path
/// alone, so a caller can decline to read bytes it could not use.
fn decodable_state(
    path: &Path,
    identity: Option<&crypto::UnlockedIdentity>,
) -> Option<EntryEncryptionState> {
    if is_encrypted_entry_file(path) {
        identity
            .is_some()
            .then_some(EntryEncryptionState::EncryptedUnlocked)
    } else {
        Some(EntryEncryptionState::Plain)
    }
}

/// The scan path. The locked check stays above the read: a locked store must
/// return placeholders without opening ciphertext it cannot decrypt, or every
/// launch reads the whole library and throws it away.
pub(crate) fn read_entry(
    journal: &str,
    path: &Path,
    identity: Option<&crypto::UnlockedIdentity>,
) -> AppResult<Entry> {
    let Some(encryption_state) = decodable_state(path, identity) else {
        return locked_entry(journal, path);
    };
    entry_from_stored_bytes(journal, path, identity, encryption_state, fs::read(path)?)
}

/// The edit path. The entry and its revision come from one read, so they always
/// describe the same stored bytes. A locked entry is read here — a revision of
/// the ciphertext is exactly what the caller asked for — which is why this is
/// not the function a scan uses.
pub(crate) fn read_entry_with_revision(
    journal: &str,
    path: &Path,
    identity: Option<&crypto::UnlockedIdentity>,
) -> AppResult<(Entry, EntryRevision)> {
    let bytes = fs::read(path)?;
    let revision = EntryRevision::from_bytes(&bytes);
    let entry = match decodable_state(path, identity) {
        Some(encryption_state) => {
            entry_from_stored_bytes(journal, path, identity, encryption_state, bytes)?
        }
        None => locked_entry(journal, path)?,
    };
    Ok((entry, revision))
}

fn entry_from_stored_bytes(
    journal: &str,
    path: &Path,
    identity: Option<&crypto::UnlockedIdentity>,
    encryption_state: EntryEncryptionState,
    bytes: Vec<u8>,
) -> AppResult<Entry> {
    let content = match decode_entry_content(path, bytes, identity) {
        Ok(content) => content,
        // An encrypted entry the loaded identity can't decrypt (e.g. a device
        // not yet approved as a recipient, or a partially re-encrypted store)
        // degrades to a locked placeholder rather than failing the whole scan.
        Err(error) if matches!(encryption_state, EntryEncryptionState::EncryptedUnlocked) => {
            if error
                .downcast_ref::<crate::EncryptionError>()
                .is_some_and(crate::EncryptionError::is_no_matching_keys)
            {
                return locked_entry(journal, path);
            }
            return unreadable_entry(journal, path);
        }
        Err(error) => return Err(error),
    };
    let (front_matter, body) = split_front_matter(&content);
    let (parsed, warning) = match front_matter {
        Some(front_matter) => match crate::markdown::parse_front_matter(front_matter) {
            Ok(parsed) => (parsed, None),
            Err(error) => (FrontMatter::default(), Some(error.user_message())),
        },
        None => (FrontMatter::default(), None),
    };
    // One TOML parse per entry instead of one per field.
    let FrontMatter {
        schema_version: _,
        mut metadata,
        datetime,
        import,
        location,
        weather,
        celestial,
        air_quality,
    } = parsed;
    metadata.feelings = normalize_feelings(metadata.feelings.iter().map(String::as_str));
    let created_at = datetime.created_at.map(Timestamp::parse);
    // `datetime.timezone`/`writing_seconds` are capture-only: preserved on disk,
    // not surfaced here.
    let edited_at = datetime.edited_at;
    let id = entry_id(path).context("entry file has no UTF-8 stem")?;
    let preview = display_preview(body);
    let body = body.trim_start_matches('\n').to_string();
    let word_count = body.split_whitespace().count();
    let search_haystack = build_search_haystack(&body, &metadata);

    // The `[location]` table is its own front-matter field; the flattened
    // `metadata` never carries it (skipped by serde), so ignore that slot.
    let Metadata {
        activities,
        feelings,
        people,
        tags,
        mood,
        starred,
        location: _,
    } = metadata;

    Ok(Entry {
        id,
        journal: journal.to_string(),
        path: path.to_path_buf(),
        encryption_state,
        created_at,
        edited_at,
        preview,
        activities,
        feelings,
        people,
        tags,
        mood,
        starred,
        location,
        weather,
        celestial,
        air_quality,
        import,
        body,
        word_count,
        search_haystack,
        warning,
    })
}

/// The `[import]` provenance of one entry, for dedupe during a re-import. An
/// entry that can't be read (undecryptable, IO error) or whose front matter is
/// malformed contributes no provenance rather than aborting the whole scan — a
/// single bad file must not block importing everything else. The trade-off is
/// that such an entry isn't matched for dedup, so it could re-import as a
/// duplicate; that is preferable to failing the import outright.
fn read_entry_import_source(
    path: &Path,
    identity: Option<&crypto::UnlockedIdentity>,
) -> AppResult<Option<ImportSource>> {
    let Ok(content) = read_entry_content(path, identity) else {
        return Ok(None);
    };
    let (front_matter, _) = split_front_matter(&content);
    Ok(front_matter
        .and_then(|front_matter| crate::markdown::parse_front_matter(front_matter).ok())
        .and_then(|parsed| parsed.import))
}

fn locked_entry(journal: &str, path: &Path) -> AppResult<Entry> {
    placeholder_entry(
        journal,
        path,
        EntryEncryptionState::EncryptedLocked,
        "[locked] Encrypted entry",
        "Encryption identity not available",
    )
}

fn unreadable_entry(journal: &str, path: &Path) -> AppResult<Entry> {
    placeholder_entry(
        journal,
        path,
        EntryEncryptionState::EncryptedUnreadable,
        "[unreadable] Encrypted entry",
        "Encrypted entry could not be decrypted",
    )
}

/// A stand-in for a non-encrypted entry whose file could not be read or parsed
/// during a scan, so the failure degrades to a placeholder instead of failing
/// the whole library load.
fn plaintext_unreadable_entry(journal: &str, path: &Path) -> AppResult<Entry> {
    placeholder_entry(
        journal,
        path,
        EntryEncryptionState::Unreadable,
        "[unreadable] Entry",
        "Entry could not be read",
    )
}

/// A dateless stand-in for an encrypted entry this device can't render as text:
/// either it has no usable key (locked) or decryption failed (unreadable).
fn placeholder_entry(
    journal: &str,
    path: &Path,
    encryption_state: EntryEncryptionState,
    preview: &str,
    body: &str,
) -> AppResult<Entry> {
    let id = entry_id(path).context("entry file has no UTF-8 stem")?;
    Ok(Entry {
        id,
        journal: journal.to_string(),
        path: path.to_path_buf(),
        encryption_state,
        created_at: None,
        edited_at: None,
        preview: preview.to_string(),
        activities: Vec::new(),
        feelings: Vec::new(),
        people: Vec::new(),
        tags: Vec::new(),
        mood: None,
        starred: false,
        location: None,
        weather: None,
        celestial: None,
        air_quality: None,
        import: None,
        body: body.to_string(),
        word_count: 0,
        search_haystack: String::new(),
        warning: None,
    })
}

/// Read an entry's text. Takes no revision, so it does no hashing. The locked
/// check stays above the read for the same reason it does in [`read_entry`].
pub(crate) fn read_entry_content(
    path: &Path,
    identity: Option<&crypto::UnlockedIdentity>,
) -> AppResult<String> {
    if decodable_state(path, identity).is_none() {
        return Err(crate::EncryptionError::Locked { context: "entry" }.into());
    }
    decode_entry_content(path, fs::read(path)?, identity)
}

/// The one read that hashes. Used only where the caller will guard a write with
/// the revision.
pub(crate) fn read_entry_content_with_revision(
    path: &Path,
    identity: Option<&crypto::UnlockedIdentity>,
) -> AppResult<(String, EntryRevision)> {
    let bytes = fs::read(path)?;
    let revision = EntryRevision::from_bytes(&bytes);
    Ok((decode_entry_content(path, bytes, identity)?, revision))
}

/// Decode bytes already read. `path` only selects the codec; nothing is read.
fn decode_entry_content(
    path: &Path,
    bytes: Vec<u8>,
    identity: Option<&crypto::UnlockedIdentity>,
) -> AppResult<String> {
    if is_encrypted_entry_file(path) {
        let identity = identity.ok_or(crate::EncryptionError::Locked { context: "entry" })?;
        let ciphertext = crypto::CiphertextBytes::from_vec(bytes);
        Ok(crypto::decrypt_bytes(identity, &ciphertext)?.into_string()?)
    } else {
        Ok(String::from_utf8(bytes)?)
    }
}
