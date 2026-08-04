use notema_domain::{Entry, Metadata};
use notema_encryption::SecretString;
use notema_storage::JournalStore;
use std::{
    env, fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
use tempfile::tempdir;

fn journal_bin() -> &'static str {
    env!("CARGO_BIN_EXE_notema")
}

fn write_config(path: &Path, root: &Path, default_journal: Option<&str>) {
    let mut journal = toml::map::Map::new();
    journal.insert(
        "path".to_string(),
        toml::Value::String(root.to_string_lossy().into_owned()),
    );
    if let Some(default) = default_journal {
        journal.insert(
            "default".to_string(),
            toml::Value::String(default.to_string()),
        );
    }
    let mut config = toml::map::Map::new();
    config.insert("schema_version".to_string(), toml::Value::Integer(1));
    config.insert("journal".to_string(), toml::Value::Table(journal));
    fs::write(path, toml::to_string(&config).unwrap()).unwrap();
}

fn scan_entries_for(root: &Path, journal: &str) -> Vec<Entry> {
    let store = JournalStore::for_config(&root.join("config.toml"), root).unwrap();
    let mut entries = store.scan_entries().unwrap();
    entries.retain(|entry| entry.journal == journal);
    entries
}

fn generate_identity_store(config: &Path, root: &Path, passphrase: &str) -> (JournalStore, String) {
    let store = JournalStore::for_config(config, root).unwrap();
    let recipient = store
        .initialize_encryption("laptop", Some(&SecretString::from(passphrase)))
        .unwrap();
    (store, recipient)
}

fn create_entry(store: &JournalStore, journal: &str, body: &str) -> std::path::PathBuf {
    store
        .create_entry(
            notema_storage::EntryDraft::new(journal, body, &Metadata::default()),
            notema_storage::EntryAssetOptions::default(),
        )
        .unwrap()
        .path
}

fn png_bytes() -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend_from_slice(&[0u8; 16]);
    bytes
}

fn age_cli_available() -> bool {
    Command::new("age").arg("--version").output().is_ok()
        && Command::new("age-keygen").arg("--version").output().is_ok()
}

/// Pull this device's age secret key out of its plaintext `identity.toml` so the
/// standard `age` CLI can decrypt what the journal wrote. The key material is
/// bundled inside the file; the `AGE-SECRET-KEY-…` bech32 string is unambiguous
/// to slice out without depending on the internal serialization.
fn extract_age_secret(identity_text: &str) -> String {
    let start = identity_text
        .find("AGE-SECRET-KEY-")
        .expect("identity file has no age secret key");
    identity_text[start..]
        .chars()
        .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '-')
        .collect()
}

#[test]
fn log_command_creates_entry_in_default_journal() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, Some("work"));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .arg("log")
        .arg("Some text")
        .output()
        .unwrap();

    assert!(output.status.success());
    let entries = scan_entries_for(&root, "work");
    assert_eq!(entries.len(), 1);
    assert!(entries[0].body.contains("Some text"));
}

#[test]
fn log_command_ingests_local_image_asset() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, Some("work"));
    let image = dir.path().join("photo.png");
    fs::write(&image, png_bytes()).unwrap();

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .arg("log")
        .arg(image.to_string_lossy().as_ref())
        .output()
        .unwrap();

    assert!(output.status.success());
    let created = Path::new(std::str::from_utf8(&output.stdout).unwrap().trim());
    let content = fs::read_to_string(created).unwrap();
    let stem = notema_storage::entry_id(created).unwrap();
    let assets = created.parent().unwrap().join(format!("{stem}.assets"));

    assert!(content.contains(&format!("![]({stem}.assets/")));
    assert_eq!(fs::read_dir(&assets).unwrap().count(), 1);
}

#[test]
fn log_command_writes_tags() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, Some("work"));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .arg("log")
        .arg("--tag")
        .arg("rust")
        .arg("--tag")
        .arg("open source")
        .arg("Some text")
        .output()
        .unwrap();

    assert!(output.status.success());
    let entries = scan_entries_for(&root, "work");
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].tags,
        vec!["rust".to_string(), "open source".to_string()]
    );
}

#[test]
fn log_command_accepts_comma_separated_tags() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, Some("work"));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .arg("log")
        .arg("--tag")
        .arg("rust,open source")
        .arg("Some text")
        .output()
        .unwrap();

    assert!(output.status.success());
    let entries = scan_entries_for(&root, "work");
    assert_eq!(
        entries[0].tags,
        vec!["rust".to_string(), "open source".to_string()]
    );
}

