use super::codec::EntryCodec;
use super::create::create_entry_file;
use super::paths::entry_path_with_id;
use super::*;
use jiff::Zoned;
use notema_encryption::{self as crypto, KeyPaths};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

/// A fixed-offset instant at the given wall-clock time. The offset is irrelevant
/// to these tests — only the wall-clock components (which drive the date folder
/// and filename) are asserted — and a fixed offset is never ambiguous, so there
/// is no DST disambiguation to handle.
fn local_time(y: i16, m: i8, d: i8, h: i8, min: i8) -> Zoned {
    jiff::civil::date(y, m, d)
        .at(h, min, 0, 0)
        .to_zoned(jiff::tz::TimeZone::UTC)
        .unwrap()
}

fn create_test_entry(
    codec: &EntryCodec<'_>,
    root: &Path,
    journal: &str,
    body: &str,
    metadata: &Metadata,
) -> PathBuf {
    create_entry(
        codec,
        root,
        EntryDraft::new(journal, body, metadata),
        EntryAssetOptions::default(),
    )
    .unwrap()
    .path
}

#[test]
fn entry_path_uses_year_month_day_folder_and_datetime_short_id_filename() {
    let dir = tempdir().unwrap();
    let now = local_time(2026, 7, 1, 23, 30);

    let path = entry_path(dir.path(), "work", &now);

    assert!(path.starts_with(dir.path().join("work").join("2026").join("07").join("01")));
    let stem = path.file_stem().unwrap().to_str().unwrap();
    let short_id = stem.strip_prefix("2026-07-01T23-30-00-").unwrap();
    assert_eq!(short_id.len(), 4);
    assert!(short_id.chars().all(|ch| ch.is_ascii_alphanumeric()));
}

#[test]
fn journal_sidecar_is_not_collected_as_an_entry() {
    let dir = tempdir().unwrap();
    let codec = EntryCodec::plain();
    create_test_entry(
        &codec,
        dir.path(),
        "work",
        "# Hi\nbody",
        &Metadata::default(),
    );
    // The per-journal sidecar sits directly in the journal folder; the entry
    // walker must skip it (hidden name) and never treat it as an entry.
    fs::write(
        dir.path().join("work").join(".journal.toml"),
        "schema_version = 1\nid = \"abcd1234\"\n",
    )
    .unwrap();

    let paths = collect_entry_paths(dir.path()).unwrap();
    assert_eq!(paths.len(), 1);
    assert!(paths[0].path.extension().is_some_and(|ext| ext == "md"));
}

#[test]
fn create_entry_file_retries_without_overwriting_existing_path() {
    let dir = tempdir().unwrap();
    let now = local_time(2026, 7, 1, 23, 30);
    let existing = entry_path_with_id(dir.path(), "work", &now, "existing");
    fs::create_dir_all(existing.parent().unwrap()).unwrap();
    fs::write(&existing, "keep me").unwrap();
    let mut ids = ["existing", "fresh"].into_iter();

    let created = create_entry_file(
        &EntryCodec::plain(),
        dir.path(),
        "work",
        &now,
        "new content",
        || ids.next().unwrap().to_string(),
    )
    .unwrap();

    assert_eq!(
        created,
        entry_path_with_id(dir.path(), "work", &now, "fresh")
    );
    assert_eq!(fs::read_to_string(existing).unwrap(), "keep me");
    assert_eq!(fs::read_to_string(created).unwrap(), "new content");
}

#[test]
fn create_entry_writes_body_after_front_matter() {
    let dir = tempdir().unwrap();

    let created = create_test_entry(
        &EntryCodec::plain(),
        dir.path(),
        "work",
        "Some text",
        &Metadata::default(),
    );
    let text = fs::read_to_string(created).unwrap();
    let (front_matter, body) = crate::markdown::split_front_matter(&text);
    let fields = crate::markdown::front_matter_fields(front_matter.unwrap());

    assert!(fields.datetime.created_at.is_some());
    assert!(fields.datetime.edited_at.is_some());
    assert!(fields.metadata.tags.is_empty());
    // A native entry captures this machine's IANA zone name, when resolvable.
    assert_eq!(
        fields.datetime.timezone,
        jiff::tz::TimeZone::system().iana_name().map(str::to_string)
    );
    assert_eq!(body.trim_start_matches('\n'), "Some text\n");
}

#[test]
fn create_entry_preserves_multiline_body_and_trailing_newline() {
    let dir = tempdir().unwrap();

    let created = create_test_entry(
        &EntryCodec::plain(),
        dir.path(),
        "work",
        "Line one\n\nLine three\n",
        &Metadata::default(),
    );
    let text = fs::read_to_string(created).unwrap();

    assert!(text.ends_with("\n\nLine three\n"));
    assert!(!text.ends_with("\n\nLine three\n\n"));
}

