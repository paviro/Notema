use crate::{
    AppResult, JournalStorePaths,
    library::{
        CachePolicy, CacheRead, CacheStatus, CachedLibrary, CachedRecord, FileStamp,
        LibraryDiscovery, LibraryLoadReport, LibrarySnapshot, MissCause, path_for_record,
    },
    storage,
};
use notema_domain::Entry;
use notema_encryption as crypto;
use notema_timing as timing;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

/// Version 2 drops `ctime` from the stamp: length plus mtime on every platform.
const CACHE_WIRE_VERSION: u32 = 2;
const PLAIN_CACHE_FILE: &str = "library-cache.msgpack";
const ENCRYPTED_CACHE_FILE: &str = "library-cache.msgpack.age";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
enum CacheSecurity {
    Plaintext,
    Encrypted { recipients_sha256: String },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CacheFile {
    wire_version: u32,
    app_version: String,
    store_id: crate::StoreId,
    journal_root: PathBuf,
    security: CacheSecurity,
    journals: Vec<crate::Journal>,
    records: Vec<CacheRecordFile>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CacheRecordFile {
    stamp: FileStamp,
    /// Whether `stamp` was taken far enough past the file's own mtime to rule
    /// out a further write hiding inside the filesystem's granularity. Recorded
    /// because trust does not accrue with age: re-judging a saved stamp against
    /// a later launch would call every racy observation trustworthy, which is
    /// exactly the one it must not be.
    trusted: bool,
    entry: Entry,
}

/// Just enough of a cache file to tell one written by a different build apart
/// from a damaged one. Deliberately lenient: the fields it skips are exactly
/// the ones that failed to decode.
///
/// `app_version` matters as much as the wire version: `Entry` is embedded whole
/// under `deny_unknown_fields`, so any release that changes its shape makes an
/// old cache undecodable without the wire version moving.
#[derive(Deserialize)]
struct CacheVersion {
    wire_version: u32,
    /// Optional so a future wire version that drops or renames it still reads as
    /// an upgrade rather than as damage.
    #[serde(default)]
    app_version: Option<String>,
}

pub(super) fn read(
    paths: &JournalStorePaths,
    identity: Option<&crypto::UnlockedIdentity>,
    policy: CachePolicy,
) -> AppResult<CacheRead> {
    let started = Instant::now();
    let mut report = LibraryLoadReport::default();
    if policy == CachePolicy::Off {
        report.cache_status = CacheStatus::Disabled;
        return Ok(CacheRead {
            cached: None,
            report,
        });
    }

    let encrypted = paths.keys.has_roster();
    if encrypted && identity.is_none() {
        report.cache_status = CacheStatus::Locked;
        return Ok(CacheRead {
            cached: None,
            report,
        });
    }
    let expected_security = security(paths)?;
    let path = cache_path(paths, encrypted);
    let bytes = match read_bytes(&path, identity, encrypted) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            report.cache_status = CacheStatus::Missing;
            report.cache_read = started.elapsed();
            return Ok(CacheRead {
                cached: None,
                report,
            });
        }
        Err(error) => {
            report.cache_status = CacheStatus::Corrupt;
            report.cache_warning = Some(format!("cache read failed: {error:#}"));
            report.cache_read = started.elapsed();
            return Ok(CacheRead {
                cached: None,
                report,
            });
        }
    };
    let cache: CacheFile = match rmp_serde::from_slice(bytes.as_ref()) {
        Ok(cache) => cache,
        // A cache this binary cannot decode is only damage if it isn't simply
        // from a wire version that shaped it differently. Probed second, not
        // first: rmp-serde walks skipped fields rather than stepping over them,
        // so a version probe ahead of the decode would traverse every record
        // twice on the launches that succeed.
        Err(error) => {
            report.cache_status = match rmp_serde::from_slice::<CacheVersion>(bytes.as_ref()) {
                Ok(header)
                    if header.wire_version != CACHE_WIRE_VERSION
                        || header.app_version.as_deref() != Some(env!("CARGO_PKG_VERSION")) =>
                {
                    CacheStatus::Incompatible
                }
                _ => {
                    report.cache_warning = Some(format!("cache decode failed: {error}"));
                    CacheStatus::Corrupt
                }
            };
            report.cache_read = started.elapsed();
            return Ok(CacheRead {
                cached: None,
                report,
            });
        }
    };
    let canonical_root = fs::canonicalize(&paths.journal_root)?;
    let store_id = crate::store_id::read(&paths.journal_root)?;
    let compatible = cache.wire_version == CACHE_WIRE_VERSION
        && cache.app_version == env!("CARGO_PKG_VERSION")
        && Some(cache.store_id.clone()) == store_id
        && cache.journal_root == canonical_root
        && cache.security == expected_security;
    if !compatible {
        report.cache_status = CacheStatus::Incompatible;
        report.cache_read = started.elapsed();
        return Ok(CacheRead {
            cached: None,
            report,
        });
    }

