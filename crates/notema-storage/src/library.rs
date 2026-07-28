use crate::{Entry, Journal};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::Metadata,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// How a library load may use or update the local derived cache.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CachePolicy {
    Off,
    #[default]
    Normal,
    Rebuild,
}

/// Progress while reconciling the on-disk journal tree with the local cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryLoadProgress {
    Discovering { entries_found: usize },
    Reading { current: usize, total: usize },
}

/// A complete, internally consistent journal-library view.
#[derive(Debug, Clone, PartialEq)]
pub struct LibrarySnapshot {
    pub journals: Vec<Journal>,
    pub entries: Vec<Entry>,
    pub report: LibraryLoadReport,
}

/// A read-only inventory of the source tree that can be validated after the
/// caller has accepted the selected folder.
pub struct LibraryDiscovery {
    pub(crate) journals: Vec<Journal>,
    pub(crate) entries: Vec<DiscoveredEntry>,
    pub(crate) elapsed: Duration,
}

impl LibraryDiscovery {
    pub fn journal_names(&self) -> impl Iterator<Item = &str> {
        self.journals.iter().map(|journal| journal.name.as_str())
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

/// Privacy-safe diagnostics for cache and source loading.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryLoadReport {
    pub total: Duration,
    pub discovery: Duration,
    pub cache_read: Duration,
    pub source_read: Duration,
    pub cache_write: Duration,
    pub entries: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub removed_records: usize,
    pub cache_status: CacheStatus,
    pub cache_warning: Option<String>,
}

impl LibraryLoadReport {
    /// One line for the cache decode alone. The hit and miss counts are not
    /// known yet at that point — validation has not run — so they are left out
    /// rather than reported as zero.
    pub fn cache_read_summary(&self) -> String {
        format!(
            "cache read: {:?}, {} records in {:.1} ms",
            self.cache_status,
            self.entries,
            self.cache_read.as_secs_f64() * 1000.0
        )
    }

    /// One-line breakdown for `NOTEMA_TIMING`.
    pub fn timing_summary(&self) -> String {
        let ms = |duration: Duration| duration.as_secs_f64() * 1000.0;
        format!(
            "library: {:?} {} entries ({} hit / {} miss), total {:.1} ms; components: cache-read {:.1}, discovery {:.1}, source-read {:.1}, cache-write {:.1}",
            self.cache_status,
            self.entries,
            self.cache_hits,
            self.cache_misses,
            ms(self.total),
            ms(self.cache_read),
            ms(self.discovery),
            ms(self.source_read),
            ms(self.cache_write),
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CacheStatus {
    Hit,
    #[default]
    Missing,
    Disabled,
    Locked,
    Corrupt,
    Incompatible,
    Rebuilt,
}

/// The coarsest mtime granularity in common use: FAT and exFAT resolve to 2
/// seconds, SMB and HFS+ to 1.
const COARSE_MTIME_WINDOW: Duration = Duration::from_secs(2);

/// Metadata-only fingerprint for entry-cache validity. Cheap enough to take for
/// every file in a scan, and correspondingly weaker than [`EntryRevision`] —
/// never use it to guard a write.
///
/// Blind to a same-length rewrite that also puts back the exact mtime recorded
/// here. A restore does not do that — it writes the backup's own mtime, which
/// differs — so this needs a `touch -r` or equivalent. Deliberate trade; see
/// `docs/STORAGE-FORMAT.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FileStamp {
    len: u64,
    modified: Option<(u64, u32)>,
}

impl FileStamp {
    pub(crate) fn from_metadata(metadata: &Metadata) -> Self {
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| (duration.as_secs(), duration.subsec_nanos()));
        Self {
            len: metadata.len(),
            modified,
        }
    }

    /// Whether a stamp taken at `observed_at` can rule out a later same-length
    /// write going unnoticed. A non-zero nanosecond field proves the filesystem
    /// resolves finer than a second, so nothing can hide. A whole-second mtime
    /// only becomes trustworthy once it is old enough that any subsequent write
    /// must land in a strictly later tick. No mtime at all leaves length as the
    /// sole discriminator, which is never enough.
    ///
    /// An mtime that lands exactly on a whole second on a nanosecond-resolution
    /// filesystem reads as coarse and costs one needless miss. Leave it — the
    /// alternative is trusting a stamp we cannot prove.
    pub(crate) fn is_trustworthy_at(&self, observed_at: SystemTime) -> bool {
        let Some((seconds, nanos)) = self.modified else {
            return false;
        };
        if nanos != 0 {
            return true;
        }
        let modified = UNIX_EPOCH + Duration::from_secs(seconds);
        // An mtime in the future (clock skew, or one set by hand) is not old.
        observed_at
            .duration_since(modified)
            .is_ok_and(|age| age >= COARSE_MTIME_WINDOW)
    }

    /// Whether the filesystem resolved this mtime finer than a whole second.
    pub(crate) fn has_subsecond_mtime(&self) -> bool {
        self.modified.is_some_and(|(_, nanos)| nanos != 0)
    }
}

/// Why a discovered entry could not be served from the cache. Reported in
/// aggregate by `NOTEMA_TIMING=2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MissCause {
    /// No cached record for this path: a new entry, or one renamed into place.
    Absent,
    Len,
    Mtime,
    /// The stamp matched but was taken too soon after a whole-second mtime to
    /// rule out a further write in the same tick.
    Racy,
    /// The stamp matched but the filesystem reported no modification time, so
    /// only length distinguishes one revision from another.
    Unstamped,
    /// Same file, different journal — the journal folder was renamed.
    Journal,
    /// The policy forced a reload. Not a cache failure.
    Rebuild,
}

impl MissCause {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Len => "len",
            Self::Mtime => "mtime",
            Self::Racy => "racy",
            Self::Unstamped => "unstamped",
            Self::Journal => "journal",
            Self::Rebuild => "rebuild",
        }
    }
}