#[test]
fn save_entry_edit_reports_changed_unchanged_and_deleted() {
    let dir = tempdir().unwrap();
    let codec = EntryCodec::plain();
    let path = create_test_entry(
        &codec,
        dir.path(),
        "work",
        "original body\n",
        &Metadata::default(),
    );
    let metadata = Metadata::default();

    // Saving the same body is not a change and does not rewrite timestamps.
    let before = fs::read_to_string(&path).unwrap();
    let saved = save_entry_edit(
        &codec,
        &path,
        EntryEdit {
            body: "original body\n",
            metadata: &metadata,
            original_metadata: &metadata,
            writing_seconds: None,
            remove_if_empty: true,
            extra_fields: &[],
        },
        EntryAssetOptions::default(),
    )
    .unwrap();
    assert_eq!(saved.outcome, EditOutcome::Unchanged);
    assert_eq!(fs::read_to_string(&path).unwrap(), before);

    // A different body is a change.
    let saved = save_entry_edit(
        &codec,
        &path,
        EntryEdit {
            body: "new body\n",
            metadata: &metadata,
            original_metadata: &metadata,
            writing_seconds: Some(30),
            remove_if_empty: true,
            extra_fields: &[],
        },
        EntryAssetOptions::default(),
    )
    .unwrap();
    assert_eq!(saved.outcome, EditOutcome::Changed);
    let text = fs::read_to_string(&path).unwrap();
    let front_matter = crate::markdown::split_front_matter(&text).0.unwrap();
    assert_eq!(
        crate::markdown::front_matter_fields(front_matter)
            .datetime
            .writing_seconds,
        Some(30)
    );

    // Emptying it deletes the entry.
    let saved = save_entry_edit(
        &codec,
        &path,
        EntryEdit {
            body: "   ",
            metadata: &metadata,
            original_metadata: &metadata,
            writing_seconds: None,
            remove_if_empty: true,
            extra_fields: &[],
        },
        EntryAssetOptions::default(),
    )
    .unwrap();
    assert_eq!(saved.outcome, EditOutcome::Deleted);
    assert!(!path.exists());
}

#[test]
fn save_entry_edit_preserves_unparseable_front_matter() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("2026-07-06T10-00-00.md");
    let original = "+++\nschema_version = 1\ntags = [unterminated\n+++\n\nold body\n";
    fs::write(&path, original).unwrap();
    let metadata = Metadata::default();

    save_entry_edit(
        &EntryCodec::plain(),
        &path,
        EntryEdit {
            body: "new body\n",
            metadata: &metadata,
            original_metadata: &metadata,
            writing_seconds: Some(12),
            remove_if_empty: true,
            extra_fields: &[],
        },
        EntryAssetOptions::default(),
    )
    .unwrap();

    let written = fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("tags = [unterminated"),
        "metadata preserved"
    );
    assert!(written.contains("new body"), "body updated");
}

/// The byte-preserving branch must keep the source's CRLF line endings rather
/// than splicing lone LF delimiters into an otherwise-CRLF file.
#[test]
fn save_entry_edit_preserves_crlf_line_endings_for_unparseable_front_matter() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("2026-07-06T10-00-00.md");
    let original = "+++\r\nschema_version = 1\r\ntags = [unterminated\r\n+++\r\n\r\nold body\r\n";
    fs::write(&path, original).unwrap();
    let metadata = Metadata::default();

    save_entry_edit(
        &EntryCodec::plain(),
        &path,
        EntryEdit {
            body: "new body\r\n",
            metadata: &metadata,
            original_metadata: &metadata,
            writing_seconds: Some(12),
            remove_if_empty: true,
            extra_fields: &[],
        },
        EntryAssetOptions::default(),
    )
    .unwrap();

    let written = fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("tags = [unterminated"),
        "metadata preserved"
    );
    assert!(written.contains("new body"), "body updated");
    let bytes = written.as_bytes();
    let bare_lf = written
        .char_indices()
        .any(|(i, c)| c == '\n' && (i == 0 || bytes[i - 1] != b'\r'));
    assert!(
        !bare_lf,
        "no lone LF injected into a CRLF file: {written:?}"
    );
    assert!(written.starts_with("+++\r\n"), "CRLF delimiter preserved");
}

#[test]
fn entry_id_and_journal_come_from_path_not_front_matter() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("id-from-file.md");
    fs::write(
        &path,
        "+++\nschema_version = 1\nid = \"wrong\"\njournal = \"wrong\"\n\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n+++\n\n# Title\n",
    )
    .unwrap();

    let entry = read_entry("folder-name", &path, None).unwrap();

    assert_eq!(entry.id, "id-from-file");
    assert_eq!(entry.journal, "folder-name");
}

