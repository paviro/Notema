use crate::{AppResult, JournalStore, storage};
use anyhow::{Context, bail};
use jiff::Zoned;
use notema_encryption::{self as crypto, KeyPaths};
use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationSummary {
    pub migrated_files: usize,
    /// Post-success cleanup steps that failed after the change itself committed
    /// (leftover backup, un-advanced trust pins). Surface to the user; never a
    /// reason to treat the operation as failed.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecryptSummary {
    pub migrated_files: usize,
    pub backup_path: Option<PathBuf>,
    pub disabled_identity_file: PathBuf,
    pub disabled_trust_file: Option<PathBuf>,
    /// The retired key had been fetched by a command, so a copy remains in
    /// whatever that command read from — the user's to remove, not ours.
    pub key_left_in_command_store: bool,
    /// See [`MigrationSummary::warnings`].
    pub warnings: Vec<String>,
}

/// The outcome of renaming `identity.toml` aside: the recoverable copy, plus
/// whatever the caller has to tell the user about the key that was in it.
struct RetiredIdentity {
    path: PathBuf,
    left_in_command_store: bool,
    warnings: Vec<String>,
}

/// The local files a device retires when it notices encryption was disabled on
/// another device — the private key and roster pins it held while encrypted,
/// renamed aside rather than deleted. Returned by [`reconcile_disabled_encryption`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DisabledElsewhereCleanup {
    pub disabled_identity_file: Option<PathBuf>,
    pub disabled_trust_file: Option<PathBuf>,
}

enum MigrationMode<'a> {
    Encrypt {
        recipients: &'a crypto::EncryptionRecipients,
    },
    Decrypt {
        identity: &'a crypto::UnlockedIdentity,
    },
}

/// Progress sink for a whole-store migration: called with `(done, total)` once
/// at the start (`0, total`) and after each file is converted.
pub(crate) type ProgressFn<'a> = &'a mut dyn FnMut(usize, usize);

pub(crate) fn encrypt_store(
    store: &JournalStore,
    progress: ProgressFn<'_>,
) -> AppResult<MigrationSummary> {
    let backup = backup_store(&store.paths().journal_root)?;
    match encrypt_store_without_backup(store, progress) {
        Ok(mut summary) => {
            summary.warnings.extend(backup_cleanup_warning(&backup));
            Ok(summary)
        }
        Err(error) => {
            if let Err(restore_error) = restore_store(&store.paths().journal_root, &backup) {
                bail!(
                    "{error}; ALSO failed to roll back the store: {restore_error}. \
                     A backup of the pre-encryption store remains at {}",
                    backup.display()
                );
            }
            Err(anyhow::anyhow!(
                "{error}; encryption failed and the store was restored unchanged"
            ))
        }
    }
}

pub(crate) fn encrypt_store_without_backup(
    store: &JournalStore,
    progress: ProgressFn<'_>,
) -> AppResult<MigrationSummary> {
    let paths = store.paths();
    let recipients = crypto::EncryptionRecipients::for_store(&paths.keys)?;
    let migrated_files = migrate_store_files(
        paths.journal_root.as_path(),
        MigrationMode::Encrypt {
            recipients: &recipients,
        },
        progress,
    )?;
    Ok(MigrationSummary {
        migrated_files,
        warnings: Vec::new(),
    })
}