#[test]
fn log_command_writes_people_and_activities() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, Some("work"));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .arg("log")
        .arg("--person")
        .arg("alex,sam")
        .arg("--activity")
        .arg("programming")
        .arg("--activity")
        .arg("cycling")
        .arg("Some text")
        .output()
        .unwrap();

    assert!(output.status.success());
    let entries = scan_entries_for(&root, "work");
    assert_eq!(
        entries[0].people,
        vec!["alex".to_string(), "sam".to_string()]
    );
    assert_eq!(
        entries[0].activities,
        vec!["programming".to_string(), "cycling".to_string()]
    );
}

#[test]
fn log_command_accepts_comma_separated_feelings() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, Some("work"));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .arg("log")
        .arg("--feeling")
        .arg("calm,focused")
        .arg("Some text")
        .output()
        .unwrap();

    assert!(output.status.success());
    let entries = scan_entries_for(&root, "work");
    assert_eq!(
        entries[0].feelings,
        vec!["calm".to_string(), "focused".to_string()]
    );
}

#[test]
fn log_command_writes_repeatable_feelings() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, Some("work"));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .arg("log")
        .arg("--feeling")
        .arg("Calm")
        .arg("--feeling")
        .arg("focused")
        .arg("Some text")
        .output()
        .unwrap();

    assert!(output.status.success());
    let entries = scan_entries_for(&root, "work");
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].feelings,
        vec!["calm".to_string(), "focused".to_string()]
    );
}

#[test]
fn log_command_rejects_unknown_feeling() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, Some("work"));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .arg("log")
        .arg("--feeling")
        .arg("sparkly")
        .arg("Some text")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown feeling 'sparkly'"));
    assert!(scan_entries_for(&root, "work").is_empty());
}

#[test]
fn piped_log_command_creates_entry_in_default_journal() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, Some("work"));

    let mut child = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .arg("log")
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
    let entries = scan_entries_for(&root, "work");
    assert_eq!(entries.len(), 1);
    assert!(entries[0].body.contains("Line one\n\nLine three"));
}

// `notema log` with no body now opens the interactive fullscreen editor, which
// can't be driven from a headless subprocess, so that path is exercised by the
// in-process editor tests in `src/tui/` rather than here.

#[test]
fn bare_text_is_rejected() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, Some("work"));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .arg("Some text")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(scan_entries_for(&root, "work").is_empty());
}

#[test]
fn bare_piped_stdin_requires_log_command() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, Some("work"));

    let mut child = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
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
    assert!(String::from_utf8_lossy(&output.stderr).contains("notema log"));
    assert!(scan_entries_for(&root, "work").is_empty());
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
        .arg(config.parent().unwrap())
        .arg("log")
        .arg("--journal")
        .arg("personal")
        .arg("Override text")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(scan_entries_for(&root, "work").is_empty());
    let entries = scan_entries_for(&root, "personal");
    assert_eq!(entries.len(), 1);
    assert!(entries[0].body.contains("Override text"));
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
        .arg(config_path.parent().unwrap())
        .arg("use")
        .arg("work")
        .output()
        .unwrap();

    assert!(output.status.success());
    let config: toml::Value = toml::from_str(&fs::read_to_string(config_path).unwrap()).unwrap();
    assert_eq!(config["journal"]["default"].as_str(), Some("work"));
}

#[test]
fn set_default_journal_keeps_a_relative_journal_root_raw() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    fs::create_dir_all(dir.path().join("journals").join("work")).unwrap();
    write_config(&config_path, Path::new("journals"), None);

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config_path.parent().unwrap())
        .arg("use")
        .arg("work")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = fs::read_to_string(config_path).unwrap();
    assert!(
        text.contains("path = \"journals\""),
        "saving rewrote the portable journal root: {text}"
    );
    let config: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(config["journal"]["default"].as_str(), Some("work"));
}

#[test]
fn log_command_without_default_or_journal_fails() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, None);

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .arg("log")
        .arg("Some text")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no journal specified"));
    assert!(scan_entries_for(&root, "work").is_empty());
}

#[test]
fn log_command_rejects_text_and_piped_stdin_together() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, Some("work"));

    let mut child = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .arg("log")
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
    assert!(scan_entries_for(&root, "work").is_empty());
}