#[test]
fn entry_preview_collapses_body_with_markdown_stripped() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("entry.md");
    fs::write(
        &path,
        "+++\nschema_version = 1\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n+++\n\n# Hi how is it going?\nThis is a test entry\n",
    )
    .unwrap();

    let entry = read_entry("journal", &path, None).unwrap();

    assert_eq!(entry.preview, "Hi how is it going? This is a test entry");
}

#[test]
fn entry_tags_read_toml_list() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("entry.md");
    fs::write(
        &path,
        "+++\nschema_version = 1\n\n[entry]\ntags = [\"work\", \"deep focus\"]\n+++\n\n# Tagged\n",
    )
    .unwrap();

    let entry = read_entry("journal", &path, None).unwrap();

    assert_eq!(entry.tags, vec!["work", "deep focus"]);
}

#[test]
fn entry_feelings_read_known_values_only() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("entry.md");
    fs::write(
        &path,
        "+++\nschema_version = 1\n\n[entry]\nfeelings = [\"Calm\", \"nope\", \"focused\"]\n+++\n\n# Feeling\n",
    )
    .unwrap();

    let entry = read_entry("journal", &path, None).unwrap();

    assert_eq!(entry.feelings, vec!["calm", "focused"]);
}

#[test]
fn create_entry_writes_metadata() {
    let dir = tempdir().unwrap();
    let tags = vec!["rust".to_string()];
    let people = vec!["alex".to_string()];
    let activities = vec!["programming".to_string(), "cycling".to_string()];
    let feelings = vec!["calm".to_string(), "focused".to_string()];

    let created = create_test_entry(
        &EntryCodec::plain(),
        dir.path(),
        "work",
        "Some text",
        &Metadata {
            tags: tags.clone(),
            people: people.clone(),
            activities: activities.clone(),
            feelings: feelings.clone(),
            mood: None,
            starred: false,
            location: None,
        },
    );
    let text = fs::read_to_string(created).unwrap();
    let (front_matter, _) = crate::markdown::split_front_matter(&text);

    let fields = front_matter.map(crate::markdown::front_matter_fields);
    assert_eq!(fields.as_ref().map(|f| f.metadata.tags.clone()), Some(tags));
    assert_eq!(
        fields.as_ref().map(|f| f.metadata.people.clone()),
        Some(people)
    );
    assert_eq!(
        fields.as_ref().map(|f| f.metadata.activities.clone()),
        Some(activities)
    );
    assert_eq!(
        fields.as_ref().map(|f| f.metadata.feelings.clone()),
        Some(feelings)
    );
    assert!(text.ends_with("\nSome text\n"));
}

#[test]
fn create_entry_writes_metadata_location() {
    let dir = tempdir().unwrap();
    let created = create_test_entry(
        &EntryCodec::plain(),
        dir.path(),
        "work",
        "Some text",
        &Metadata {
            location: Some(notema_domain::Location {
                name: Some("Cafe".to_string()),
                latitude: Some(52.52),
                longitude: Some(13.405),
                ..notema_domain::Location::default()
            }),
            ..Metadata::default()
        },
    );

    let text = fs::read_to_string(created).unwrap();
    let (front_matter, _) = crate::markdown::split_front_matter(&text);
    let fields = front_matter.map(crate::markdown::front_matter_fields);

    assert_eq!(
        fields
            .as_ref()
            .and_then(|fields| fields.location.as_ref())
            .and_then(|location| location.name.as_deref()),
        Some("Cafe")
    );
}

#[test]
fn plain_entry_preview_is_the_whole_body() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("entry.md");
    fs::write(
        &path,
        "+++\nschema_version = 1\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n+++\n\nPlain title\nPlain preview\n",
    )
    .unwrap();

    let entry = read_entry("journal", &path, None).unwrap();

    assert_eq!(entry.preview, "Plain title Plain preview");
    assert_eq!(entry.display_label(), "Plain title Plain preview");
}

#[test]
fn empty_entry_preview_is_empty_and_label_falls_back_to_timestamp() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("entry.md");
    fs::write(
        &path,
        "+++\nschema_version = 1\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n+++\n\n",
    )
    .unwrap();

    let entry = read_entry("journal", &path, None).unwrap();

    assert_eq!(entry.preview, "");
    assert_eq!(entry.display_label(), "2026-07-01T10:00:00+02:00");
}

#[test]
fn scan_entries_skips_trash() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026").join("07").join("01");
    let trash_dir = dir
        .path()
        .join("work")
        .join(".trash")
        .join("2026")
        .join("07")
        .join("01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::create_dir_all(&trash_dir).unwrap();
    fs::write(
        entry_dir.join("entry.md"),
        "+++\nschema_version = 1\n+++\n\n# Active\n",
    )
    .unwrap();
    fs::write(
        trash_dir.join("trashed.md"),
        "+++\nschema_version = 1\n+++\n\n# Trashed\n",
    )
    .unwrap();

    let entries = scan_entries(dir.path(), None).unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].preview, "Active");
}

