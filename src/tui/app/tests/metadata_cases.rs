//! Entry metadata dialogs seeding from the right source, and journal-level
//! metadata: archived partitioning and archiving a journal.

use super::*;

#[test]
fn begin_edit_feelings_uses_fixed_list_and_selected_entry_values() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n\n[entry]\nfeelings = [\"calm\", \"excited\"]\n+++\n\n# A\n",
    )
    .unwrap();

    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");

    app.begin_edit_feelings();

    let state = app.edit_feeling_state().unwrap();
    // Groups start collapsed: only headers are visible and the cursor rests on the
    // first one. The entry's stored feelings are preselected regardless.
    let rows = state.visible_rows();
    assert_eq!(rows.len(), FEELING_GROUPS.len());
    assert!(matches!(rows[0], FeelingRow::Header { group: 0 }));
    assert!(matches!(&state.groups[0], g if g.name == "Joy & Delight"));
    assert_eq!(state.list.selected(), Some(0));
    assert_eq!(state.selected, vec!["calm", "excited"]);
}

#[test]
fn location_dialog_seeds_from_editor_draft_not_selected_entry() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n[location]\nname = \"Home\"\nlatitude = 52.5\nlongitude = 13.4\n+++\n\n# A\n",
    )
    .unwrap();

    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");

    // Without an editor, the dialog seeds from the selected entry.
    app.begin_edit_location();
    let state = app.edit_location_state().unwrap();
    assert_eq!(state.name.as_str(), "Home");
    assert!(!state.query.is_empty());
    app.close_overlay();

    // Composing a new entry: the dialog seeds from the (empty) editor draft,
    // not the entry that happens to still be selected underneath.
    app.open_editor_for_new();
    app.begin_edit_location();
    let state = app.edit_location_state().unwrap();
    assert!(state.name.is_empty());
    assert!(state.query.is_empty());
    assert!(state.resolved.is_none());
}

#[test]
fn metadata_partitioned_excludes_archived_and_isolates_archived_only() {
    let dir = tempdir().unwrap();
    let active_dir = dir.path().join("work").join("2026-07-01");
    let archived_dir = dir.path().join("old.archived").join("2026-07-01");
    fs::create_dir_all(&active_dir).unwrap();
    fs::create_dir_all(&archived_dir).unwrap();
    fs::write(
        active_dir.join("a.md"),
        "+++\nschema_version = 1\n\n[entry]\ntags = [\"berlin\", \"shared\"]\n\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n+++\n\n# A\n",
    )
    .unwrap();
    fs::write(
        archived_dir.join("b.md"),
        "+++\nschema_version = 1\n\n[entry]\ntags = [\"wanderlust\", \"shared\"]\n\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n+++\n\n# B\n",
    )
    .unwrap();

    let app = new_app(Config::new(dir.path().to_path_buf()));
    let (active, archived_only) = app.metadata_partitioned(MetadataKind::Tags);

    let active_tags: Vec<&str> = active.iter().map(|(t, _)| t.as_str()).collect();
    assert!(active_tags.contains(&"berlin"));
    assert!(active_tags.contains(&"shared"));
    // Archived usage doesn't leak into the active list or its counts.
    assert!(!active_tags.contains(&"wanderlust"));

    // Only values living *solely* in archived journals are surfaced; "shared"
    // also appears in the active journal, so it's not archived-only.
    let archived_tags: Vec<&str> = archived_only.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(archived_tags, vec!["wanderlust"]);
}

#[test]
fn archiving_journal_renames_reorders_and_keeps_entries_resolvable() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("personal").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    write_entry(&entry_dir, "a.md", "2026-07-01T10:00:00+02:00", "# A\nbody");
    fs::create_dir_all(dir.path().join("work")).unwrap();

    let mut app = new_app(Config::new(dir.path().to_path_buf()));

    app.services
        .store
        .set_journal_archived("personal", true)
        .unwrap();
    // What `toggle_archive_selected_journal` does after the rename: the journal
    // list is re-read and the entries follow the folder in memory.
    app.reload_journal_list().unwrap();
    app.rename_journal_entries("personal", "personal.archived");

    // The directory was renamed and the journal now sorts after active ones.
    assert!(dir.path().join("personal.archived").is_dir());
    assert!(!dir.path().join("personal").exists());
    let names: Vec<&str> = app
        .library
        .journals
        .iter()
        .map(|j| j.name.as_str())
        .collect();
    assert_eq!(names, vec!["work", "personal.archived"]);

    // Its entry still resolves under the suffixed identity (the critical
    // invariant: the raw name stays the lookup key).
    app.select_journal_by_name("personal.archived");
    let selected = app.selected_journal().unwrap();
    assert!(selected.archived);
    assert_eq!(selected.display_name(), "personal");
    assert_eq!(app.selected_entries().len(), 1);
}