pub(crate) fn decrypt_store(
    store: &JournalStore,
    identity: &crypto::UnlockedIdentity,
    progress: ProgressFn<'_>,
) -> AppResult<DecryptSummary> {
    let paths = store.paths();
    let root = paths.journal_root.as_path();
    let backup = backup_store(root)?;
    let migration = migrate_store_files(root, MigrationMode::Decrypt { identity }, progress);
    let migrated_files = match migration {
        Ok(migrated_files) => migrated_files,
        Err(error) => {
            if let Err(restore_error) = restore_store(root, &backup) {
                bail!(
                    "{error}; ALSO failed to roll back the store: {restore_error}. \
                     A backup of the pre-decryption store remains at {}",
                    backup.display()
                );
            }
            bail!("{error}; decryption failed and the store was restored unchanged");
        }
    };
    // These retire key files in the config dir, outside the root snapshot, so a
    // failure here must not roll the (fully decrypted) root back.
    clear_age_dir(&paths.keys)?;
    let disabled_trust_file = disable_trust_file(&paths.keys)?;
    let retired = disable_identity_file(&paths.keys)?;
    // The decrypt completed, so this snapshot is a deliberate keep, not a crash
    // leftover: move it out of the `*.backup-*` namespace the startup warning
    // covers. On a failed rename the old name stays — merely over-warned.
    let kept = backup
        .file_name()
        .and_then(OsStr::to_str)
        .map(|name| backup.with_file_name(name.replacen(BACKUP_MARKER, DECRYPT_BACKUP_MARKER, 1)));
    let backup = match kept {
        Some(kept) if fs::rename(&backup, &kept).is_ok() => kept,
        _ => backup,
    };
    Ok(DecryptSummary {
        migrated_files,
        backup_path: Some(backup),
        disabled_identity_file: retired.path,
        disabled_trust_file,
        key_left_in_command_store: retired.left_in_command_store,
        warnings: retired.warnings,
    })
}

/// Notice an encryption *disable* that happened on another device and mirror it
/// locally. When that device turned encryption off it deleted the synced roster
/// (`devices.toml`) and decrypted every entry, but this device still holds the
/// `identity.toml` and `devices-trust.toml` it used while encrypted. Detect that —
/// a roster this device had pinned that is now gone — and retire the key and pins
/// by renaming them aside, exactly as a local [`decrypt_store`] does, so the
/// device drops back to plaintext instead of trying to unlock a store that no
/// longer exists. Returns `None` (no change) when there is nothing to reconcile.
///
/// Gated to fail safe:
/// - Requires the local trust pins to exist, so a freshly-enrolled device whose
///   synced `.age/` folder simply hasn't downloaded yet is never mistaken for a
///   disable — it has an identity but has never pinned a roster.
/// - Requires no encrypted entries to remain, so a half-synced store (roster gone
///   but entries still `.age`) keeps the key that can still read them until the
///   plaintext conversions finish syncing.
pub(crate) fn reconcile_disabled_encryption(
    store: &JournalStore,
) -> AppResult<Option<DisabledElsewhereCleanup>> {
    let paths = &store.paths().keys;
    if paths.devices_file.exists() || !paths.trust_file.exists() {
        return Ok(None);
    }
    if store_has_encrypted_entry_files(store)? {
        return Ok(None);
    }
    let disabled_trust_file = disable_trust_file(paths)?;
    let disabled_identity_file = if paths.identity_file.exists() {
        Some(disable_identity_file(paths)?.path)
    } else {
        None
    };
    Ok(Some(DisabledElsewhereCleanup {
        disabled_identity_file,
        disabled_trust_file,
    }))
}

/// Retire this device's now-dead private key after the verified roster showed a
/// revoke op for it: the store is still encrypted for other devices, so only
/// `identity.toml` is renamed aside (recoverable), letting a fresh `enroll`
/// request access without the user deleting the file by hand. The roster trust
/// pins are deliberately kept — the genesis is unchanged, so they still guard a
/// re-enroll against a swapped or rolled-back roster. Returns the renamed path,
/// or `None` when no identity exists here.
pub(crate) fn retire_revoked_identity(store: &JournalStore) -> AppResult<Option<PathBuf>> {
    let paths = &store.paths().keys;
    if !paths.identity_file.exists() {
        return Ok(None);
    }
    Ok(Some(disable_identity_file(paths)?.path))
}