/// A locked store must return placeholders without opening ciphertext it cannot
/// use — otherwise every launch reads the whole library and throws it away.
/// Making the file unreadable is the only way to prove no read happened.
#[test]
#[cfg(unix)]
fn scan_entries_does_not_read_locked_encrypted_entries() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let path = dir
        .path()
        .join("work/2026/07/01/2026-07-01T10-23-00-secret.md.age");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "ciphertext no one may read").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(&path).is_ok() {
        return; // Running as root, where the probe cannot work.
    }

    let entries = scan_entries(dir.path(), None).unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].encryption_state,
        EntryEncryptionState::EncryptedLocked
    );
}

/// The same rule on the content read, which the import scanner walks the whole
/// tree with. An unreadable file proves the locked check runs before the read.
#[test]
#[cfg(unix)]
fn read_entry_content_does_not_read_locked_encrypted_entries() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let path = dir
        .path()
        .join("work/2026/07/01/2026-07-01T10-23-00-secret.md.age");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "ciphertext no one may read").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(&path).is_ok() {
        return; // Running as root, where the probe cannot work.
    }

    let error = read_entry_content(&path, None).unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<crate::EncryptionError>(),
            Some(crate::EncryptionError::Locked { .. })
        ),
        "expected a locked-store error rather than an IO error, got: {error}"
    );

    assert!(scan_import_sources(dir.path(), None).unwrap().is_empty());
}

/// The deliberate asymmetry with the scan path above: asking for a revision
/// means asking for a digest of the stored ciphertext, so this one does read the
/// file even when the entry itself can only be a placeholder.
#[test]
fn read_entry_with_revision_hashes_locked_ciphertext() {
    let dir = tempdir().unwrap();
    let path = dir
        .path()
        .join("work/2026/07/01/2026-07-01T10-23-00-secret.md.age");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "ciphertext this device cannot open").unwrap();

    let (entry, revision) = read_entry_with_revision("work", &path, None).unwrap();

    assert_eq!(
        entry.encryption_state,
        EntryEncryptionState::EncryptedLocked
    );
    assert_eq!(revision, crate::EntryRevision::read(&path).unwrap());
}

#[test]
fn scan_entries_returns_locked_placeholder_for_encrypted_entry_without_key() {
    let dir = tempdir().unwrap();
    let path = dir
        .path()
        .join("work")
        .join("2026")
        .join("07")
        .join("01")
        .join("2026-07-01T10-23-00-secret.md.age");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "not decrypted during locked scans").unwrap();

    let entries = scan_entries(dir.path(), None).unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].encryption_state,
        EntryEncryptionState::EncryptedLocked
    );
    assert_eq!(entries[0].preview, "[locked] Encrypted entry");
    assert_eq!(entries[0].body, "Encryption identity not available");
    assert_eq!(
        notema_domain::entry_group_date(&entries[0]),
        Some(jiff::civil::date(2026, 7, 1))
    );
}

/// One unreadable plaintext entry must degrade to a placeholder rather than
/// failing the whole scan, so a reload still surfaces every other entry and the
/// failure is reported.
#[test]
fn scan_degrades_unreadable_plaintext_entry_and_reports_it() {
    let dir = tempdir().unwrap();
    let day = dir.path().join("work").join("2026").join("07").join("01");
    fs::create_dir_all(&day).unwrap();
    fs::write(
        day.join("2026-07-01T09-00-00-good1.md"),
        "+++\nschema_version = 1\n+++\n\n# Readable\n",
    )
    .unwrap();
    // Invalid UTF-8 fails the plaintext decode the same way a torn write would.
    let bad = day.join("2026-07-01T10-00-00-bad00.md");
    fs::write(&bad, [0xff, 0xfe, 0x00, 0xff]).unwrap();

    let (entries, failures) = scan_entries_with_failures(dir.path(), None).unwrap();

    assert_eq!(entries.len(), 2, "the good entry still loads");
    let unreadable = entries
        .iter()
        .find(|entry| entry.encryption_state == EntryEncryptionState::Unreadable)
        .expect("the bad entry degrades to an unreadable placeholder");
    assert_eq!(unreadable.preview, "[unreadable] Entry");
    assert_eq!(failures.len(), 1, "the failure is reported");
    assert!(
        failures[0].contains("bad00"),
        "the failure names the offending file, got: {}",
        failures[0]
    );
}