    report.cache_read = started.elapsed();
    report.cache_status = CacheStatus::Hit;
    report.entries = cache.records.len();
    report.cache_hits = cache.records.len();
    Ok(CacheRead {
        cached: Some(CachedLibrary {
            journals: cache.journals,
            records: cache
                .records
                .into_iter()
                .map(|record| CachedRecord {
                    stamp: record.stamp,
                    trusted: record.trusted,
                    entry: record.entry,
                })
                .collect(),
            warning: report.cache_warning.clone(),
        }),
        report,
    })
}

pub(super) fn validate(
    paths: &JournalStorePaths,
    identity: Option<&crypto::UnlockedIdentity>,
    cached: Option<CachedLibrary>,
    policy: CachePolicy,
    progress: Option<&(dyn Fn(crate::LibraryLoadProgress) + Sync)>,
) -> AppResult<LibrarySnapshot> {
    let discovery = discover(paths, progress)?;
    validate_discovery(paths, identity, cached, policy, discovery, progress)
}

pub(super) fn discover(
    paths: &JournalStorePaths,
    progress: Option<&(dyn Fn(crate::LibraryLoadProgress) + Sync)>,
) -> AppResult<LibraryDiscovery> {
    let discovery_started = Instant::now();
    if let Some(progress) = progress {
        progress(crate::LibraryLoadProgress::Discovering { entries_found: 0 });
    }
    let journals = storage::discover_journals(&paths.journal_root)?;
    let report_discovery = |entries_found| {
        if let Some(progress) = progress {
            progress(crate::LibraryLoadProgress::Discovering { entries_found });
        }
    };
    let entries = storage::collect_discovered_entries_with_progress(
        &journals,
        progress.map(|_| &report_discovery as &(dyn Fn(usize) + Sync)),
    )?;
    Ok(LibraryDiscovery {
        journals,
        entries,
        elapsed: discovery_started.elapsed(),
    })
}