/// Tear down the synced key folder when encryption is disabled: drop the signed
/// `devices.toml` roster and any leftover `pending-*.toml` join requests (which
/// would otherwise keep syncing and resurface as phantom approval modals), then
/// remove the `.age` folder itself if nothing else is left in it. The local trust
/// pins are not deleted here — the caller renames `devices-trust.toml` aside (like
/// the identity), keeping a recoverable copy; they are meaningless once the roster
/// is gone and would otherwise reject a freshly re-enabled store as a "changed
/// genesis".
fn clear_age_dir(paths: &KeyPaths) -> AppResult<()> {
    if paths.devices_file.exists() {
        fs::remove_file(&paths.devices_file)?;
    }
    if !paths.age_dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&paths.age_dir)? {
        let path = entry?.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("pending-") && name.ends_with(".toml"))
        {
            fs::remove_file(path)?;
        }
    }
    // Leaves the folder in place if the user dropped unrelated files in it.
    let _ = fs::remove_dir(&paths.age_dir);
    Ok(())
}

/// Re-encrypt every encrypted file (entries and their assets) to the store's
/// *current* recipient set. Runs after a recipient is added or removed so the
/// change reaches existing history, not just new entries. Requires an unlocked
/// identity that can decrypt the store as it stands now.
///
/// Converts every file or returns `Err` on the first failure, leaving the store
/// partially converted. Callers must run this inside [`atomic`] so such a
/// failure rolls the whole store back rather than stranding it mid-conversion.
pub(crate) fn reencrypt_store(
    store: &JournalStore,
    identity: &crypto::UnlockedIdentity,
    progress: ProgressFn<'_>,
) -> AppResult<MigrationSummary> {
    let paths = store.paths();
    let mut files = Vec::new();
    collect_store_files_including_trash(paths.journal_root.as_path(), &mut |path| {
        if path.extension() == Some(OsStr::new("age")) {
            files.push(path.to_path_buf());
        }
        Ok(())
    })?;
    files.sort();
    let recipients = crypto::EncryptionRecipients::for_store(&paths.keys)?;

    progress(0, files.len());
    for (done, path) in files.iter().enumerate() {
        reencrypt_file(path, &recipients, identity)?;
        progress(done + 1, files.len());
    }
    Ok(MigrationSummary {
        migrated_files: files.len(),
        warnings: Vec::new(),
    })
}

fn reencrypt_file(
    path: &Path,
    recipients: &crypto::EncryptionRecipients,
    identity: &crypto::UnlockedIdentity,
) -> AppResult<()> {
    // Stream old ciphertext -> plaintext -> new ciphertext without buffering the
    // whole file. Safe to write back to the same path: the source is fully read and
    // re-encrypted into a sibling temp, which is only then renamed over `path`.
    let reader = crypto::decrypt_file_reader(identity, path)?;
    recipients.encrypt_reader_to_file(reader, path)?;
    Ok(())
}

pub(crate) fn store_has_encrypted_entry_files(store: &JournalStore) -> AppResult<bool> {
    let mut has_match = false;
    collect_store_files_including_trash(store.paths().journal_root.as_path(), &mut |path| {
        // A `.md.age` attachment inside `*.assets/` is an asset, not an entry.
        if storage::is_encrypted_entry_file(path) && !is_in_assets_dir(path) {
            has_match = true;
        }
        Ok(())
    })?;
    Ok(has_match)
}

fn migrate_store_files(
    root: &Path,
    mode: MigrationMode<'_>,
    progress: ProgressFn<'_>,
) -> AppResult<usize> {
    let entry_files = migration_files(root, &mode)?;
    let asset_files = migration_asset_files(root, &mode)?;
    let total = entry_files.len() + asset_files.len();
    if total == 0 {
        return Ok(0);
    }
    ensure_no_migration_collisions(&entry_files, &mode)?;
    ensure_no_asset_collisions(&asset_files, &mode)?;

    progress(0, total);
    let mut done = 0usize;
    for source in &entry_files {
        match &mode {
            MigrationMode::Encrypt { recipients } => encrypt_plain_entry(source, recipients)?,
            MigrationMode::Decrypt { identity } => decrypt_encrypted_entry(source, identity)?,
        }
        done += 1;
        progress(done, total);
    }
    // Assets carry the same `.age` suffix as entries but keep clean body
    // links, so converting them only renames files — no entry is rewritten.
    for source in &asset_files {
        convert_asset_file(source, &mode)?;
        done += 1;
        progress(done, total);
    }
    Ok(total)
}