#[test]
fn scan_entries_marks_encrypted_entry_unlocked_with_identity() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let root = dir.path().join("journals");
    let paths = KeyPaths::for_config(&config, &root).unwrap();
    crypto::initialize_store_identity(&paths, "laptop", Some(&crate::SecretString::from("secret")))
        .unwrap();
    let encrypted = create_test_entry(
        &EntryCodec::new(paths.clone(), None),
        &root,
        "work",
        "# Secret\nBody",
        &Metadata::default(),
    );
    let identity =
        crypto::unlock_identity(&paths, Some(&crate::SecretString::from("secret"))).unwrap();

    let entries = scan_entries(&root, Some(&identity)).unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, encrypted);
    assert_eq!(
        entries[0].encryption_state,
        EntryEncryptionState::EncryptedUnlocked
    );
    assert_eq!(entries[0].preview, "Secret Body");
    assert!(entries[0].body.contains("Body"));
}

#[test]
fn scan_entries_marks_corrupt_encrypted_entry_unreadable_with_identity() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let root = dir.path().join("journals");
    let paths = KeyPaths::for_config(&config, &root).unwrap();
    crypto::initialize_store_identity(&paths, "laptop", Some(&crate::SecretString::from("secret")))
        .unwrap();
    let encrypted = create_test_entry(
        &EntryCodec::new(paths.clone(), None),
        &root,
        "work",
        "# Secret\nBody",
        &Metadata::default(),
    );
    fs::write(&encrypted, "not an age file").unwrap();
    let identity =
        crypto::unlock_identity(&paths, Some(&crate::SecretString::from("secret"))).unwrap();

    let entries = scan_entries(&root, Some(&identity)).unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].encryption_state,
        EntryEncryptionState::EncryptedUnreadable
    );
    assert_eq!(entries[0].preview, "[unreadable] Encrypted entry");
    assert_eq!(entries[0].body, "Encrypted entry could not be decrypted");
}

#[test]
fn delete_moves_entry_to_journal_trash() {
    let dir = tempdir().unwrap();
    let path = dir
        .path()
        .join("work")
        .join("2026")
        .join("07")
        .join("01")
        .join("id.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "body").unwrap();

    let trash = move_entry_to_trash(dir.path(), &path).unwrap();

    assert_eq!(
        trash,
        dir.path()
            .join(".trash")
            .join("work")
            .join("2026")
            .join("07")
            .join("01")
            .join("id.md")
    );
    assert!(trash.exists());
    assert!(!path.exists());
}

#[test]
fn delete_journal_trashes_the_whole_directory_including_unrecognized_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let journal = root.join("work");
    let day = journal.join("2026").join("07").join("01");
    fs::create_dir_all(&day).unwrap();
    fs::write(
        day.join("2026-07-01T10-00-00-abc12.md"),
        "+++\nschema_version = 1\n+++\n\nbody\n",
    )
    .unwrap();
    // A file the app doesn't manage must survive the deletion.
    fs::write(journal.join("NOTES.txt"), "hand-written").unwrap();

    delete_journal(root, "work", &journal).unwrap();

    assert!(!journal.exists(), "the journal directory is moved out");
    let trashed = root.join(".trash").join("work");
    assert!(
        trashed.join("NOTES.txt").exists(),
        "unrecognized file preserved rather than deleted"
    );
    assert!(
        trashed
            .join("2026")
            .join("07")
            .join("01")
            .join("2026-07-01T10-00-00-abc12.md")
            .exists(),
        "entry preserved in trash"
    );
}

#[test]
fn delete_journal_disambiguates_an_occupied_trash_slot() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    // A prior deletion already parked `work` in the trash.
    fs::create_dir_all(root.join(".trash").join("work")).unwrap();
    let journal = root.join("work");
    fs::create_dir_all(&journal).unwrap();
    fs::write(journal.join("keep.txt"), "second copy").unwrap();

    delete_journal(root, "work", &journal).unwrap();

    assert!(!journal.exists());
    assert!(
        root.join(".trash").join("work-1").join("keep.txt").exists(),
        "the second deletion lands beside the first, not over it"
    );
}

#[test]
fn delete_prunes_empty_day_month_year_dirs() {
    let dir = tempdir().unwrap();
    let day = dir.path().join("work").join("2026").join("07").join("01");
    fs::create_dir_all(&day).unwrap();
    let path = day.join("id.md");
    fs::write(&path, "body").unwrap();

    move_entry_to_trash(dir.path(), &path).unwrap();

    assert!(!day.exists());
    assert!(!dir.path().join("work").join("2026").join("07").exists());
    assert!(!dir.path().join("work").join("2026").exists());
    // The journal folder is kept even when empty.
    assert!(dir.path().join("work").exists());
}