pub(super) fn validate_discovery(
    paths: &JournalStorePaths,
    identity: Option<&crypto::UnlockedIdentity>,
    cached: Option<CachedLibrary>,
    policy: CachePolicy,
    discovery: LibraryDiscovery,
    progress: Option<&(dyn Fn(crate::LibraryLoadProgress) + Sync)>,
) -> AppResult<LibrarySnapshot> {
    let validation_started = Instant::now();
    let LibraryDiscovery {
        mut journals,
        entries: discovered,
        elapsed: discovery,
    } = discovery;
    storage::initialize_journals(&mut journals);

    let had_cache = cached.is_some();
    let journals_changed = cached
        .as_ref()
        .is_none_or(|cache| cache.journals != journals);
    let cache_warning = cached.as_ref().and_then(|cache| cache.warning.clone());
    let mut records: HashMap<PathBuf, CachedRecord> = cached
        .into_iter()
        .flat_map(|cache| cache.records)
        .map(|record| (path_for_record(&record), record))
        .collect();
    let discovered_total = discovered.len();
    let mut stamps = HashMap::with_capacity(discovered_total);
    let mut entries = Vec::with_capacity(discovered_total);
    let mut misses = Vec::new();
    let mut causes = MissCauses::default();
    let mut subsecond_mtimes = 0usize;
    for discovered in discovered {
        subsecond_mtimes += usize::from(discovered.stamp.has_subsecond_mtime());
        stamps.insert(
            discovered.source.path.clone(),
            (discovered.stamp, discovered.stamp_trusted),
        );
        let record = records.remove(&discovered.source.path);
        let cause = discovered
            .miss_cause(record.as_ref())
            .or_else(|| (policy != CachePolicy::Normal).then_some(MissCause::Rebuild));
        match (cause, record) {
            (None, Some(record)) => entries.push(record.entry),
            (Some(cause), _) => {
                causes.record(cause);
                misses.push(discovered.source);
            }
            // `miss_cause` answers `Absent` when there is no record, so a hit
            // without one cannot arise.
            (None, None) => unreachable!("a cache hit needs a record"),
        }
    }
    // Without an mtime the stamp is only a length, which is never enough to
    // trust. Such a store re-reads and rewrites everything on every launch, so
    // say so rather than let it look like ordinary slowness.
    let cache_warning = cache_warning.or_else(|| {
        (discovered_total > 0 && causes.count(MissCause::Unstamped) == discovered_total).then(
            || {
                "this filesystem reports no modification times, so the entry cache cannot be used"
                    .to_owned()
            },
        )
    });
    if timing::detailed() {
        timing::note(&format!(
            "cache mtime precision: {subsecond_mtimes}/{discovered_total} stamps carry sub-second mtime"
        ));
        if let Some(summary) = causes.summary() {
            timing::note(&format!("cache misses by cause: {summary}"));
        }
    }
    let cache_hits = entries.len();
    let cache_misses = misses.len();
    let removed_records = records.len();
    if let Some(progress) = progress {
        progress(crate::LibraryLoadProgress::Reading {
            current: cache_hits,
            total: cache_hits + cache_misses,
        });
    }

    let source_started = Instant::now();
    let report_miss = |current, total| {
        if let Some(progress) = progress {
            progress(crate::LibraryLoadProgress::Reading {
                current: cache_hits + current,
                total: cache_hits + total,
            });
        }
    };
    let (source_entries, source_failures) = storage::read_entries_with_progress(
        misses,
        identity,
        progress.map(|_| &report_miss as &(dyn Fn(usize, usize) + Sync)),
    )?;
    entries.extend(source_entries);
    entries.sort_by(|left, right| right.path.cmp(&left.path));
    let source_read = source_started.elapsed();

    let mut report = LibraryLoadReport {
        discovery,
        source_read,
        entries: entries.len(),
        cache_hits,
        cache_misses,
        removed_records,
        cache_status: if had_cache && !journals_changed && cache_misses == 0 && removed_records == 0
        {
            CacheStatus::Hit
        } else {
            CacheStatus::Rebuilt
        },
        cache_warning,
        source_failures,
        ..LibraryLoadReport::default()
    };

    let encrypted = paths.keys.has_roster();
    let should_save = policy != CachePolicy::Off
        && !(encrypted && identity.is_none())
        && (policy == CachePolicy::Rebuild
            || !had_cache
            || journals_changed
            || cache_misses > 0
            || removed_records > 0);
    if should_save {
        let write_started = Instant::now();
        if let Err(error) = save(paths, encrypted, &journals, &entries, &stamps) {
            report.cache_warning = Some(format!("cache save failed: {error:#}"));
        }
        report.cache_write = write_started.elapsed();
    }
    report.total = discovery.saturating_add(validation_started.elapsed());
    Ok(LibrarySnapshot {
        journals,
        entries,
        report,
    })
}

/// Miss causes tallied over one validation run. Counted unconditionally — the
/// cause falls out of the hit decision the loop makes anyway.
#[derive(Default)]
struct MissCauses {
    counts: Vec<(MissCause, usize)>,
}

impl MissCauses {
    fn record(&mut self, cause: MissCause) {
        match self.counts.iter_mut().find(|(seen, _)| *seen == cause) {
            Some((_, count)) => *count += 1,
            None => self.counts.push((cause, 1)),
        }
    }

    fn count(&self, cause: MissCause) -> usize {
        self.counts
            .iter()
            .find(|(seen, _)| *seen == cause)
            .map_or(0, |(_, count)| *count)
    }

    /// `None` when nothing missed.
    fn summary(&self) -> Option<String> {
        if self.counts.is_empty() {
            return None;
        }
        let mut counts = self.counts.clone();
        counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        Some(
            counts
                .iter()
                .map(|(cause, count)| format!("{}={count}", cause.name()))
                .collect::<Vec<_>>()
                .join(" "),
        )
    }
}