/// Opaque version of an entry file captured alongside an authoritative read.
/// Pass it back when saving to avoid overwriting a file changed by another
/// process while the editor was open.
///
/// This is a digest of the exact bytes the file holds — ciphertext for an
/// encrypted entry — not a summary of its metadata, so two different contents
/// can never share a revision whatever the filesystem reports about length or
/// timestamps. `FileStamp` answers a different, weaker question for the entry
/// cache; the two must not be conflated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryRevision([u8; 32]);

impl EntryRevision {
    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// Take a revision with no bytes already in hand. Only the write-time
    /// conflict check needs this; every other caller hashes a read it was
    /// making anyway.
    pub(crate) fn read(path: &std::path::Path) -> std::io::Result<Self> {
        Ok(Self::from_bytes(&std::fs::read(path)?))
    }
}

pub(crate) struct DiscoveredEntry {
    pub source: notema_domain::EntryPath,
    pub stamp: FileStamp,
    /// Whether `stamp` was taken far enough past the file's own mtime to rule
    /// out a further write hiding inside the filesystem's mtime granularity.
    /// A property of the observation, not of the file, so it is decided during
    /// the walk — and recorded with the stamp, since re-deciding it later would
    /// call every racy observation trustworthy purely for having aged.
    pub stamp_trusted: bool,
}

impl DiscoveredEntry {
    /// Why `record` cannot be reused for this file, or `None` for a cache hit.
    /// Cache hits are decided only here.
    ///
    /// The stamp fields are compared before the trust check so that `Racy` and
    /// `Unstamped` count only files that looked unchanged and were re-read
    /// anyway — what the racy rule actually costs.
    pub(crate) fn miss_cause(&self, record: Option<&CachedRecord>) -> Option<MissCause> {
        let Some(record) = record else {
            return Some(MissCause::Absent);
        };
        if record.stamp.len != self.stamp.len {
            return Some(MissCause::Len);
        }
        if record.stamp.modified != self.stamp.modified {
            return Some(MissCause::Mtime);
        }
        // Both observations must be trustworthy. A record written from a racy
        // one describes bytes that a same-second, same-length rewrite could have
        // replaced without moving the stamp at all.
        if !self.stamp_trusted || !record.trusted {
            return Some(match self.stamp.modified {
                Some(_) => MissCause::Racy,
                None => MissCause::Unstamped,
            });
        }
        if record.entry.journal != self.source.journal {
            return Some(MissCause::Journal);
        }
        None
    }
}

