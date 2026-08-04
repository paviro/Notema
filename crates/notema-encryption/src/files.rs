use crate::Result;
use std::{
    fmt,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

/// A unique hidden sibling temp path next to `target`, for atomic
/// write-then-rename. Named `.notema-<pid>-<rand>.<suffix>` in the target's
/// directory so it lands on the same filesystem as the eventual rename target.
fn sibling_temp_path(target: &Path, suffix: &str) -> Result<PathBuf> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let mut noise = [0u8; 8];
    getrandom::fill(&mut noise)
        .map_err(|error| crate::EncryptionError::Randomness(error.to_string()))?;
    Ok(parent.join(format!(
        ".notema-{}-{}.{suffix}",
        std::process::id(),
        hex::encode(noise),
    )))
}

/// Write `content` to `path` via a sibling temp file plus rename, so a crash
/// mid-write can't truncate an existing file (which would strand every device)
/// or leave a half-written join request behind.
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    write_atomic(path, false, |file| Ok(file.write_all(content)?))
}

/// Atomically write a file readable only by its owner (mode 0600 on Unix),
/// creating parent directories as needed.
///
/// Off Unix only the atomicity holds: the file inherits its directory's ACL,
/// which under `%APPDATA%` is already the user, SYSTEM and Administrators.
/// Tightening it needs Win32 FFI no maintained crate wraps safely.
pub fn atomic_write_private(path: &Path, content: &[u8]) -> Result<()> {
    write_atomic(path, true, |file| Ok(file.write_all(content)?))
}

/// How a file that [`atomic_write_private`] made owner-only has stopped being
/// owner-only, ordered most serious first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateFileExposure {
    /// Someone else owns it, so they can rewrite it whenever they like — mode
    /// bits don't constrain a file's owner.
    ForeignOwner { uid: u32 },
    /// Group- or world-writable: another user can rewrite it in place.
    Writable { mode: u32 },
    /// Group- or world-readable, but only its owner can write it.
    Readable { mode: u32 },
}

impl PrivateFileExposure {
    /// What to run to close it. The owner case has to fix the mode too: a file
    /// taken back from someone else carries whatever mode they left on it.
    pub fn remedy(self, path: &Path) -> String {
        match self {
            Self::ForeignOwner { .. } => format!(
                "take it back with `sudo chown \"$(id -un)\" {path} && chmod 600 {path}`, or restore it from a backup",
                path = path.display()
            ),
            Self::Writable { .. } | Self::Readable { .. } => {
                format!("run `chmod 600 {}`", path.display())
            }
        }
    }
}

/// Completes a sentence whose subject is the file: "… is mode 0666, so …".
impl fmt::Display for PrivateFileExposure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignOwner { uid } => write!(f, "is owned by uid {uid}, not by you"),
            Self::Writable { mode } => {
                write!(f, "is mode {mode:04o}, so other users can write it")
            }
            Self::Readable { mode } => {
                write!(f, "is mode {mode:04o}, so other users can read it")
            }
        }
    }
}

/// Open a file that is meant to be private, alongside a verdict on whether its
/// permissions still make it so — `None` when only its owner can reach it.
///
/// Owner and mode come from an `fstat` on the open handle, not a second `stat`
/// of the path, so the file that was judged is the file the caller then reads.
///
/// The parent directory is deliberately not checked: someone who can write it
/// cannot leave behind a file owned by us, so the owner check covers it — and a
/// directory check would fail on the group-writable one a umask of 002 hands out.
///
/// Off Unix this reports nothing: [`atomic_write_private`] makes no owner-only
/// claim there, so there is none to verify.
pub(crate) fn open_private(path: &Path) -> Result<(fs::File, Option<PrivateFileExposure>)> {
    let file = fs::File::open(path)?;
    let exposure = exposure_of(&file)?;
    Ok((file, exposure))
}

#[cfg(unix)]
fn exposure_of(file: &fs::File) -> Result<Option<PrivateFileExposure>> {
    use std::os::unix::fs::MetadataExt;

    let metadata = file.metadata()?;
    let uid = metadata.uid();
    if uid != rustix::process::geteuid().as_raw() {
        return Ok(Some(PrivateFileExposure::ForeignOwner { uid }));
    }
    // Permission bits only: the file type would otherwise land in the octal the
    // error prints back.
    let mode = metadata.mode() & 0o7777;
    if mode & 0o022 != 0 {
        return Ok(Some(PrivateFileExposure::Writable { mode }));
    }
    if mode & 0o044 != 0 {
        return Ok(Some(PrivateFileExposure::Readable { mode }));
    }
    Ok(None)
}

#[cfg(not(unix))]
fn exposure_of(_file: &fs::File) -> Result<Option<PrivateFileExposure>> {
    Ok(None)
}

/// Atomically produce `path` by writing through a sibling temp file: `write`
/// receives the freshly created temp file and streams its content into it, then
/// the temp is fsynced and renamed over `path`. Lets callers stream data
/// (e.g. an age encryptor) straight to disk without buffering the whole payload,
/// while keeping the same crash-safety guarantees as [`atomic_write`].
pub fn atomic_write_with<F>(path: &Path, private: bool, write: F) -> Result<()>
where
    F: FnOnce(&mut fs::File) -> Result<()>,
{
    write_atomic(path, private, write)
}

fn write_atomic<F>(path: &Path, private: bool, write: F) -> Result<()>
where
    F: FnOnce(&mut fs::File) -> Result<()>,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = sibling_temp_path(path, "tmp")?;
    let result = write_temp_then_rename(&temp, path, private, write);
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn write_temp_then_rename<F>(temp: &Path, path: &Path, private: bool, write: F) -> Result<()>
where
    F: FnOnce(&mut fs::File) -> Result<()>,
{
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if private {
            options.mode(0o600);
        }
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut file = options.open(temp)?;
    write(&mut file)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, path)?;
    sync_parent_dir(path);
    Ok(())
}

/// Best-effort fsync of `path`'s parent directory, so a fresh file's directory
/// entry survives a crash. Silent no-op off Unix. Callers that reserve a name
/// with `create_new` (entry and asset writes) rather than routing through the
/// atomic write-then-rename reuse this to keep the same durability guarantee.
pub fn sync_parent_dir(path: &Path) {
    #[cfg(unix)]
    if let Some(parent) = path.parent()
        && let Ok(dir) = fs::File::open(parent)
    {
        let _ = dir.sync_all();
    }
    #[cfg(not(unix))]
    let _ = path;
}
