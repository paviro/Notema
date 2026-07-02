use crate::{AppResult, config::Config, crypto, storage};
use chrono::Local;
use nanoid::nanoid;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn encrypt_workspace(config_path: &Path, config: &Config) -> AppResult<()> {
    let paths = crypto::EncryptionPaths::for_config(config_path, &config.journal_root)?;
    let recipient = if crypto::should_encrypt(&paths) {
        crypto::public_recipient(&paths)?
    } else if workspace_has_encrypted_entry_files(&config.journal_root)? {
        return Err(format!(
            "encrypted entries already exist but recipients file is missing at {}; cannot safely continue encryption",
            paths.recipients_file.display()
        )
        .into());
    } else {
        println!("No journal encryption identity configured; generating an age identity.");
        crypto::generate_identity_store_interactive(&paths)?
    };

    migrate_workspace(
        &config.journal_root,
        MigrationMode::Encrypt { paths: &paths },
    )?;
    println!(
        "Encrypted journal workspace at {}",
        config.journal_root.display()
    );
    println!(
        "Encryption recipient: {recipient}. Age identity: {}. Back it up; without it encrypted journal files cannot be decrypted.",
        paths.identity_file.display()
    );
    Ok(())
}

pub fn decrypt_workspace(config_path: &Path, config: &Config) -> AppResult<()> {
    let paths = crypto::EncryptionPaths::for_config(config_path, &config.journal_root)?;
    if !crypto::can_decrypt(&paths) {
        return Err(format!(
            "age identity not found at {}; encrypted entries cannot be decrypted on this machine",
            paths.identity_file.display()
        )
        .into());
    }
    let identity = crypto::prompt_unlock_identity(&paths)?;
    migrate_workspace(
        &config.journal_root,
        MigrationMode::Decrypt {
            identity: &identity,
        },
    )?;
    if paths.recipients_file.exists() {
        fs::remove_file(&paths.recipients_file)?;
    }
    let disabled_identity = disable_identity_file(&paths)?;
    println!(
        "Decrypted journal workspace at {}",
        config.journal_root.display()
    );
    println!("Disabled age identity at {}", disabled_identity.display());
    Ok(())
}

enum MigrationMode<'a> {
    Encrypt {
        paths: &'a crypto::EncryptionPaths,
    },
    Decrypt {
        identity: &'a crypto::UnlockedIdentity,
    },
}

fn migrate_workspace(root: &Path, mode: MigrationMode<'_>) -> AppResult<()> {
    let files = migration_files(root, &mode)?;
    if files.is_empty() {
        return Ok(());
    }
    ensure_no_migration_collisions(&files, &mode)?;
    let backup = backup_workspace(root)?;

    let result = (|| -> AppResult<()> {
        for source in files {
            match mode {
                MigrationMode::Encrypt { paths } => encrypt_plain_entry(&source, paths)?,
                MigrationMode::Decrypt { identity } => decrypt_encrypted_entry(&source, identity)?,
            }
        }
        Ok(())
    })();

    if let Err(error) = result {
        eprintln!(
            "Migration failed; plaintext backup remains at {}",
            backup.display()
        );
        return Err(error);
    }

    if matches!(mode, MigrationMode::Encrypt { .. }) {
        fs::remove_dir_all(&backup)?;
    } else {
        println!("Backup written to {}", backup.display());
    }

    Ok(())
}

fn migration_files(root: &Path, mode: &MigrationMode<'_>) -> AppResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_workspace_files_including_trash(root, &mut |path| {
        let matches = match mode {
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

pub fn workspace_has_encrypted_entry_files(root: &Path) -> AppResult<bool> {
    let mut has_match = false;
    collect_workspace_files_including_trash(root, &mut |path| {
        if storage::is_encrypted_entry_file(path) {
            has_match = true;
        }
        Ok(())
    })?;
    Ok(has_match)
}

fn collect_workspace_files_including_trash(
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
            collect_workspace_files_including_trash(&path, visit)?;
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
            return Err(format!(
                "cannot migrate {}; target already exists: {}",
                source.display(),
                target.display()
            )
            .into());
        }
    }
    Ok(())
}

fn encrypt_plain_entry(path: &Path, paths: &crypto::EncryptionPaths) -> AppResult<()> {
    let target = path.with_extension("md.age");
    let temp = unique_temp_path(&target, "tmp.age")?;
    crypto::encrypt_file(paths, path, &temp)?;
    fs::rename(&temp, &target)?;
    fs::remove_file(path)?;
    Ok(())
}