/// Collect asset files (inside any `*.assets/` folder) that need converting:
/// plaintext files when encrypting, `.age` files when decrypting.
fn migration_asset_files(root: &Path, mode: &MigrationMode<'_>) -> AppResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_store_files_including_trash(root, &mut |path| {
        if is_in_assets_dir(path) && asset_matches_mode(path, mode) {
            files.push(path.to_path_buf());
        }
        Ok(())
    })?;
    files.sort();
    Ok(files)
}

fn is_in_assets_dir(path: &Path) -> bool {
    path.parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".assets"))
}

fn asset_matches_mode(path: &Path, mode: &MigrationMode<'_>) -> bool {
    let is_encrypted = path.extension() == Some(OsStr::new("age"));
    match mode {
        MigrationMode::Encrypt { .. } => !is_encrypted,
        MigrationMode::Decrypt { .. } => is_encrypted,
    }
}

/// Encrypt (`<name>` → `<name>.age`) or decrypt (`<name>.age` → `<name>`) one
/// asset file in place, atomically via temp + rename.
fn convert_asset_file(path: &Path, mode: &MigrationMode<'_>) -> AppResult<()> {
    match mode {
        MigrationMode::Encrypt { recipients } => {
            let target = append_age(path);
            recipients.encrypt_reader_to_file(fs::File::open(path)?, &target)?;
            fs::remove_file(path)?;
        }
        MigrationMode::Decrypt { identity } => {
            let target = strip_age(path)?;
            let reader = crypto::decrypt_file_reader(identity, path)?;
            // This path intentionally writes plaintext to disk; streaming keeps
            // memory constant but the output is the decrypted file itself.
            stream_to_atomic_file(reader, &target)?;
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn append_age(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".age");
    path.with_file_name(name)
}

fn strip_age(path: &Path) -> AppResult<PathBuf> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("asset path has no UTF-8 file name")?;
    let base = name
        .strip_suffix(".age")
        .context("encrypted asset does not end in .age")?;
    Ok(path.with_file_name(base))
}

fn migration_files(root: &Path, mode: &MigrationMode<'_>) -> AppResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_store_files_including_trash(root, &mut |path| {
        // An attachment keeps its extension verbatim, so a `.md`/`.md.age` file
        // inside `*.assets/` belongs to the asset pass, not the entry pass.
        let matches = !is_in_assets_dir(path)
            && match mode {
                MigrationMode::Encrypt { .. } => storage::is_plain_entry_file(path),
                MigrationMode::Decrypt { .. } => storage::is_encrypted_entry_file(path),
            };
        if matches {
            files.push(path.to_path_buf());
        }
        Ok(())
    })?;
    files.sort();
    Ok(files)
}

fn collect_store_files_including_trash(
    dir: &Path,
    visit: &mut impl FnMut(&Path) -> AppResult<()>,
) -> AppResult<()> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_store_files_including_trash(&path, visit)?;
            continue;
        }
        visit(&path)?;
    }

    Ok(())
}

fn ensure_no_migration_collisions(files: &[PathBuf], mode: &MigrationMode<'_>) -> AppResult<()> {
    for source in files {
        let target = migration_target(source, mode)?;
        if target.exists() {
            bail!(
                "cannot migrate {}; target already exists: {}",
                source.display(),
                target.display()
            );
        }
    }
    Ok(())
}