fn read_bytes(
    path: &Path,
    identity: Option<&crypto::UnlockedIdentity>,
    encrypted: bool,
) -> AppResult<Option<CacheBytes>> {
    if encrypted {
        if !path.exists() {
            return Ok(None);
        }
        return Ok(Some(CacheBytes::Secret(crypto::decrypt_file_bytes(
            identity.context("encrypted cache requires an unlocked identity")?,
            path,
        )?)));
    }
    match fs::read(path) {
        Ok(bytes) => Ok(Some(CacheBytes::Plain(bytes))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

enum CacheBytes {
    Plain(Vec<u8>),
    Secret(crypto::PlaintextBytes),
}

impl AsRef<[u8]> for CacheBytes {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Plain(bytes) => bytes,
            Self::Secret(bytes) => bytes.as_bytes(),
        }
    }
}

fn save(
    paths: &JournalStorePaths,
    encrypted: bool,
    journals: &[crate::Journal],
    entries: &[Entry],
    stamps: &HashMap<PathBuf, (FileStamp, bool)>,
) -> AppResult<()> {
    let records = entries
        .iter()
        .filter_map(|entry| {
            let (stamp, trusted) = *stamps.get(&entry.path)?;
            Some(CacheRecordFile {
                stamp,
                trusted,
                entry: entry.clone(),
            })
        })
        .collect();
    let store_id =
        crate::store_id::read(&paths.journal_root)?.context("journal store marker is missing")?;
    let cache = CacheFile {
        wire_version: CACHE_WIRE_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        store_id,
        journal_root: fs::canonicalize(&paths.journal_root)?,
        security: security(paths)?,
        journals: journals.to_vec(),
        records,
    };
    let serialized = rmp_serde::to_vec_named(&cache)?;
    if encrypted {
        let plaintext = crypto::PlaintextBytes::from_vec(serialized);
        let ciphertext = crypto::encrypt_bytes(&paths.keys, &plaintext)?;
        crypto::atomic_write_private(&encrypted_path(paths), ciphertext.as_bytes())?;
    } else {
        crypto::atomic_write_private(&plain_path(paths), &serialized)?;
    }
    Ok(())
}

fn security(paths: &JournalStorePaths) -> AppResult<CacheSecurity> {
    if !paths.keys.has_roster() {
        return Ok(CacheSecurity::Plaintext);
    }
    let mut keys: Vec<String> = crypto::read_recipients(&paths.keys)?
        .into_iter()
        .map(|recipient| recipient.encryption_key)
        .collect();
    keys.sort();
    let mut digest = Sha256::new();
    for key in keys {
        let length = u32::try_from(key.len())?;
        digest.update(length.to_le_bytes());
        digest.update(key.as_bytes());
    }
    Ok(CacheSecurity::Encrypted {
        recipients_sha256: hex::encode(digest.finalize()),
    })
}

pub(super) fn remove_incompatible(paths: &JournalStorePaths, encrypted: bool) -> AppResult<()> {
    if encrypted {
        remove_if_exists(&plain_path(paths))?;
    } else {
        remove_if_exists(&encrypted_path(paths))?;
    }
    Ok(())
}

pub(super) fn invalidate(paths: &JournalStorePaths) -> AppResult<()> {
    for path in [plain_path(paths), encrypted_path(paths)] {
        remove_if_exists(&path)?;
    }
    Ok(())
}

fn remove_if_exists(path: &Path) -> AppResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn cache_path(paths: &JournalStorePaths, encrypted: bool) -> PathBuf {
    if encrypted {
        encrypted_path(paths)
    } else {
        plain_path(paths)
    }
}

fn plain_path(paths: &JournalStorePaths) -> PathBuf {
    paths.config_dir.join(PLAIN_CACHE_FILE)
}

fn encrypted_path(paths: &JournalStorePaths) -> PathBuf {
    paths.config_dir.join(ENCRYPTED_CACHE_FILE)
}

use anyhow::Context;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntryAssetOptions, EntryDraft, JournalStore};
    use notema_domain::Metadata;
    use tempfile::tempdir;

    fn set_mtime(path: &Path, when: std::time::SystemTime) {
        let times = fs::FileTimes::new().set_modified(when);
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(times)
            .unwrap();
    }

    fn seconds_ago(seconds: u64) -> std::time::SystemTime {
        std::time::SystemTime::now() - std::time::Duration::from_secs(seconds)
    }

    /// Push an entry's mtime into the past so its stamp is outside the coarse
    /// window whatever the test machine resolves mtime to. Without it, a test
    /// that writes a file and then expects a cache hit is a coin flip on a
    /// one-second-granularity filesystem.
    fn settle(path: &Path) {
        set_mtime(path, seconds_ago(60));
    }

    fn settle_all(store: &JournalStore) {
        for entry in store.scan_entries().unwrap() {
            settle(&entry.path);
        }
    }

    fn store_with_entries(root: &Path, config: &Path, bodies: &[&str]) -> JournalStore {
        let store = JournalStore::new(root, config);
        store.ensure().unwrap();
        store.create_journal("daily").unwrap();
        for body in bodies {
            store
                .create_entry(
                    EntryDraft::new("daily", body, &Metadata::default()),
                    EntryAssetOptions::default(),
                )
                .unwrap();
        }
        store
    }

    #[test]
    fn miss_causes_are_summarized_by_descending_count() {
        let mut causes = MissCauses::default();
        assert_eq!(causes.summary(), None);

        causes.record(MissCause::Absent);
        for _ in 0..3 {
            causes.record(MissCause::Mtime);
        }
        causes.record(MissCause::Len);
        causes.record(MissCause::Len);

        assert_eq!(causes.summary().as_deref(), Some("mtime=3 len=2 absent=1"));
    }

    /// The hit and miss counts only exist after validation. Reporting the cache
    /// read's own counts instead would always say "0 miss".
    #[test]
    fn the_validated_report_carries_real_hit_and_miss_counts() {
        let dir = tempdir().unwrap();
        let store = store_with_entries(
            &dir.path().join("journals"),
            &dir.path().join("config"),
            &["first", "second"],
        );
        settle_all(&store);
        store.load_library(CachePolicy::Normal).unwrap();
        let cached = store.read_cached_library(CachePolicy::Normal).unwrap();
        assert!(cached.report.cache_read_summary().contains("2 records"));

        let changed = store.scan_entries().unwrap()[0].path.clone();
        fs::write(&changed, "a body of an entirely different length\n").unwrap();

        let validated = store
            .validate_library(cached.cached, CachePolicy::Normal)
            .unwrap();
        assert!(
            validated.report.timing_summary().contains("1 hit / 1 miss"),
            "{}",
            validated.report.timing_summary()
        );
    }

    /// A source read that hits one unreadable entry degrades it to a placeholder
    /// and records the failure in the report rather than failing the whole load.
    #[test]
    fn the_report_carries_unreadable_source_failures() {
        let dir = tempdir().unwrap();
        let store = store_with_entries(
            &dir.path().join("journals"),
            &dir.path().join("config"),
            &["first", "second"],
        );
        let corrupt = store.scan_entries().unwrap()[0].path.clone();
        fs::write(&corrupt, [0xff, 0xfe, 0x00, 0xff]).unwrap();

        let snapshot = store.load_library(CachePolicy::Rebuild).unwrap();

        assert_eq!(snapshot.entries.len(), 2, "the readable entry still loads");
        assert_eq!(snapshot.report.source_failures.len(), 1);
        assert!(
            snapshot.report.timing_summary().contains("1 unreadable"),
            "{}",
            snapshot.report.timing_summary()
        );
    }

    /// chmod bumps `ctime` and nothing else, and so do Spotlight, Time Machine
    /// and every sync client.
    #[cfg(unix)]
    #[test]
    fn a_permission_change_alone_is_a_cache_hit() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let store = store_with_entries(
            &dir.path().join("journals"),
            &dir.path().join("config"),
            &["body"],
        );
        settle_all(&store);
        let path = store.load_library(CachePolicy::Normal).unwrap().entries[0]
            .path
            .clone();
        let cached = store
            .read_cached_library(CachePolicy::Normal)
            .unwrap()
            .cached;
        fs::set_permissions(&path, PermissionsExt::from_mode(0o600)).unwrap();

        let validated = store.validate_library(cached, CachePolicy::Normal).unwrap();
        assert_eq!(validated.report.cache_hits, 1);
        assert_eq!(validated.report.cache_misses, 0);
        assert_eq!(validated.report.cache_status, CacheStatus::Hit);
    }

    /// What length plus mtime catches: a rewrite that moves the mtime.
    #[test]
    fn a_same_length_rewrite_with_a_new_mtime_is_a_miss() {
        let dir = tempdir().unwrap();
        let store = store_with_entries(
            &dir.path().join("journals"),
            &dir.path().join("config"),
            &["first"],
        );
        settle_all(&store);
        let path = store.load_library(CachePolicy::Normal).unwrap().entries[0]
            .path
            .clone();
        let cached = store
            .read_cached_library(CachePolicy::Normal)
            .unwrap()
            .cached;

        let before = fs::metadata(&path).unwrap().len();
        let rewritten = fs::read_to_string(&path).unwrap().replace("first", "FIRST");
        fs::write(&path, rewritten).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().len(),
            before,
            "the rewrite has to keep the length for this test to test anything"
        );

        let validated = store.validate_library(cached, CachePolicy::Normal).unwrap();
        assert_eq!(validated.report.cache_misses, 1);
        assert_eq!(validated.report.cache_hits, 0);
        assert!(validated.entries[0].body.contains("FIRST"));
    }

    /// A restore writes the backup's own modification time, which differs from
    /// the one recorded for the newer content it replaces — so the stamp does
    /// not match and the entry is re-read. Ordering is not what saves this;
    /// inequality is.
    #[test]
    fn restoring_an_older_entry_over_a_newer_one_is_a_miss() {
        let dir = tempdir().unwrap();
        let store = store_with_entries(
            &dir.path().join("journals"),
            &dir.path().join("config"),
            &["aaaaa"],
        );
        let path = store.scan_entries().unwrap()[0].path.clone();
        let backup = fs::read_to_string(&path).unwrap();
        let backup_mtime = seconds_ago(3600);
        set_mtime(&path, backup_mtime);

        // The newer content that gets cached: same length, later mtime.
        fs::write(&path, backup.replace("aaaaa", "bbbbb")).unwrap();
        set_mtime(&path, seconds_ago(60));
        store.load_library(CachePolicy::Normal).unwrap();
        let cached = store
            .read_cached_library(CachePolicy::Normal)
            .unwrap()
            .cached;

        // What `cp -p`, `tar -x` or a restic restore does.
        fs::write(&path, &backup).unwrap();
        set_mtime(&path, backup_mtime);

        let validated = store.validate_library(cached, CachePolicy::Normal).unwrap();
        assert_eq!(validated.report.cache_misses, 1);
        assert_eq!(validated.entries[0].body, "aaaaa\n");
    }

    /// The hole the trade leaves, written down in `docs/STORAGE-FORMAT.md`: a
    /// same-length rewrite that puts the recorded mtime back is
    /// indistinguishable from no change at all.
    #[test]
    fn a_same_length_rewrite_that_restores_the_recorded_mtime_is_missed() {
        let dir = tempdir().unwrap();
        let store = store_with_entries(
            &dir.path().join("journals"),
            &dir.path().join("config"),
            &["aaaaa"],
        );
        settle_all(&store);
        let path = store.load_library(CachePolicy::Normal).unwrap().entries[0]
            .path
            .clone();
        let recorded_mtime = fs::metadata(&path).unwrap().modified().unwrap();
        let cached = store
            .read_cached_library(CachePolicy::Normal)
            .unwrap()
            .cached;

        let rewritten = fs::read_to_string(&path).unwrap().replace("aaaaa", "bbbbb");
        fs::write(&path, rewritten).unwrap();
        set_mtime(&path, recorded_mtime);

        let validated = store.validate_library(cached, CachePolicy::Normal).unwrap();
        assert_eq!(validated.report.cache_hits, 1);
        assert_eq!(
            validated.entries[0].body, "aaaaa\n",
            "the stale cached body is served; the file on disk says bbbbb"
        );
    }

    /// An untrusted stamp is refused even when it matches exactly. Driven off
    /// the flag rather than a real coarse filesystem, which the test machine is
    /// unlikely to have.
    #[test]
    fn an_untrusted_stamp_is_not_reused_even_when_it_matches() {
        let dir = tempdir().unwrap();
        let store = store_with_entries(
            &dir.path().join("journals"),
            &dir.path().join("config"),
            &["body"],
        );
        settle_all(&store);
        store.load_library(CachePolicy::Normal).unwrap();
        let cached = store
            .read_cached_library(CachePolicy::Normal)
            .unwrap()
            .cached;

        let mut discovery = store.discover_library_with_progress(&|_| {}).unwrap();
        assert!(discovery.entries[0].stamp_trusted);
        discovery.entries[0].stamp_trusted = false;

        let validated = store
            .validate_discovered_library(cached, CachePolicy::Normal, discovery)
            .unwrap();
        assert_eq!(validated.report.cache_hits, 0);
        assert_eq!(validated.report.cache_misses, 1);
        assert_eq!(validated.entries[0].body, "body\n");
    }

    /// A record written from an untrusted observation stays untrusted however
    /// long it sits in the cache. Trust is a property of the moment the stamp
    /// was taken, and re-deciding it against a later launch would hand every
    /// racy record a hit purely for having aged — which is precisely the write
    /// the racy rule exists to catch.
    #[test]
    fn a_racy_observation_is_still_refused_on_the_next_launch() {
        let dir = tempdir().unwrap();
        let store = store_with_entries(
            &dir.path().join("journals"),
            &dir.path().join("config"),
            &["body"],
        );
        settle_all(&store);

        // Save a cache from a walk that could not trust what it saw.
        let cached = store
            .read_cached_library(CachePolicy::Normal)
            .unwrap()
            .cached;
        let mut discovery = store.discover_library_with_progress(&|_| {}).unwrap();
        discovery.entries[0].stamp_trusted = false;
        store
            .validate_discovered_library(cached, CachePolicy::Normal, discovery)
            .unwrap();

        // A later launch sees the same file, and now trusts its own observation.
        let cached = store
            .read_cached_library(CachePolicy::Normal)
            .unwrap()
            .cached;
        let discovery = store.discover_library_with_progress(&|_| {}).unwrap();
        assert!(discovery.entries[0].stamp_trusted);

        let validated = store
            .validate_discovered_library(cached, CachePolicy::Normal, discovery)
            .unwrap();
        assert_eq!(
            validated.report.cache_hits, 0,
            "the recorded observation was racy, so the bytes it describes are unproven"
        );
        assert_eq!(validated.report.cache_misses, 1);
    }

    #[test]
    fn cached_snapshot_is_available_before_source_validation() {
        let dir = tempdir().unwrap();
        let store = store_with_entries(
            &dir.path().join("journals"),
            &dir.path().join("config"),
            &["first", "second"],
        );
        settle_all(&store);
        let first = store.load_library(CachePolicy::Normal).unwrap();
        assert_eq!(first.report.cache_misses, 2);

        let cached = store.read_cached_library(CachePolicy::Normal).unwrap();
        let snapshot = cached.cached.as_ref().unwrap().snapshot();
        assert_eq!(snapshot.entries.len(), 2);

        let validated = store
            .validate_library(cached.cached, CachePolicy::Normal)
            .unwrap();
        assert_eq!(validated.report.cache_hits, 2);
        assert_eq!(validated.report.cache_misses, 0);
    }

    #[test]
    fn rebuild_reports_entry_progress() {
        let dir = tempdir().unwrap();
        let store = store_with_entries(
            &dir.path().join("journals"),
            &dir.path().join("config"),
            &["first", "second"],
        );
        let updates = std::sync::Mutex::new(Vec::new());

        store
            .load_library_with_progress(CachePolicy::Rebuild, &|update| {
                updates.lock().unwrap().push(update);
            })
            .unwrap();

        let updates = updates.into_inner().unwrap();
        assert_eq!(
            updates.first(),
            Some(&crate::LibraryLoadProgress::Discovering { entries_found: 0 })
        );
        assert!(updates.contains(&crate::LibraryLoadProgress::Discovering { entries_found: 2 }));
        let reading = updates.iter().filter_map(|update| match update {
            crate::LibraryLoadProgress::Reading { current, total } => Some((*current, *total)),
            crate::LibraryLoadProgress::Discovering { .. } => None,
        });
        let reading = reading.collect::<Vec<_>>();
        assert!(reading.iter().all(|(_, total)| *total == 2));
        assert_eq!(reading.iter().map(|(current, _)| *current).max(), Some(2));
    }

    #[test]
    fn setup_discovery_is_read_only_and_reused_for_rebuild() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("journals");
        let journal = root.join("daily");
        let store = JournalStore::new(&root, dir.path().join("config"));
        store.ensure().unwrap();
        fs::create_dir_all(&journal).unwrap();
        fs::write(journal.join("entry.md"), "# First\n").unwrap();
        let sidecar = journal.join(".journal.toml");

        let discovery = store.discover_library_with_progress(&|_| {}).unwrap();
        assert_eq!(discovery.entry_count(), 1);
        assert_eq!(discovery.journal_names().collect::<Vec<_>>(), ["daily"]);
        assert!(!sidecar.exists());

        let snapshot = store
            .load_discovered_library_with_progress(CachePolicy::Rebuild, discovery, &|_| {})
            .unwrap();
        assert_eq!(snapshot.entries.len(), 1);
        assert!(sidecar.exists());
    }

    #[test]
    fn validation_persists_journal_only_changes() {
        let dir = tempdir().unwrap();
        let store = store_with_entries(
            &dir.path().join("journals"),
            &dir.path().join("config"),
            &[],
        );
        store.load_library(CachePolicy::Normal).unwrap();
        let cached = store
            .read_cached_library(CachePolicy::Normal)
            .unwrap()
            .cached;
        store.create_journal("second").unwrap();

        let validated = store.validate_library(cached, CachePolicy::Normal).unwrap();
        assert_eq!(validated.journals.len(), 2);

        let persisted = store.read_cached_library(CachePolicy::Normal).unwrap();
        assert_eq!(persisted.cached.unwrap().snapshot().journals.len(), 2);
    }

    #[test]
    fn validation_reloads_changed_entries_and_drops_deleted_entries() {
        let dir = tempdir().unwrap();
        let store = store_with_entries(
            &dir.path().join("journals"),
            &dir.path().join("config"),
            &["first", "second"],
        );
        settle_all(&store);
        let initial = store.load_library(CachePolicy::Normal).unwrap();
        let changed_path = initial.entries[0].path.clone();
        let deleted_path = initial.entries[1].path.clone();
        let cached = store
            .read_cached_library(CachePolicy::Normal)
            .unwrap()
            .cached;
        fs::write(&changed_path, "changed body with a different length\n").unwrap();
        fs::remove_file(&deleted_path).unwrap();

        let validated = store.validate_library(cached, CachePolicy::Normal).unwrap();

        assert_eq!(validated.entries.len(), 1);
        assert_eq!(
            validated.entries[0].body,
            "changed body with a different length\n"
        );
        assert_eq!(validated.report.cache_hits, 0);
        assert_eq!(validated.report.cache_misses, 1);
        assert_eq!(validated.report.removed_records, 1);
    }

    #[test]
    fn app_version_or_store_mismatch_is_not_trusted() {
        let dir = tempdir().unwrap();
        let store = store_with_entries(
            &dir.path().join("journals"),
            &dir.path().join("config"),
            &["body"],
        );
        store.load_library(CachePolicy::Normal).unwrap();
        let path = plain_path(store.paths());
        let mut cache: CacheFile = rmp_serde::from_slice(&fs::read(&path).unwrap()).unwrap();
        cache.app_version = "other".to_owned();
        fs::write(&path, rmp_serde::to_vec_named(&cache).unwrap()).unwrap();

        let read = store.read_cached_library(CachePolicy::Normal).unwrap();
        assert!(read.cached.is_none());
        assert_eq!(read.report.cache_status, CacheStatus::Incompatible);
    }

    /// A wire version that reshapes the file enough that it no longer decodes.
    /// The version is still readable, so this is an upgrade, not damage — and
    /// an upgrade must rebuild quietly, with no warning shown to the user.
    #[test]
    fn an_undecodable_cache_with_a_readable_version_is_incompatible() {
        #[derive(Serialize)]
        struct FutureShape {
            wire_version: u32,
            unrecognizable: bool,
        }

        let dir = tempdir().unwrap();
        let store = store_with_entries(
            &dir.path().join("journals"),
            &dir.path().join("config"),
            &["body"],
        );
        store.load_library(CachePolicy::Normal).unwrap();
        let future = rmp_serde::to_vec_named(&FutureShape {
            wire_version: CACHE_WIRE_VERSION + 1,
            unrecognizable: true,
        })
        .unwrap();
        fs::write(plain_path(store.paths()), future).unwrap();

        let read = store.read_cached_library(CachePolicy::Normal).unwrap();
        assert!(read.cached.is_none());
        assert_eq!(read.report.cache_status, CacheStatus::Incompatible);
        assert_eq!(read.report.cache_warning, None);
    }

    /// The version probe must not swallow real damage.
    #[test]
    fn a_cache_without_a_readable_version_is_corrupt() {
        let dir = tempdir().unwrap();
        let store = store_with_entries(
            &dir.path().join("journals"),
            &dir.path().join("config"),
            &["body"],
        );
        store.load_library(CachePolicy::Normal).unwrap();
        fs::write(plain_path(store.paths()), b"not msgpack at all").unwrap();

        let read = store.read_cached_library(CachePolicy::Normal).unwrap();
        assert!(read.cached.is_none());
        assert_eq!(read.report.cache_status, CacheStatus::Corrupt);
        assert!(read.report.cache_warning.is_some());
    }

    #[test]
    fn binary_cache_round_trips_non_finite_source_values() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("journals");
        let store = store_with_entries(&root, &dir.path().join("config"), &[]);
        let path = root.join("daily/2026-07-13-nonfinite.md");
        fs::write(
            &path,
            "+++\nschema_version = 1\n[weather]\ntemperature_celsius = nan\n+++\n\nBody\n",
        )
        .unwrap();

        store.load_library(CachePolicy::Normal).unwrap();
        let cached = store.read_cached_library(CachePolicy::Normal).unwrap();
        let temperature = cached.cached.unwrap().snapshot().entries[0]
            .weather
            .as_ref()
            .unwrap()
            .temperature_celsius
            .unwrap();
        assert!(temperature.is_nan());
    }

    #[cfg(unix)]
    #[test]
    fn plaintext_cache_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let store = store_with_entries(
            &dir.path().join("journals"),
            &dir.path().join("config"),
            &["body"],
        );
        store.load_library(CachePolicy::Normal).unwrap();
        let mode = fs::metadata(plain_path(store.paths()))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