fn decrypt_encrypted_entry(path: &Path, identity: &crypto::UnlockedIdentity) -> AppResult<()> {
    let target = decrypted_entry_path(path)?;
    let temp = unique_temp_path(&target, "tmp.md")?;
    crypto::decrypt_file(identity, path, &temp)?;
    let decrypted = fs::read_to_string(&temp)?;
    if decrypted.is_empty() {
        let _ = fs::remove_file(&temp);
        return Err(format!("decrypted entry is empty: {}", path.display()).into());
    }
    fs::rename(&temp, &target)?;
    fs::remove_file(path)?;
    Ok(())
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
        .ok_or("encrypted entry path has no UTF-8 file name")?;
    let plain_name = name
        .strip_suffix(".md.age")
        .ok_or("encrypted entry path does not end in .md.age")?;
    Ok(path.with_file_name(format!("{plain_name}.md")))
}

fn backup_workspace(root: &Path) -> AppResult<PathBuf> {
    let backup = backup_path(root);
    copy_dir_all(root, &backup)?;
    Ok(backup)
}

fn backup_path(root: &Path) -> PathBuf {
    let timestamp = Local::now().format("%Y%m%d%H%M%S%f");
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("journal");
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

fn disable_identity_file(paths: &crypto::EncryptionPaths) -> AppResult<PathBuf> {
    let target = disabled_identity_path(&paths.identity_file);
    fs::rename(&paths.identity_file, &target)?;
    Ok(target)
}

fn disabled_identity_path(identity_file: &Path) -> PathBuf {
    let timestamp = Local::now().format("%Y%m%d%H%M%S");
    disabled_identity_path_for_timestamp(identity_file, &timestamp.to_string())
}

fn disabled_identity_path_for_timestamp(identity_file: &Path, timestamp: &str) -> PathBuf {
    let parent = identity_file.parent().unwrap_or_else(|| Path::new(""));
    let base = parent.join(format!("identity.disabled-{timestamp}.age"));
    if !base.exists() {
        return base;
    }

    for _ in 0..32 {
        let candidate = parent.join(format!("identity.disabled-{timestamp}-{}.age", nanoid!(6)));
        if !candidate.exists() {
            return candidate;
        }
    }

    parent.join(format!(
        "identity.disabled-{timestamp}-{}.age",
        Local::now().timestamp_nanos_opt().unwrap_or_default()
    ))
}

/// Builds a unique temporary path in the same directory as `target` so the
/// later `fs::rename` stays on one filesystem (avoids cross-device EXDEV).
fn unique_temp_path(target: &Path, suffix: &str) -> AppResult<PathBuf> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    Ok(parent.join(format!(
        ".journal-{}-{}.{}",
        std::process::id(),
        Local::now().timestamp_nanos_opt().unwrap_or_default(),
        suffix
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn disabled_identity_path_uses_timestamped_age_filename() {
        let dir = tempdir().unwrap();
        let identity = dir.path().join("identity.age");

        let disabled = disabled_identity_path_for_timestamp(&identity, "20260702123456");

        assert_eq!(
            disabled,
            dir.path().join("identity.disabled-20260702123456.age")
        );
    }

    #[test]
    fn disabled_identity_path_adds_suffix_when_target_exists() {
        let dir = tempdir().unwrap();
        let identity = dir.path().join("identity.age");
        let base = dir.path().join("identity.disabled-20260702123456.age");
        fs::write(&base, "existing").unwrap();

        let disabled = disabled_identity_path_for_timestamp(&identity, "20260702123456");

        assert_ne!(disabled, base);
        let name = disabled.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("identity.disabled-20260702123456-"));
        assert!(name.ends_with(".age"));
    }

    #[test]
    fn disable_identity_file_renames_active_identity() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("config.toml");
        let root = dir.path().join("journals");
        let paths = crypto::EncryptionPaths::for_config(&config, &root).unwrap();
        fs::write(&paths.identity_file, "identity").unwrap();

        let disabled = disable_identity_file(&paths).unwrap();

        assert!(!paths.identity_file.exists());
        assert_eq!(fs::read_to_string(disabled).unwrap(), "identity");
    }

    #[test]
    fn unique_temp_path_is_beside_target() {
        let dir = tempdir().unwrap();
        let target = dir
            .path()
            .join("2026")
            .join("07")
            .join("02")
            .join("x.md.age");

        let temp = unique_temp_path(&target, "tmp.age").unwrap();

        assert_eq!(temp.parent(), target.parent());
    }

    #[test]
    fn encrypted_entries_in_trash_are_detected_for_migration_safety() {
        let dir = tempdir().unwrap();
        let encrypted_trash = dir
            .path()
            .join("work")
            .join(".trash")
            .join("2026")
            .join("07")
            .join("02")
            .join("old.md.age");
        fs::create_dir_all(encrypted_trash.parent().unwrap()).unwrap();
        fs::write(encrypted_trash, "ciphertext").unwrap();

        assert!(workspace_has_encrypted_entry_files(dir.path()).unwrap());
    }
}