/// Guard the asset conversions the same way [`ensure_no_migration_collisions`]
/// guards entries: refuse to run if converting an asset would clobber a file
/// that already exists (an inconsistent store holding both `x.png` and
/// `x.png.age`), since the conversion renames onto the target in place.
fn ensure_no_asset_collisions(files: &[PathBuf], mode: &MigrationMode<'_>) -> AppResult<()> {
    for source in files {
        let target = match mode {
            MigrationMode::Encrypt { .. } => append_age(source),
            MigrationMode::Decrypt { .. } => strip_age(source)?,
        };
        if target.exists() {
            bail!(
                "cannot migrate asset {}; target already exists: {}",
                source.display(),
                target.display()
            );
        }
    }
    Ok(())
}

fn encrypt_plain_entry(path: &Path, recipients: &crypto::EncryptionRecipients) -> AppResult<()> {
    let target = path.with_extension("md.age");
    recipients.encrypt_reader_to_file(fs::File::open(path)?, &target)?;
    fs::remove_file(path)?;
    Ok(())
}

fn decrypt_encrypted_entry(path: &Path, identity: &crypto::UnlockedIdentity) -> AppResult<()> {
    let target = decrypted_entry_path(path)?;
    let reader = crypto::decrypt_file_reader(identity, path)?;
    // Stream the plaintext straight to disk (decrypting the store intentionally
    // produces plaintext files). We can't cheaply re-validate the whole payload
    // as UTF-8 while streaming, so we keep only the emptiness guard via the byte
    // count; entry text is UTF-8-validated on read.
    let written = stream_to_atomic_file(reader, &target)?;
    if written == 0 {
        fs::remove_file(&target)?;
        bail!("decrypted entry is empty: {}", path.display());
    }
    fs::remove_file(path)?;
    Ok(())
}

/// Copy `reader` into `path` via an atomic temp+rename, returning the number of
/// bytes written. Used for the decrypt-migration paths, which produce plaintext
/// files on disk by design.
fn stream_to_atomic_file<R: io::Read>(mut reader: R, path: &Path) -> AppResult<u64> {
    let mut written = 0u64;
    crypto::atomic_write_with(path, false, |file| {
        written = io::copy(&mut reader, file)?;
        Ok(())
    })?;
    Ok(written)
}

fn migration_target(path: &Path, mode: &MigrationMode<'_>) -> AppResult<PathBuf> {
    match mode {
        MigrationMode::Encrypt { .. } => Ok(path.with_extension("md.age")),
        MigrationMode::Decrypt { .. } => decrypted_entry_path(path),
    }
}

fn decrypted_entry_path(path: &Path) -> AppResult<PathBuf> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("encrypted entry path has no UTF-8 file name")?;
    let plain_name = name
        .strip_suffix(".md.age")
        .context("encrypted entry path does not end in .md.age")?;
    Ok(path.with_file_name(format!("{plain_name}.md")))
}

/// Run `op` as an all-or-nothing change to the store: snapshot the whole
/// journal root first, and on any error roll every file (entries, assets, and
/// the `devices.toml` roster) back to the snapshot so a failed key change leaves
/// no trace. The snapshot is deleted on success. Key-changing operations must run
/// their roster mutation *and* [`reencrypt_store`] inside this so the two can't
/// diverge. (The local trust pins live outside the root; callers advance them
/// only after this returns `Ok`.)
pub(crate) fn atomic<T>(
    store: &JournalStore,
    op: impl FnOnce() -> AppResult<T>,
) -> AppResult<(T, Vec<String>)> {
    let root = store.paths().journal_root.clone();
    let backup = backup_store(&root)?;
    match op() {
        Ok(value) => Ok((value, backup_cleanup_warning(&backup).into_iter().collect())),
        Err(error) => {
            if let Err(restore_error) = restore_store(&root, &backup) {
                bail!(
                    "{error}; ALSO failed to roll back the store: {restore_error}. \
                     A backup of the pre-change store remains at {}",
                    backup.display()
                );
            }
            Err(error)
        }
    }
}