#[test]
fn delete_keeps_dirs_with_surviving_siblings() {
    let dir = tempdir().unwrap();
    let month = dir.path().join("work").join("2026").join("07");
    let day_one = month.join("01");
    let day_two = month.join("02");
    fs::create_dir_all(&day_one).unwrap();
    fs::create_dir_all(&day_two).unwrap();
    let path = day_one.join("id.md");
    fs::write(&path, "body").unwrap();
    fs::write(day_two.join("other.md"), "body").unwrap();

    move_entry_to_trash(dir.path(), &path).unwrap();

    // The emptied day is gone, but its non-empty month/year and sibling day stay.
    assert!(!day_one.exists());
    assert!(day_two.exists());
    assert!(month.exists());
    assert!(dir.path().join("work").join("2026").exists());
}

#[test]
fn delete_prunes_month_but_keeps_year_with_other_months() {
    let dir = tempdir().unwrap();
    let year = dir.path().join("work").join("2026");
    let july_day = year.join("07").join("01");
    let august_day = year.join("08").join("01");
    fs::create_dir_all(&july_day).unwrap();
    fs::create_dir_all(&august_day).unwrap();
    let path = july_day.join("id.md");
    fs::write(&path, "body").unwrap();
    fs::write(august_day.join("other.md"), "body").unwrap();

    move_entry_to_trash(dir.path(), &path).unwrap();

    assert!(!year.join("07").exists());
    assert!(year.join("08").exists());
    assert!(year.exists());
}

#[test]
fn delete_prunes_dirs_holding_only_os_junk() {
    let dir = tempdir().unwrap();
    let year = dir.path().join("work").join("2026");
    let month = year.join("07");
    let day = month.join("01");
    fs::create_dir_all(&day).unwrap();
    let path = day.join("id.md");
    fs::write(&path, "body").unwrap();
    // Finder/Explorer droppings scattered up the date tree must not block pruning.
    fs::write(day.join(".DS_Store"), b"finder").unwrap();
    fs::write(day.join("._id.md"), b"appledouble").unwrap();
    fs::write(month.join("Thumbs.db"), b"win").unwrap();
    fs::write(year.join(".DS_Store"), b"finder").unwrap();

    move_entry_to_trash(dir.path(), &path).unwrap();

    assert!(!day.exists());
    assert!(!month.exists());
    assert!(!year.exists());
    assert!(dir.path().join("work").exists());
}

#[test]
fn delete_keeps_dir_with_unrecognized_file() {
    let dir = tempdir().unwrap();
    let day = dir.path().join("work").join("2026").join("07").join("01");
    fs::create_dir_all(&day).unwrap();
    let path = day.join("id.md");
    fs::write(&path, "body").unwrap();
    // An unknown file is treated as real content — the folder must survive.
    fs::write(day.join("notes.txt"), b"keep me").unwrap();

    move_entry_to_trash(dir.path(), &path).unwrap();

    assert!(day.exists());
    assert!(day.join("notes.txt").exists());
}

#[test]
fn delete_empty_entry_prunes_empty_date_dirs() {
    let dir = tempdir().unwrap();
    let day = dir.path().join("work").join("2026").join("07").join("01");
    fs::create_dir_all(&day).unwrap();
    let path = day.join("id.md");
    fs::write(&path, "").unwrap();

    delete_empty_entry(dir.path(), &path).unwrap();

    assert!(!path.exists());
    assert!(!day.exists());
    assert!(!dir.path().join("work").join("2026").join("07").exists());
    assert!(!dir.path().join("work").join("2026").exists());
    assert!(dir.path().join("work").exists());
}

#[test]
fn delete_relocates_entry_asset_folder_to_trash() {
    let dir = tempdir().unwrap();
    let day = dir.path().join("work").join("2026").join("07").join("01");
    fs::create_dir_all(&day).unwrap();
    let path = day.join("id.md");
    fs::write(&path, "body").unwrap();
    let assets = day.join("id.assets");
    fs::create_dir_all(&assets).unwrap();
    fs::write(assets.join("x9.png"), b"img").unwrap();

    move_entry_to_trash(dir.path(), &path).unwrap();

    let trashed_assets = dir
        .path()
        .join(".trash")
        .join("work")
        .join("2026")
        .join("07")
        .join("01")
        .join("id.assets");
    assert!(trashed_assets.join("x9.png").exists());
    assert!(!assets.exists());
}