#[test]
fn encrypt_command_converts_store_and_entry_command_writes_encrypted_files() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    let (mut store, _recipient) = generate_identity_store(&config, &root, "secret");
    let entry_dir = root.join("work").join("2026").join("07").join("02");
    let trash_dir = root
        .join("work")
        .join(".trash")
        .join("2026")
        .join("07")
        .join("01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::create_dir_all(&trash_dir).unwrap();
    let entry = entry_dir.join("entry.md");
    let trashed = trash_dir.join("old.md");
    fs::write(
        &entry,
        "+++\nschema_version = 1\ntags = []\n+++\n\n# Secret\nBody\n",
    )
    .unwrap();
    fs::write(
        &trashed,
        "+++\nschema_version = 1\ntags = []\n+++\n\n# Trashed\n",
    )
    .unwrap();
    write_config(&config, &root, Some("work"));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .args(["encryption", "enable"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let encrypted_entry = entry_dir.join("entry.md.age");
    let encrypted_trash = trash_dir.join("old.md.age");
    assert!(!entry.exists());
    assert!(encrypted_entry.exists());
    assert!(encrypted_trash.exists());
    store.unlock(Some(&SecretString::from("secret"))).unwrap();
    assert!(
        store
            .read_entry_content(&encrypted_entry)
            .unwrap()
            .contains("# Secret")
    );
    assert_eq!(
        store
            .device_roster_path()
            .file_name()
            .and_then(|name| name.to_str()),
        Some("devices.toml")
    );
    assert_eq!(
        store.device_roster_path(),
        root.join(".age").join("devices.toml")
    );
    assert_eq!(store.identity_path(), dir.path().join("identity.toml"));
    assert!(store.device_roster_path().exists());
    assert!(store.identity_path().exists());
    assert!(!dir.path().join("encryption").exists());
    assert!(!fs::read_dir(dir.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("backup")
    }));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .arg("log")
        .arg("--journal")
        .arg("work")
        .arg("New encrypted body")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let created = Path::new(std::str::from_utf8(&output.stdout).unwrap().trim()).to_path_buf();
    assert!(
        created
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".md.age")
    );
    assert!(
        store
            .read_entry_content(&created)
            .unwrap()
            .contains("New encrypted body")
    );
}

#[test]
fn encrypt_command_can_be_rerun_when_store_is_already_encrypted() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    let (mut store, _recipient) = generate_identity_store(&config, &root, "secret");
    let encrypted = create_entry(&store, "work", "# Secret\nBody");
    write_config(&config, &root, Some("work"));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .args(["encryption", "enable"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(encrypted.exists());
    store.unlock(Some(&SecretString::from("secret"))).unwrap();
    assert!(
        store
            .read_entry_content(&encrypted)
            .unwrap()
            .contains("# Secret")
    );
    assert!(!fs::read_dir(dir.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("backup")
    }));
}

#[test]
fn encrypt_command_finishes_partial_encryption_without_touching_existing_age_files() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    let (mut store, _recipient) = generate_identity_store(&config, &root, "secret");
    let existing_encrypted = create_entry(&store, "work", "# Existing");
    let entry_dir = root.join("work").join("2026").join("07").join("02");
    fs::create_dir_all(&entry_dir).unwrap();
    let remaining_plain = entry_dir.join("remaining.md");
    fs::write(
        &remaining_plain,
        "+++\nschema_version = 1\ntags = []\n+++\n\n# Remaining\n",
    )
    .unwrap();
    write_config(&config, &root, Some("work"));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .args(["encryption", "enable"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let remaining_encrypted = entry_dir.join("remaining.md.age");
    assert!(existing_encrypted.exists());
    assert!(!remaining_plain.exists());
    assert!(remaining_encrypted.exists());
    store.unlock(Some(&SecretString::from("secret"))).unwrap();
    assert!(
        store
            .read_entry_content(&existing_encrypted)
            .unwrap()
            .contains("# Existing")
    );
    assert!(
        store
            .read_entry_content(&remaining_encrypted)
            .unwrap()
            .contains("# Remaining")
    );
}

#[test]
fn encrypt_command_fails_when_plain_entry_target_age_file_already_exists() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    let (_store, _recipient) = generate_identity_store(&config, &root, "secret");
    let entry_dir = root.join("work").join("2026").join("07").join("02");
    fs::create_dir_all(&entry_dir).unwrap();
    let plain = entry_dir.join("entry.md");
    let encrypted = entry_dir.join("entry.md.age");
    fs::write(
        &plain,
        "+++\nschema_version = 1\ntags = []\n+++\n\n# Plain\n",
    )
    .unwrap();
    fs::write(&encrypted, "# Existing encrypted\n").unwrap();
    write_config(&config, &root, Some("work"));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .args(["encryption", "enable"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("target already exists"));
    assert!(plain.exists());
    assert!(encrypted.exists());
}

#[test]
fn encrypt_command_fails_when_encrypted_entries_exist_without_device_roster() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    let (store, _recipient) = generate_identity_store(&config, &root, "secret");
    let encrypted = create_entry(&store, "work", "# Secret");
    fs::remove_file(store.device_roster_path()).unwrap();
    write_config(&config, &root, Some("work"));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .args(["encryption", "enable"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("device roster is missing"));
    assert!(encrypted.exists());
    assert!(!store.device_roster_path().exists());
}

#[test]
fn encrypt_command_fails_when_recipients_exist_but_device_has_no_identity() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    // Recipients synced from another device, but this one never enrolled.
    let (store, _recipient) = generate_identity_store(&config, &root, "secret");
    fs::remove_file(store.identity_path()).unwrap();
    write_config(&config, &root, Some("work"));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .args(["encryption", "enable"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("device enroll"));
}

#[test]
fn encrypted_entry_command_writes_age_files_without_unlocking() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    let (mut store, _recipient) = generate_identity_store(&config, &root, "secret");
    store.unlock(Some(&SecretString::from("secret"))).unwrap();
    fs::remove_file(store.identity_path()).unwrap();
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, Some("work"));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .arg("log")
        .arg("--journal")
        .arg("work")
        .arg("age readable body")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let encrypted = Path::new(std::str::from_utf8(&output.stdout).unwrap().trim()).to_path_buf();
    assert!(
        encrypted
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".md.age")
    );
    let decrypted = store.read_entry_content(&encrypted).unwrap();

    assert!(decrypted.contains("age readable body"));
}