/// Replace `root` with `backup` wholesale: rename the (partially changed) root
/// aside, rename the snapshot into its place, then drop the aside copy. The
/// root is never deleted before the snapshot is in position, so a crash at any
/// point leaves a complete store either at `root` or in a `*.backup-*` sibling
/// the startup scan reports — never an empty root that would silently mint a
/// fresh store id.
pub(crate) fn restore_store(root: &Path, backup: &Path) -> AppResult<()> {
    let aside = if root.exists() {
        let aside = backup_path(root);
        fs::rename(root, &aside)?;
        Some(aside)
    } else {
        None
    };
    if let Err(error) = fs::rename(backup, root) {
        if let Some(aside) = &aside {
            let _ = fs::rename(aside, root);
        }
        return Err(error.into());
    }
    if let Some(aside) = aside {
        // Best effort: a leftover aside copy is caught by the startup scan.
        let _ = fs::remove_dir_all(aside);
    }
    Ok(())
}

pub(crate) fn backup_store(root: &Path) -> AppResult<PathBuf> {
    let backup = backup_path(root);
    copy_dir_all(root, &backup)?;
    Ok(backup)
}

/// Remove a consumed backup, degrading failure to a warning: the operation the
/// backup covered already committed, so a stuck cleanup must not report it as
/// failed. A leftover is also caught by the startup scan.
pub(crate) fn backup_cleanup_warning(backup: &Path) -> Option<String> {
    fs::remove_dir_all(backup).err().map(|error| {
        format!(
            "the change succeeded, but its backup at {} could not be removed: {error}; delete it by hand",
            backup.display()
        )
    })
}

/// The name marker of a snapshot a migration is still using (or crashed and
/// left behind); [`DECRYPT_BACKUP_MARKER`] is what a successful decrypt renames
/// its deliberately kept snapshot to, taking it out of the crash-leftover
/// namespace the startup warning covers.
const BACKUP_MARKER: &str = ".backup-";
const DECRYPT_BACKUP_MARKER: &str = ".decrypt-backup-";

/// Leftover `<root_name>.backup-*` siblings of the journal root: snapshots a
/// crashed migration or restore never consumed. Detection only — a leftover can
/// hold the sole complete copy of the store, so nothing here deletes it.
pub(crate) fn stale_backup_dirs(root: &Path) -> AppResult<Vec<PathBuf>> {
    backup_siblings(root, BACKUP_MARKER, |file_type| file_type.is_dir())
}

/// The `<root_name>.decrypt-backup-*` siblings a successful decrypt kept on
/// purpose: pre-decryption ciphertext, not a crash artifact — surface as a
/// gentle cleanup reminder, not a warning.
pub(crate) fn kept_decrypt_backups(root: &Path) -> AppResult<Vec<PathBuf>> {
    backup_siblings(root, DECRYPT_BACKUP_MARKER, |file_type| file_type.is_dir())
}

/// Leftover `identity.toml.backup-*` siblings of the identity file: private-key
/// snapshots a crashed rotation left behind. Detection only, like
/// [`stale_backup_dirs`] — a leftover can hold the only copy of the
/// pre-rotation key.
pub(crate) fn stale_identity_backups(paths: &KeyPaths) -> AppResult<Vec<PathBuf>> {
    backup_siblings(&paths.identity_file, BACKUP_MARKER, |file_type| {
        file_type.is_file()
    })
}

fn backup_siblings(
    path: &Path,
    marker: &str,
    keep: impl Fn(&fs::FileType) -> bool,
) -> AppResult<Vec<PathBuf>> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name().and_then(OsStr::to_str))
    else {
        return Ok(Vec::new());
    };
    if !parent.exists() {
        return Ok(Vec::new());
    }
    let prefix = format!("{name}{marker}");
    let mut leftovers = Vec::new();
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        if keep(&entry.file_type()?)
            && entry
                .file_name()
                .to_str()
                .is_some_and(|sibling| sibling.starts_with(&prefix))
        {
            leftovers.push(entry.path());
        }
    }
    leftovers.sort();
    Ok(leftovers)
}