#[test]
fn delete_does_not_move_entry_when_asset_trash_destination_exists() {
    let dir = tempdir().unwrap();
    let day = dir.path().join("work").join("2026").join("07").join("01");
    fs::create_dir_all(&day).unwrap();
    let path = day.join("id.md");
    fs::write(&path, "body").unwrap();
    let assets = day.join("id.assets");
    fs::create_dir_all(&assets).unwrap();
    fs::write(assets.join("x9.png"), b"img").unwrap();
    let trashed_assets = dir
        .path()
        .join(".trash")
        .join("work")
        .join("2026")
        .join("07")
        .join("01")
        .join("id.assets");
    fs::create_dir_all(&trashed_assets).unwrap();

    let error = move_entry_to_trash(dir.path(), &path).unwrap_err();

    assert!(matches!(
        error.downcast_ref::<crate::StorageError>(),
        Some(crate::StorageError::TargetExists {
            what: "asset trash destination",
            ..
        })
    ));
    assert!(path.exists());
    assert!(assets.join("x9.png").exists());
}

/// Seed `<stem>.assets/keep.png` and point `entry`'s body at it.
fn seed_referenced_asset(entry: &Path) -> PathBuf {
    let assets = super::paths::entry_assets_dir(entry).unwrap();
    fs::create_dir_all(&assets).unwrap();
    let kept = assets.join("keep.png");
    fs::write(&kept, b"kept image bytes").unwrap();
    kept
}

#[test]
fn save_entry_edit_rolls_back_staged_assets_when_the_content_cannot_be_rendered() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("2026-07-06T10-00-00.md");
    let assets_dir_name = "2026-07-06T10-00-00.assets";
    // Unparseable front matter plus a metadata change makes `render_edited_content`
    // bail — after ingest has already run, which is the window under test.
    fs::write(
        &path,
        format!(
            "+++\nschema_version = 1\ntags = [unterminated\n+++\n\n![k]({assets_dir_name}/keep.png)\n"
        ),
    )
    .unwrap();
    let kept = seed_referenced_asset(&path);
    let incoming = dir.path().join("new.png");
    fs::write(&incoming, png_test_bytes()).unwrap();

    let original_metadata = Metadata::default();
    let metadata = Metadata {
        tags: vec!["added".to_string()],
        ..Metadata::default()
    };

    let error = save_entry_edit(
        &EntryCodec::plain(),
        &path,
        EntryEdit {
            // Drops the `keep.png` reference, so cleanup would prune it.
            body: &format!("![new]({})\n", incoming.display()),
            metadata: &metadata,
            original_metadata: &original_metadata,
            writing_seconds: None,
            remove_if_empty: true,
            extra_fields: &[],
        },
        EntryAssetOptions::default(),
    )
    .unwrap_err();

    assert!(error.to_string().contains("front matter"));
    assert!(kept.exists(), "asset the on-disk entry still references");
    let remaining: Vec<_> = fs::read_dir(kept.parent().unwrap())
        .unwrap()
        .map(|item| item.unwrap().file_name())
        .collect();
    assert_eq!(remaining, ["keep.png"], "nothing staged was left behind");
}

#[test]
fn create_entry_copy_rolls_back_cloned_assets_when_ingest_fails() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("work/2026/07/01/source.md");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(
        &source,
        "+++\nschema_version = 1\n+++\n\n![p](source.assets/photo.png)\n",
    )
    .unwrap();
    let source_assets = dir.path().join("work/2026/07/01/source.assets");
    fs::create_dir_all(&source_assets).unwrap();
    fs::write(source_assets.join("photo.png"), png_test_bytes()).unwrap();

    // A roster file that exists but does not parse: `encrypts_new_entries` is true,
    // so ingest resolves recipients and fails — after `clone_entry_assets` has
    // already copied the source's assets into the new entry's folder.
    //
    // This injector depends on `EncryptionRecipients::for_store` being resolved
    // inside ingest. If recipient resolution ever moves out to the callers, this
    // test has to move with it.
    let age_dir = dir.path().join(".age");
    fs::create_dir_all(&age_dir).unwrap();
    let devices = age_dir.join("devices.toml");
    fs::write(&devices, "this is not valid toml {{{").unwrap();
    let codec = EntryCodec::new(
        KeyPaths {
            age_dir: age_dir.clone(),
            devices_file: devices,
            identity_file: age_dir.join("identity.age"),
            trust_file: age_dir.join("trust.toml"),
        },
        None,
    );
    let metadata = Metadata::default();

    let result = create_entry_copy(
        &codec,
        dir.path(),
        &source,
        EntryDraft::new("work", "![p](source.assets/photo.png)\n", &metadata),
        EntryAssetOptions::default(),
    );

    assert!(result.is_err(), "ingest fails on the broken roster");
    assert!(
        source_assets.join("photo.png").exists(),
        "source assets untouched"
    );
    // The copy is dated now, not from the source, so look across the journal.
    let leaked = assets_dirs_under(&dir.path().join("work"));
    assert_eq!(
        leaked,
        [source_assets],
        "the failed copy left its cloned asset folder behind"
    );
}

fn png_test_bytes() -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend_from_slice(&[0u8; 16]);
    bytes
}

