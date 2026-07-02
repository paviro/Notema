use super::paths::ENTRY_ID_LEN;
use crate::{AppResult, crypto, markdown::set_front_matter_value};
use chrono::Local;
use nanoid::nanoid;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn open_editor(editor: &str, path: &Path) -> AppResult<()> {
    let mut parts = shell_words::split(editor)?;
    if parts.is_empty() {
        return Err("editor command is empty".into());
    }

    let program = parts.remove(0);
    let status = Command::new(program).args(parts).arg(path).status()?;
    if !status.success() {
        return Err(format!("editor exited with status {status}").into());
    }
    Ok(())
}

pub fn set_updated_at_now(path: &Path) -> AppResult<()> {
    let content = fs::read_to_string(path)?;
    let updated = set_front_matter_value(&content, "updated_at", &Local::now().to_rfc3339());
    fs::write(path, updated)?;
    Ok(())
}

pub fn edit_encrypted_entry(
    path: &Path,
    editor: &str,
    paths: &crypto::EncryptionPaths,
    identity: &crypto::UnlockedIdentity,
) -> AppResult<()> {
    let temp_dir = std::env::temp_dir();
    let plaintext = unique_temp_path(&temp_dir, "edit.md");
    let encrypted = unique_temp_path(&temp_dir, "edit.age");
    let result = (|| {
        crypto::decrypt_file(identity, path, &plaintext)?;
        open_editor(editor, &plaintext)?;
        set_updated_at_now(&plaintext)?;
        crypto::encrypt_file(paths, &plaintext, &encrypted)?;
        fs::rename(&encrypted, path)?;
        Ok(())
    })();
    let _ = fs::remove_file(&plaintext);
    let _ = fs::remove_file(&encrypted);
    result
}

pub fn move_entry_to_trash(root: &Path, entry_path: &Path) -> AppResult<PathBuf> {
    let relative = entry_path.strip_prefix(root)?;
    let mut components = relative.components();
    let journal = components
        .next()
        .ok_or("entry path is missing journal component")?
        .as_os_str();
    let mut entry_relative_path = PathBuf::new();
    for component in components {
        entry_relative_path.push(component.as_os_str());
    }
    if entry_relative_path.as_os_str().is_empty() {
        return Err("entry path is missing file path after journal component".into());
    }

    let trash_path = root.join(journal).join(".trash").join(entry_relative_path);
    if let Some(parent) = trash_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(entry_path, &trash_path)?;
    Ok(trash_path)
}

fn unique_temp_path(dir: &Path, suffix: &str) -> PathBuf {
    dir.join(format!(
        ".journal-{}-{}.{}",
        std::process::id(),
        nanoid!(ENTRY_ID_LEN),
        suffix
    ))
}