/// Sibling backup path for a single file: `<file_name>.backup-<timestamp>`,
/// matching the naming the startup scan looks for.
pub(crate) fn file_backup_path(file: &Path) -> PathBuf {
    let timestamp = now_stamp_nanos();
    let name = file.file_name().and_then(OsStr::to_str).unwrap_or("notema");
    file.with_file_name(format!("{name}.backup-{timestamp}"))
}

/// A `YYYYMMDDhhmmss` stamp with a nine-digit sub-second suffix, for backup and
/// disabled-file names that need to sort and stay unique down to the nanosecond.
fn now_stamp_nanos() -> String {
    let now = Zoned::now();
    format!(
        "{}{:09}",
        now.strftime("%Y%m%d%H%M%S"),
        now.subsec_nanosecond()
    )
}

/// Backups must live on the root's filesystem so [`restore_store`]'s renames
/// stay atomic; the sibling slot is the only guaranteed such place. Cost: if
/// the root's *parent* is inside a synced tree, a mid-migration plaintext
/// snapshot syncs too — the startup scan at least surfaces any leftover.
fn backup_path(root: &Path) -> PathBuf {
    let timestamp = now_stamp_nanos();
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("notema");
    root.with_file_name(format!("{name}.backup-{timestamp}"))
}

fn copy_dir_all(source: &Path, target: &Path) -> AppResult<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&source_path, &target_path)?;
        } else {
            fs::copy(&source_path, &target_path)?;
        }
    }
    Ok(())
}

/// Retire this device's private key when encryption is turned off: rename
/// `identity.toml` aside as `identity.disabled-<timestamp>.toml` — a recoverable
/// copy, not a delete. Returns the new path.
fn disable_identity_file(paths: &KeyPaths) -> AppResult<RetiredIdentity> {
    // A key kept in the keychain or a secret manager would leave the retired
    // copy pointing at something nothing references any more, so inline it
    // first: this rename is meant to be recoverable, not a delete.
    let snapshot = crypto::snapshot_identity(paths);
    let mut retired = RetiredIdentity {
        path: rename_aside(&paths.identity_file, "identity", "toml")?,
        left_in_command_store: false,
        warnings: Vec::new(),
    };
    match snapshot {
        Ok(snapshot) if snapshot.is_external() => {
            crypto::atomic_write_private(&retired.path, &snapshot.portable_bytes()?)?;
            // Only once the recoverable copy is on disk. A keychain item is ours
            // to delete; a secret manager's copy is the user's, so it is named
            // rather than reached into.
            snapshot.forget_stored_key();
            retired.left_in_command_store = snapshot.store() == crypto::KeyStore::Command;
        }
        Ok(_) => {}
        // Not a reason to refuse the disable, but the retired copy is then only
        // a pointer at a key this device can no longer reach.
        Err(error) => retired.warnings.push(format!(
            "could not read this device's key ({error}), so {} only points at where it was kept rather than holding it",
            retired.path.display()
        )),
    }
    Ok(retired)
}

/// Retire this device's roster trust pins the same way as its key, renaming
/// `devices-trust.toml` aside rather than deleting it. Returns the new path, or
/// `None` when there were no pins on this device to retire.
fn disable_trust_file(paths: &KeyPaths) -> AppResult<Option<PathBuf>> {
    if !paths.trust_file.exists() {
        return Ok(None);
    }
    Ok(Some(rename_aside(
        &paths.trust_file,
        "devices-trust",
        "toml",
    )?))
}