#[test]
fn encrypted_log_command_writes_age_files_without_unlocking() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    let (mut store, _recipient) = generate_identity_store(&config, &root, "secret");
    store.unlock(Some(&SecretString::from("secret"))).unwrap();
    // Remove the identity so the CLI has no way to decrypt — a new entry must
    // still encrypt to the roster, proving writing needs only the recipients.
    fs::remove_file(store.identity_path()).unwrap();
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, Some("work"));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .arg("log")
        .arg("# Encrypted editor body")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let encrypted = Path::new(std::str::from_utf8(&output.stdout).unwrap().trim()).to_path_buf();
    assert!(
        encrypted
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".md.age")
    );
    let decrypted = store.read_entry_content(&encrypted).unwrap();

    assert!(decrypted.contains("# Encrypted editor body"));
}

#[test]
fn encrypted_entries_can_be_decrypted_with_age_cli() {
    if !age_cli_available() {
        return;
    }

    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    // A normal no-passphrase device: its own age key is the sole recipient, so the
    // standard age CLI can decrypt what the journal writes to prove it is real age.
    let store = JournalStore::for_config(&config, &root).unwrap();
    store.initialize_encryption("laptop", None).unwrap();
    write_config(&config, &root, Some("work"));

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .arg("log")
        .arg("--journal")
        .arg("work")
        .arg("age CLI readable body")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let encrypted = Path::new(std::str::from_utf8(&output.stdout).unwrap().trim()).to_path_buf();

    let identity = dir.path().join("age-identity.txt");
    let secret = extract_age_secret(&fs::read_to_string(store.identity_path()).unwrap());
    fs::write(&identity, format!("{secret}\n")).unwrap();

    let output = Command::new("age")
        .arg("--decrypt")
        .arg("--identity")
        .arg(&identity)
        .arg(&encrypted)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let decrypted = String::from_utf8(output.stdout).unwrap();

    assert!(decrypted.contains("age CLI readable body"));
}

/// A passphrase-protected identity stores its key material as standalone age
/// ASCII armor — a real age file the `age` CLI can decrypt in an emergency — and
/// never leaks the secret key in cleartext.
#[test]
fn passphrase_identity_stores_recoverable_age_armor() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    let (store, _) = generate_identity_store(&config, &root, "correct horse battery");

    let text = fs::read_to_string(store.identity_path()).unwrap();
    assert!(
        text.contains("encrypted_keys"),
        "passphrase identity should store encrypted_keys: {text}"
    );
    assert!(
        text.contains("-----BEGIN AGE ENCRYPTED FILE-----"),
        "encrypted key material should be age ASCII armor: {text}"
    );
    assert!(
        !text.contains("AGE-SECRET-KEY-"),
        "the age secret key must not appear in cleartext when passphrase-protected"
    );
}