pub(crate) struct CachedRecord {
    pub stamp: FileStamp,
    /// Whether the observation that took `stamp` could rule out a later write
    /// hiding inside the filesystem's mtime granularity.
    pub trusted: bool,
    pub entry: Entry,
}

/// A decoded cache kept opaque so callers cannot confuse it with validated data.
pub struct CachedLibrary {
    pub(crate) journals: Vec<Journal>,
    pub(crate) records: Vec<CachedRecord>,
    pub(crate) warning: Option<String>,
}

/// Result of probing the cache without touching the journal source tree.
pub struct CacheRead {
    pub cached: Option<CachedLibrary>,
    pub report: LibraryLoadReport,
}

impl CachedLibrary {
    /// Clone the cached view for immediate read-only display while this value is
    /// retained as the validation seed.
    pub fn snapshot(&self) -> LibrarySnapshot {
        LibrarySnapshot {
            journals: self.journals.clone(),
            entries: self
                .records
                .iter()
                .map(|record| record.entry.clone())
                .collect(),
            report: LibraryLoadReport {
                entries: self.records.len(),
                cache_hits: self.records.len(),
                cache_status: CacheStatus::Hit,
                cache_warning: self.warning.clone(),
                ..LibraryLoadReport::default()
            },
        }
    }
}

pub(crate) fn path_for_record(record: &CachedRecord) -> PathBuf {
    record.entry.path.clone()
}

#[cfg(test)]
mod tests {
    use super::{Duration, EntryRevision, FileStamp, SystemTime, UNIX_EPOCH};

    fn stamp_at(seconds: u64, nanos: u32) -> FileStamp {
        FileStamp {
            len: 64,
            modified: Some((seconds, nanos)),
        }
    }

    /// Built from values rather than from a real file, so the answer does not
    /// depend on what the test machine's filesystem happens to resolve.
    #[test]
    fn a_coarse_mtime_is_trusted_only_once_its_tick_has_closed() {
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let seconds = 1_800_000_000u64;

        assert!(
            !stamp_at(seconds, 0).is_trustworthy_at(now),
            "coarse, fresh"
        );
        assert!(
            !stamp_at(seconds - 1, 0).is_trustworthy_at(now),
            "still inside the coarse window"
        );
        assert!(
            stamp_at(seconds - 5, 0).is_trustworthy_at(now),
            "coarse but settled"
        );
        assert!(
            stamp_at(seconds, 1).is_trustworthy_at(now),
            "sub-second resolution proven"
        );
        assert!(
            !stamp_at(seconds + 60, 0).is_trustworthy_at(now),
            "a future mtime is not an old one"
        );

        let unstamped = FileStamp {
            len: 64,
            modified: None,
        };
        assert!(!unstamped.is_trustworthy_at(SystemTime::now()));

        assert!(stamp_at(seconds, 1).has_subsecond_mtime());
        assert!(!stamp_at(seconds, 0).has_subsecond_mtime());
    }

    /// The property [`FileStamp`] cannot offer, and the reason a revision is not
    /// one: equal-length contents are still told apart.
    #[test]
    fn revisions_separate_equal_length_contents() {
        assert_eq!(
            EntryRevision::from_bytes(b"aaaa"),
            EntryRevision::from_bytes(b"aaaa")
        );
        assert_ne!(
            EntryRevision::from_bytes(b"aaaa"),
            EntryRevision::from_bytes(b"bbbb")
        );
    }
}