/// Rename `path` aside as `<stem>.disabled-<timestamp>.<ext>` next to it,
/// returning the new path. Shared by the key and trust-pin retirement so both
/// leave a recoverable, uniformly-named copy when encryption is disabled.
fn rename_aside(path: &Path, stem: &str, ext: &str) -> AppResult<PathBuf> {
    let target = disabled_path(path, stem, ext);
    fs::rename(path, &target)?;
    Ok(target)
}

fn disabled_path(path: &Path, stem: &str, ext: &str) -> PathBuf {
    let timestamp = Zoned::now().strftime("%Y%m%d%H%M%S").to_string();
    disabled_path_for_timestamp(path, stem, ext, &timestamp)
}

fn disabled_path_for_timestamp(path: &Path, stem: &str, ext: &str, timestamp: &str) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let base = parent.join(format!("{stem}.disabled-{timestamp}.{ext}"));
    if !base.exists() {
        return base;
    }

    for _ in 0..32 {
        let candidate = parent.join(format!(
            "{stem}.disabled-{timestamp}-{}.{ext}",
            storage::random_id(6)
        ));
        if !candidate.exists() {
            return candidate;
        }
    }

    parent.join(format!(
        "{stem}.disabled-{timestamp}-{}.{ext}",
        Zoned::now().timestamp().as_nanosecond()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn stale_backup_sibling_is_reported() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("journals");
        fs::create_dir_all(&root).unwrap();
        let stale = dir.path().join("journals.backup-20260730120000");
        fs::create_dir_all(&stale).unwrap();
        fs::create_dir_all(dir.path().join("unrelated")).unwrap();
        fs::write(dir.path().join("journals.backup-notadir"), "file").unwrap();

        assert_eq!(stale_backup_dirs(&root).unwrap(), vec![stale]);
    }

    #[test]
    fn kept_decrypt_backup_is_not_reported_as_stale() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("journals");
        fs::create_dir_all(&root).unwrap();
        let kept = dir.path().join("journals.decrypt-backup-20260730120000");
        fs::create_dir_all(&kept).unwrap();

        assert!(stale_backup_dirs(&root).unwrap().is_empty());
        assert_eq!(kept_decrypt_backups(&root).unwrap(), vec![kept]);
    }

    #[test]
    fn restore_store_replaces_root_with_backup() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("journals");
        let backup = dir.path().join("journals.backup-1");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("partial.md"), "half converted").unwrap();
        fs::create_dir_all(&backup).unwrap();
        fs::write(backup.join("entry.md"), "snapshot").unwrap();

        restore_store(&root, &backup).unwrap();

        assert_eq!(
            fs::read_to_string(root.join("entry.md")).unwrap(),
            "snapshot"
        );
        assert!(!root.join("partial.md").exists());
        let leftovers = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".backup-"))
            .count();
        assert_eq!(leftovers, 0, "aside copy and backup should both be gone");
    }

    #[test]
    fn restore_store_keeps_root_when_backup_rename_fails() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("journals");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("entry.md"), "still here").unwrap();

        let missing_backup = dir.path().join("journals.backup-missing");
        restore_store(&root, &missing_backup).unwrap_err();

        assert_eq!(
            fs::read_to_string(root.join("entry.md")).unwrap(),
            "still here",
            "root should be renamed back after a failed restore"
        );
    }

    #[test]
    fn disabled_path_uses_timestamped_filename() {
        let dir = tempdir().unwrap();
        let identity = dir.path().join("identity.toml");

        let disabled = disabled_path_for_timestamp(&identity, "identity", "toml", "20260702123456");

        assert_eq!(
            disabled,
            dir.path().join("identity.disabled-20260702123456.toml")
        );
    }

    #[test]
    fn disabled_path_reuses_stem_and_extension_for_trust_pins() {
        let dir = tempdir().unwrap();
        let trust = dir.path().join("devices-trust.toml");

        let disabled =
            disabled_path_for_timestamp(&trust, "devices-trust", "toml", "20260702123456");

        assert_eq!(
            disabled,
            dir.path()
                .join("devices-trust.disabled-20260702123456.toml")
        );
    }
}