/// Run the journal binary against `config` and assert success, returning stdout.
fn run_ok(config: &Path, args: &[&str]) -> String {
    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn key_workflow_grants_second_device_history_access() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    fs::create_dir_all(root.join("work")).unwrap();
    // Two config dirs sharing one journal root simulate two devices; each keeps
    // its own identity next to its config.
    let laptop_cfg = dir.path().join("laptop/config.toml");
    let phone_cfg = dir.path().join("phone/config.toml");
    fs::create_dir_all(laptop_cfg.parent().unwrap()).unwrap();
    fs::create_dir_all(phone_cfg.parent().unwrap()).unwrap();
    write_config(&laptop_cfg, &root, Some("work"));
    write_config(&phone_cfg, &root, Some("work"));

    // Laptop enables encryption and writes an entry before the phone exists.
    run_ok(
        &laptop_cfg,
        &[
            "encryption",
            "enable",
            "--name",
            "laptop",
            "--no-passphrase",
        ],
    );
    run_ok(&laptop_cfg, &["log", "--journal", "work", "secret history"]);

    // Phone requests access; laptop lists it pending, then approves it.
    run_ok(
        &phone_cfg,
        &[
            "encryption",
            "device",
            "enroll",
            "--name",
            "phone",
            "--no-passphrase",
        ],
    );
    let listing = run_ok(&laptop_cfg, &["encryption", "device", "list"]);
    assert!(listing.contains("Pending approval"), "{listing}");
    assert!(listing.contains("phone"), "{listing}");
    run_ok(&laptop_cfg, &["encryption", "device", "approve", "--all"]);

    // After re-encryption the phone can read the entry written before it joined.
    let mut phone = JournalStore::for_config(&phone_cfg, &root).unwrap();
    phone.unlock(None).unwrap();
    let entries = phone.scan_entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].body.contains("secret history"));

    // Both devices are recipients; the pending request is cleared.
    assert_eq!(phone.recipients().unwrap().len(), 2);
    assert!(phone.pending_requests().unwrap().is_empty());
}

#[test]
fn empty_xdg_config_home_is_treated_as_unset() {
    let home = tempdir().unwrap();

    // With XDG_CONFIG_HOME empty (spec: counts as unset) the config path must
    // fall back under HOME, not resolve relative to an empty base.
    let output = Command::new(journal_bin())
        .args(["use", "work"])
        .env("XDG_CONFIG_HOME", "")
        .env("HOME", home.path())
        .env_remove("NOTEMA_CONFIG")
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let marker = "config file not found at ";
    let start = stderr.find(marker).unwrap_or_else(|| panic!("{stderr}"));
    let path = &stderr[start + marker.len()..];
    assert!(
        path.starts_with(&*home.path().to_string_lossy()),
        "config path should be under HOME: {stderr}"
    );
}

#[test]
fn first_run_setup_refuses_without_a_terminal() {
    let dir = tempdir().unwrap();

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(dir.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("stdin is not a terminal"), "{stderr}");
    assert!(!dir.path().join("config.toml").exists());
    assert!(!dir.path().join("Journals").exists());
}

#[test]
fn approve_rejects_a_named_request_together_with_all() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("config.toml");
    fs::create_dir_all(dir.path().join("journals").join("work")).unwrap();
    write_config(&config, &dir.path().join("journals"), None);

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .args(["encryption", "device", "approve", "phone", "--all"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot be used with"), "{stderr}");
}

#[test]
fn enable_refuses_to_overwrite_an_existing_device_key() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("config.toml");

    // A device key from a prior enrol lives beside config.toml; its roster and
    // trust pins belong to a different (unsynced) journal. Drop the trust pins so
    // startup's disabled-encryption reconcile leaves the identity in place — the
    // real "waiting to join" state where the roster simply hasn't synced yet.
    let roster_root = dir.path().join("other-journal");
    fs::create_dir_all(&roster_root).unwrap();
    generate_identity_store(&config, &roster_root, "secret passphrase");
    fs::remove_file(dir.path().join("devices-trust.toml")).unwrap();
    let identity = dir.path().join("identity.toml");
    let identity_before = fs::read(&identity).unwrap();

    // Point the config at a fresh plaintext journal that has no roster.
    let root = dir.path().join("journals");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, None);

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .args([
            "encryption",
            "enable",
            "--name",
            "laptop",
            "--no-passphrase",
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("device key already exists"), "{stderr}");
    assert_eq!(
        fs::read(&identity).unwrap(),
        identity_before,
        "enable overwrote the existing device key"
    );
}

#[test]
fn disable_on_a_plaintext_store_fails_before_confirming() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, None);

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .args(["encryption", "disable"])
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not encrypted"), "{stderr}");
    // Validation must run before the confirmation prompt, and a plaintext
    // store must not be pointed at enroll.
    assert!(!stderr.contains("refusing to continue"), "{stderr}");
    assert!(!stderr.contains("enroll"), "{stderr}");
}

#[test]
fn revoke_on_a_plaintext_store_fails_before_confirming() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, None);

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .args(["encryption", "device", "revoke", "phone"])
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not encrypted"), "{stderr}");
    assert!(!stderr.contains("refusing to continue"), "{stderr}");
}