/// A rewrite the entry cache's stamp is blind to: same byte length, mtime put
/// back. A save must not depend on that stamp, so the conflict is still caught.
#[test]
fn save_entry_edit_if_revision_detects_a_same_length_same_mtime_rewrite() {
    let dir = tempdir().unwrap();
    let path = dir
        .path()
        .join("work/2026/07/01/2026-07-01T10-00-00-abcd.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let head =
        "+++\nschema_version = 1\n\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n+++\n\n";
    fs::write(&path, format!("{head}original\n")).unwrap();
    let before = fs::metadata(&path).unwrap();
    let revision = crate::EntryRevision::read(&path).unwrap();

    fs::write(&path, format!("{head}replaced\n")).unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(before.modified().unwrap())
        .unwrap();

    let after = fs::metadata(&path).unwrap();
    assert_eq!(after.len(), before.len(), "the test rewrite changed length");
    assert_eq!(
        after.modified().unwrap(),
        before.modified().unwrap(),
        "the test rewrite left a different mtime"
    );

    let metadata = Metadata::default();
    let error = save_entry_edit_if_revision(
        &EntryCodec::plain(),
        &path,
        revision,
        EntryEdit {
            body: "mine\n",
            metadata: &metadata,
            original_metadata: &metadata,
            writing_seconds: None,
            remove_if_empty: false,
            extra_fields: &[],
        },
        EntryAssetOptions::default(),
    )
    .unwrap_err();

    assert!(
        matches!(
            error.downcast_ref::<crate::StorageError>(),
            Some(crate::StorageError::EntryRevisionConflict { .. })
        ),
        "expected a revision conflict, got: {error}"
    );
    assert!(
        fs::read_to_string(&path).unwrap().ends_with("replaced\n"),
        "the refused save overwrote the other process's write"
    );
}

/// The revision check that guards a save brackets asset ingest: once on the
/// bytes the save opened, and again just before the write. Ingest is where a
/// save spends real time, because it downloads remote images — so a write
/// landing in that window is the reachable conflict. Serve the image from a
/// local socket that rewrites the entry the moment the download starts, to land
/// inside the window on purpose.
#[test]
fn save_entry_edit_if_revision_conflict_during_ingest_keeps_on_disk_assets() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let dir = tempdir().unwrap();
    let path = dir
        .path()
        .join("work/2026/07/01/2026-07-01T10-00-00-abcd.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        "+++\nschema_version = 1\n\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n+++\n\n![k](2026-07-01T10-00-00-abcd.assets/keep.png)\n",
    )
    .unwrap();
    let kept = seed_referenced_asset(&path);
    let revision = crate::EntryRevision::read(&path).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let entry = path.clone();
    let server = std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            // The reachability probe connects and drops without sending; ignore it
            // and keep waiting for the real request.
            let mut head = [0u8; 1];
            if stream.read(&mut head).unwrap_or(0) == 0 {
                continue;
            }
            // Inside the window: change the entry's length, so the stamp differs
            // whatever the filesystem's mtime granularity is.
            fs::write(&entry, "replaced by another process, and rather longer\n").unwrap();
            let body = png_test_bytes();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&body);
            let _ = stream.flush();
            return;
        }
    });

    let metadata = Metadata::default();
    let error = save_entry_edit_if_revision(
        &EntryCodec::plain(),
        &path,
        revision,
        EntryEdit {
            // Drops the `keep.png` reference, so cleanup would prune it.
            body: &format!("![shot](http://127.0.0.1:{port}/pic.png)\n"),
            metadata: &metadata,
            original_metadata: &metadata,
            writing_seconds: None,
            remove_if_empty: true,
            extra_fields: &[],
        },
        EntryAssetOptions {
            download_remote: true,
            replace_offline: false,
        },
    )
    .unwrap_err();
    server.join().unwrap();

    assert!(
        matches!(
            error.downcast_ref::<crate::StorageError>(),
            Some(crate::StorageError::EntryRevisionConflict { .. })
        ),
        "expected a revision conflict, got: {error}"
    );
    assert!(
        kept.exists(),
        "the refused save destroyed an asset the on-disk entry references"
    );
    let remaining: Vec<_> = fs::read_dir(kept.parent().unwrap())
        .unwrap()
        .map(|item| item.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(
        remaining,
        ["keep.png"],
        "the downloaded asset was rolled back"
    );
}

/// Every `<stem>.assets` directory under `root`, sorted, for leak assertions.
fn assets_dirs_under(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(items) = fs::read_dir(&dir) else {
            continue;
        };
        for item in items.flatten() {
            let path = item.path();
            if !path.is_dir() {
                continue;
            }
            if super::paths::is_assets_dir(&path) {
                found.push(path);
            } else {
                pending.push(path);
            }
        }
    }
    found.sort();
    found
}
