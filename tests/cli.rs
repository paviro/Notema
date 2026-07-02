use std::{
    env, fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
use tempfile::tempdir;

fn journal_bin() -> &'static str {
    env!("CARGO_BIN_EXE_journal")
}

fn write_config(path: &Path, root: &Path, default_journal: Option<&str>) {
    let mut config = journal::config::Config::new(root.to_path_buf(), "true");
    config.default_journal = default_journal.map(str::to_string);
    journal::config::save_config(path, &config).unwrap();
}

fn entry_texts(root: &Path, journal: &str) -> Vec<String> {
    let mut entries = journal::storage::scan_entries(root).unwrap();
    entries.retain(|entry| entry.journal == journal);
    entries
        .into_iter()
        .map(|entry| fs::read_to_string(entry.path).unwrap())
        .collect()
}

#[test]
fn positional_entry_command_creates_entry_in_default_journal() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, Some("work"));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(&config)
        .arg("Some text")
        .output()
        .unwrap();

    assert!(output.status.success());
    let entries = entry_texts(&root, "work");
    assert_eq!(entries.len(), 1);
    assert!(entries[0].contains("\n---\n\nSome text\n"));
}

#[test]
fn piped_entry_command_creates_entry_in_default_journal() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, Some("work"));

    let mut child = Command::new(journal_bin())
        .arg("--config")
        .arg(&config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"Line one\n\nLine three")
        .unwrap();
    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    let entries = entry_texts(&root, "work");
    assert_eq!(entries.len(), 1);
    assert!(entries[0].ends_with("Line one\n\nLine three\n"));
}

#[test]
fn journal_flag_overrides_default_journal() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    fs::create_dir_all(root.join("personal")).unwrap();
    write_config(&config, &root, Some("work"));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(&config)
        .arg("--journal")
        .arg("personal")
        .arg("Override text")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(entry_texts(&root, "work").is_empty());
    let entries = entry_texts(&root, "personal");
    assert_eq!(entries.len(), 1);
    assert!(entries[0].contains("Override text\n"));
}

#[test]
fn set_default_journal_persists_to_config() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config_path = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config_path, &root, None);

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(&config_path)
        .arg("--set-default")
        .arg("work")
        .output()
        .unwrap();

    assert!(output.status.success());
    let config = journal::config::load_config(&config_path).unwrap();
    assert_eq!(config.default_journal.as_deref(), Some("work"));
}

#[test]
fn entry_command_without_default_or_journal_fails() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, None);

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(&config)
        .arg("Some text")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no journal specified"));
    assert!(entry_texts(&root, "work").is_empty());
}

#[test]
fn entry_command_rejects_text_and_piped_stdin_together() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, Some("work"));

    let mut child = Command::new(journal_bin())
        .arg("--config")
        .arg(&config)
        .arg("Arg text")
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"Pipe text")
        .unwrap();
    let output = child.wait_with_output().unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("piped stdin"));
    assert!(entry_texts(&root, "work").is_empty());
}

#[test]
fn fake_editor_command_edits_entry_files_in_place() {
    let root = tempdir().unwrap();
    fs::create_dir_all(root.path().join("work")).unwrap();

    let script = root.path().join("fake-editor.sh");
    fs::write(
        &script,
        "#!/bin/sh\nprintf '\\n# Edited\\nBody from fake editor\\n' >> \"$1\"\n",
    )
    .unwrap();
    let chmod = Command::new("chmod")
        .arg("+x")
        .arg(&script)
        .status()
        .unwrap();
    assert!(chmod.success());

    let entry =
        journal::storage::create_entry(root.path(), "work", script.to_str().unwrap()).unwrap();
    let entry_text = fs::read_to_string(entry).unwrap();
    assert!(entry_text.contains("# Edited"));
}