#[test]
fn revoke_of_an_unknown_device_fails_before_confirming() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, None);
    generate_identity_store(&config, &root, "secret passphrase");

    let output = Command::new(journal_bin())
        .arg("--config")
        .arg(config.parent().unwrap())
        .args(["encryption", "device", "revoke", "ghost"])
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no device named 'ghost'"), "{stderr}");
    assert!(!stderr.contains("refusing to continue"), "{stderr}");
}

#[test]
fn encrypt_decrypt_converts_assets_and_keeps_clean_links() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config = dir.path().join("config.toml");
    fs::create_dir_all(root.join("work")).unwrap();
    write_config(&config, &root, Some("work"));
    let image = dir.path().join("photo.png");
    fs::write(&image, png_bytes()).unwrap();

    // A plaintext entry with an ingested image; its body link is clean.
    run_ok(
        &config,
        &["log", "--journal", "work", image.to_string_lossy().as_ref()],
    );
    let plain = JournalStore::for_config(&config, &root).unwrap();
    let body = plain.scan_entries().unwrap().remove(0).body;
    assert!(
        body.contains(".assets/") && !body.contains(".age"),
        "{body}"
    );

    // Encrypt without a passphrase.
    run_ok(&config, &["encryption", "enable", "--no-passphrase"]);
    let mut enc = JournalStore::for_config(&config, &root).unwrap();
    assert!(!enc.identity_needs_passphrase().unwrap());
    enc.unlock(None).unwrap();
    let entry = enc.scan_entries().unwrap().remove(0);
    // The body is byte-for-byte unchanged (link still clean) though it's encrypted.
    assert_eq!(entry.body, body);
    assert!(entry.path.to_string_lossy().ends_with(".md.age"));

    // The asset on disk is now `.age`, and the clean link still resolves+decrypts.
    let stem = notema_storage::entry_id(&entry.path).unwrap();
    let assets_dir = entry.path.parent().unwrap().join(format!("{stem}.assets"));
    let asset = fs::read_dir(&assets_dir).unwrap().next().unwrap().unwrap();
    let asset_name = asset.file_name().into_string().unwrap();
    assert!(
        asset_name.ends_with(".age"),
        "asset encrypted: {asset_name}"
    );
    let clean = asset_name.strip_suffix(".age").unwrap();
    assert!(
        enc.read_entry_asset_bytes(&entry.path, clean)
            .unwrap()
            .is_some()
    );

    // Decrypt (plaintext identity → no unlock prompt): asset returns to plaintext, body unchanged.
    run_ok(&config, &["encryption", "disable", "--yes"]);
    let dec = JournalStore::for_config(&config, &root).unwrap();
    let dec_entry = dec.scan_entries().unwrap().remove(0);
    assert_eq!(dec_entry.body, body);
    let dec_stem = notema_storage::entry_id(&dec_entry.path).unwrap();
    let dec_assets = dec_entry
        .path
        .parent()
        .unwrap()
        .join(format!("{dec_stem}.assets"));
    let dec_asset = fs::read_dir(&dec_assets).unwrap().next().unwrap().unwrap();
    assert!(
        !dec_asset.file_name().to_string_lossy().ends_with(".age"),
        "asset plaintext again"
    );
}

/// Run a `notema` subcommand against `config_dir` with no stdin, the way a
/// script would.
fn run_notema(config_dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(journal_bin())
        .arg("--config")
        .arg(config_dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

/// An encrypted store with one entry and a passphrase-less key.
///
/// `--key-store file` is explicit rather than relying on the default: these
/// tests run the real binary, so `notema-encryption` is built without
/// `cfg(test)` and its keyring stub is not in play. Any test that reached the
/// keychain would write to the developer's own login keychain and leave the item
/// behind.
fn encrypted_store_with_an_entry(dir: &Path) -> std::path::PathBuf {
    let config = dir.join("config.toml");
    let root = dir.join("journals");
    fs::create_dir_all(root.join("diary")).unwrap();
    write_config(&config, &root, Some("diary"));

    assert!(
        run_notema(
            dir,
            &[
                "encryption",
                "enable",
                "--name",
                "laptop",
                "--no-passphrase",
                "--key-store",
                "file",
            ]
        )
        .status
        .success()
    );
    assert!(run_notema(dir, &["log", "a secret entry"]).status.success());
    dir.join("identity.toml")
}

#[cfg(unix)]
#[test]
fn key_store_command_takes_the_key_out_of_the_identity_file() {
    let dir = tempdir().unwrap();
    let identity = encrypted_store_with_an_entry(dir.path());
    let vault = dir.path().join("vault.toml");

    let output = run_notema(
        dir.path(),
        &[
            "encryption",
            "key",
            "store",
            "command",
            "--read",
            &format!("cat {}", vault.display()),
            "--write",
            &format!("cat > {}", vault.display()),
        ],
    );
    assert!(output.status.success(), "{:?}", output);

    // The whole point of the feature: no key left on disk where it was.
    let on_disk = fs::read_to_string(&identity).unwrap();
    assert!(!on_disk.contains("AGE-SECRET-KEY-"), "{on_disk}");
    assert!(on_disk.contains("keys_command"), "{on_disk}");
    assert!(
        fs::read_to_string(&vault)
            .unwrap()
            .contains("AGE-SECRET-KEY-"),
        "the key should have landed in the vault instead"
    );

    // And the store is still usable through it.
    assert!(
        run_notema(dir.path(), &["log", "written via the command"])
            .status
            .success()
    );
}

#[cfg(unix)]
#[test]
fn rotate_writes_the_new_key_back_through_the_store_command() {
    let dir = tempdir().unwrap();
    let identity = encrypted_store_with_an_entry(dir.path());
    let vault = dir.path().join("vault.toml");
    run_notema(
        dir.path(),
        &[
            "encryption",
            "key",
            "store",
            "command",
            "--read",
            &format!("cat {}", vault.display()),
            "--write",
            &format!("cat > {}", vault.display()),
        ],
    );
    let before = fs::read_to_string(&vault).unwrap();

    let output = run_notema(dir.path(), &["encryption", "key", "rotate"]);
    assert!(output.status.success(), "{output:?}");

    // The new key went back out to the vault, and stayed out of identity.toml.
    let after = fs::read_to_string(&vault).unwrap();
    assert!(after.contains("AGE-SECRET-KEY-"));
    assert_ne!(
        after, before,
        "rotation should have replaced the stored key"
    );
    assert!(
        !fs::read_to_string(&identity)
            .unwrap()
            .contains("AGE-SECRET-KEY-")
    );
    // And the rotated key still opens the store.
    assert!(
        run_notema(dir.path(), &["log", "after rotating"])
            .status
            .success()
    );
}

#[cfg(unix)]
#[test]
fn rotate_refuses_a_key_command_that_cannot_write() {
    let dir = tempdir().unwrap();
    let identity = encrypted_store_with_an_entry(dir.path());
    let vault = dir.path().join("vault.toml");
    run_notema(
        dir.path(),
        &[
            "encryption",
            "key",
            "store",
            "command",
            "--read",
            &format!("cat {}", vault.display()),
            "--write",
            &format!("cat > {}", vault.display()),
        ],
    );
    // Drop the store command, leaving a key that can be read but not replaced.
    let text = fs::read_to_string(&identity).unwrap();
    fs::write(
        &identity,
        text.lines()
            .filter(|line| !line.starts_with("keys_store_command"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let before = fs::read(&identity).unwrap();

    for args in [
        vec!["encryption", "key", "rotate"],
        vec!["encryption", "key", "passphrase"],
    ] {
        let output = run_notema(dir.path(), &args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{args:?} should have refused");
        assert!(stderr.contains("--write"), "{stderr}");
    }
    assert_eq!(
        fs::read(&identity).unwrap(),
        before,
        "a refused command must not rewrite the identity file"
    );
}

#[cfg(unix)]
#[test]
fn a_broken_key_command_is_reported_as_such_not_as_corruption() {
    let dir = tempdir().unwrap();
    let identity = encrypted_store_with_an_entry(dir.path());
    let vault = dir.path().join("vault.toml");
    run_notema(
        dir.path(),
        &[
            "encryption",
            "key",
            "store",
            "command",
            "--read",
            &format!("cat {}", vault.display()),
            "--write",
            &format!("cat > {}", vault.display()),
        ],
    );
    fs::remove_file(&vault).unwrap();

    let output = run_notema(dir.path(), &["encryption", "key", "store", "file"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    // Naming the command and its complaint is the difference between "fix your
    // vault path" and a user thinking their key is corrupt.
    assert!(stderr.contains("key command"), "{stderr}");
    assert!(stderr.contains("No such file"), "{stderr}");
    assert!(!stderr.contains("malformed"), "{stderr}");
    assert!(
        fs::read_to_string(&identity)
            .unwrap()
            .contains("keys_command")
    );
}

#[test]
fn an_exported_key_restores_into_a_fresh_config_dir() {
    let dir = tempdir().unwrap();
    encrypted_store_with_an_entry(dir.path());
    let backup = dir.path().join("backup.toml");

    let output = run_notema(
        dir.path(),
        &[
            "encryption",
            "key",
            "export",
            backup.to_str().unwrap(),
            "-y",
        ],
    );
    assert!(output.status.success(), "{:?}", output);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&backup).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "an exported key must not be readable by others"
        );
    }

    // Restoring is copying the file back — there is no import command.
    let restored = dir.path().join("restored");
    fs::create_dir_all(&restored).unwrap();
    write_config(
        &restored.join("config.toml"),
        &dir.path().join("journals"),
        Some("diary"),
    );
    fs::copy(&backup, restored.join("identity.toml")).unwrap();

    let output = run_notema(&restored, &["encryption", "status"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{:?}", output);
    assert!(stdout.contains("identity file"), "{stdout}");
    // It can actually read the store, not merely parse.
    assert!(
        run_notema(&restored, &["log", "written after restoring"])
            .status
            .success()
    );
}

/// Without a terminal to ask on, a new key stays in the identity file.
///
/// A script, container, or provisioning run cannot answer a prompt, and a
/// keychain chosen on its behalf may be unreachable in the session that has to
/// open the key. It is also what keeps this very suite off the developer's own
/// login keychain.
#[test]
fn a_non_interactive_enable_keeps_the_key_in_the_identity_file() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    fs::create_dir_all(root.join("diary")).unwrap();
    write_config(&dir.path().join("config.toml"), &root, Some("diary"));

    // No --key-store: the default is what is under test.
    let output = run_notema(
        dir.path(),
        &[
            "encryption",
            "enable",
            "--name",
            "laptop",
            "--no-passphrase",
        ],
    );
    assert!(output.status.success(), "{output:?}");

    let identity = fs::read_to_string(dir.path().join("identity.toml")).unwrap();
    assert!(
        identity.contains("AGE-SECRET-KEY-"),
        "the key should still be in the identity file: {identity}"
    );
    assert!(
        !identity.contains("keyring_account"),
        "no terminal to ask on means no keychain: {identity}"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Identity file:"),
        "backup advice should name the file the key is actually in: {stdout}"
    );
}

/// `status` answers "what is my encryption state" in one place: whether it is
/// on, where this device's key is, and who else can read the journal.
#[test]
fn status_reports_encryption_state_key_location_and_roster() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    fs::create_dir_all(root.join("diary")).unwrap();
    write_config(&dir.path().join("config.toml"), &root, Some("diary"));

    // A plaintext journal must still get an answer, not an error.
    let plaintext = run_notema(dir.path(), &["encryption", "status"]);
    assert!(plaintext.status.success(), "{plaintext:?}");
    let before = String::from_utf8_lossy(&plaintext.stdout);
    assert!(before.contains("Encryption is off"), "{before}");

    encrypted_store_with_an_entry(dir.path());

    let output = run_notema(dir.path(), &["encryption", "status"]);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Encryption is on"), "{stdout}");
    assert!(stdout.contains("identity file"), "{stdout}");
    assert!(stdout.contains("opens automatically"), "{stdout}");
    assert!(stdout.contains("laptop"), "roster should appear: {stdout}");
    assert!(stdout.contains("(this device)"), "{stdout}");
}

/// `--read` and `--write` describe a fetch command, so naming them for a
/// location that does not fetch is a mistake worth reporting rather than
/// silently dropping.
#[test]
fn key_store_rejects_fetch_flags_that_do_not_apply() {
    let dir = tempdir().unwrap();
    encrypted_store_with_an_entry(dir.path());

    let output = run_notema(
        dir.path(),
        &["encryption", "key", "store", "file", "--read", "cat /x"],
    );
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("only apply to `command`"), "{stderr}");
}

/// A roster that will not verify is part of the encryption state, so `status`
/// reports it instead of abandoning the report. The local half — that encryption
/// is on, and where this device's key is — stays knowable and is exactly what
/// someone in this situation needs.
#[test]
fn status_reports_an_unverifiable_roster_instead_of_failing() {
    let dir = tempdir().unwrap();
    encrypted_store_with_an_entry(dir.path());

    let roster = dir
        .path()
        .join("journals")
        .join(".age")
        .join("devices.toml");
    let tampered = fs::read_to_string(&roster)
        .unwrap()
        .replacen("laptop", "swapped", 1);
    fs::write(&roster, tampered).unwrap();

    let output = run_notema(dir.path(), &["encryption", "status"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "status should report, not fail: {output:?}"
    );
    assert!(stdout.contains("Encryption is on"), "{stdout}");
    assert!(stdout.contains("identity file"), "{stdout}");
    assert!(stdout.contains("Device roster: cannot be read"), "{stdout}");
    // Naming the recipients would contradict the line above: they are exactly
    // what could not be verified.
    assert!(
        !stdout.contains("Recipients:"),
        "an unverifiable roster must not be listed as though it were trusted: {stdout}"
    );
}
