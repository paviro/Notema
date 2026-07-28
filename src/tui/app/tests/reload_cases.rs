//! Loading the library and keeping it current: the cache-miss first paint,
//! targeted `refresh_paths` updates, rebuilds, and reload results landing.

use super::*;

#[test]
fn cache_miss_starts_with_live_journals_while_entries_validate() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config_path = dir.path().join("config/config.toml");
    let config = Config::new(root.clone());
    let store = JournalStore::for_config(&config_path, &root).unwrap();
    store.ensure().unwrap();
    store.create_journal("work").unwrap();
    store
        .create_entry(
            notema_storage::EntryDraft::new("work", "Body", &notema_domain::Metadata::default()),
            notema_storage::EntryAssetOptions::default(),
        )
        .unwrap();

    let (mut app, cached) =
        AppModel::new_cached(config_path, config, store, crate::tui::theme::Mode::Dark).unwrap();

    assert!(cached.is_none());
    assert_eq!(app.library.journals.len(), 1);
    assert_eq!(app.library.journals[0].name, "work");
    assert!(app.library.entries.is_empty());
    assert_eq!(app.toasts.items().len(), 1);
    assert_eq!(app.toasts.items()[0].message, "Loading journals from disk…");
    assert!(!app.expire_toasts());

    app.finish_initial_library_loading();
    assert!(app.toasts.items().is_empty());

    app.begin_manual_refresh(false);
    assert_eq!(app.toasts.items()[0].message, "Refreshing from disk…");
    assert!(!app.expire_toasts());
    app.finish_manual_refresh();
    assert!(app.toasts.items().is_empty());
}

#[test]
fn refresh_paths_updates_only_the_changed_entry() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    let a = write_entry(
        &entry_dir,
        "a.md",
        "2026-07-01T10:00:00+02:00",
        "# A\nold body",
    );
    write_entry(&entry_dir, "b.md", "2026-07-01T11:00:00+02:00", "# B\nbee");
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    assert_eq!(app.library.entries.len(), 2);

    // Edit a.md on disk, then reload just that path.
    write_entry(
        &entry_dir,
        "a.md",
        "2026-07-01T10:00:00+02:00",
        "# A\nnew body here",
    );
    app.refresh_paths(&[a]).unwrap();

    assert_eq!(app.library.entries.len(), 2);
    let updated = app.library.entry_by_id("a").unwrap();
    assert!(updated.body.contains("new body here"));
    // Precomputed word count is rebuilt from the fresh body on re-read.
    assert_eq!(updated.word_count, updated.body.split_whitespace().count());
    assert!(!updated.search_haystack.is_empty());
    // `entries` stays sorted by path (descending) so `journal_ranges` holds.
    assert!(
        app.library
            .entries
            .windows(2)
            .all(|w| w[0].path > w[1].path)
    );
    assert_eq!(app.selected_entries().len(), 2);
}

#[test]
fn refresh_paths_handles_create_and_delete() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    let a = write_entry(
        &entry_dir,
        "a.md",
        "2026-07-01T10:00:00+02:00",
        "# A\nalpha",
    );
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    assert_eq!(app.library.entries.len(), 1);

    // A newly written file is picked up by its path alone.
    let c = write_entry(&entry_dir, "c.md", "2026-07-01T12:00:00+02:00", "# C\nsea");
    app.refresh_paths(std::slice::from_ref(&c)).unwrap();
    assert_eq!(app.library.entries.len(), 2);
    assert!(app.library.entry_by_id("c").is_some());

    // Deleting the file on disk removes it on the next targeted reload.
    fs::remove_file(&a).unwrap();
    app.refresh_paths(&[a]).unwrap();
    assert_eq!(app.library.entries.len(), 1);
    assert!(app.library.entry_by_id("a").is_none());
    assert_eq!(app.selected_entries().len(), 1);
}

/// `path.exists()` and the read that follows are separate syscalls, so a file
/// can vanish or turn unreadable between them — during exactly the create/delete
/// storms this path exists to absorb. The failure is reported, but the index is
/// rebuilt first: abandoning the loop would leave the journal ranges describing
/// the vector as it was before the removal, and the next frame slices it.
#[test]
fn refresh_paths_rebuilds_the_index_even_when_a_read_fails() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    let a = write_entry(
        &entry_dir,
        "a.md",
        "2026-07-01T10:00:00+02:00",
        "# A\nalpha",
    );
    let b = write_entry(&entry_dir, "b.md", "2026-07-01T11:00:00+02:00", "# B\nbeta");
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    assert_eq!(app.library.entries.len(), 2);

    // `a` is gone; `b` is replaced by a directory, so it still `exists()` but
    // cannot be read. The removal sorts first, so it lands before the failure.
    fs::remove_file(&a).unwrap();
    fs::remove_file(&b).unwrap();
    fs::create_dir(&b).unwrap();

    assert!(app.refresh_paths(&[a, b]).is_err());
    assert_eq!(app.library.entries.len(), 1);
    assert_eq!(
        app.library.range("work"),
        Some(0..app.library.entries.len()),
        "the index must describe the entries that survived, not the ones that did not"
    );
    // The range is what the entry list slices, so a stale one panics here.
    assert_eq!(app.selected_entries().len(), 1);
}

#[test]
fn refresh_paths_falls_back_to_full_reload_for_a_new_journal() {
    let dir = tempdir().unwrap();
    let work = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&work).unwrap();
    write_entry(&work, "a.md", "2026-07-01T10:00:00+02:00", "# A\nalpha");
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");

    // A path under a brand-new journal isn't attributable to a known journal,
    // so the incremental path must fall back to a full reload that also picks
    // up the new journal in the list.
    let personal = dir.path().join("personal").join("2026-07-01");
    fs::create_dir_all(&personal).unwrap();
    let z = write_entry(&personal, "z.md", "2026-07-02T10:00:00+02:00", "# Z\nzed");
    app.refresh_paths(&[z]).unwrap();

    // The walk runs on the worker, so nothing has landed yet.
    assert!(app.library_reload.has_pending());
    assert!(app.library.entry_by_id("z").is_none());

    settle_library_reload(&mut app);

    assert!(
        app.library
            .journals
            .iter()
            .any(|journal| journal.name == "personal")
    );
    assert!(app.library.entry_by_id("z").is_some());
}

/// The hole the entry cache's stamp deliberately leaves open: a rewrite that
/// keeps the length and puts the recorded mtime back is indistinguishable from
/// no change at all. `R` is the way out of it without deleting the cache file.
#[test]
fn a_rebuild_recovers_an_entry_the_stamp_cannot_tell_changed() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("journals");
    let config_path = dir.path().join("config/config.toml");
    let config = Config::new(root.clone());
    let store = JournalStore::for_config(&config_path, &root).unwrap();
    // Without this there is no store id, so nothing the load produces is ever
    // written to the cache and the whole test would measure a cold read twice.
    store.ensure().unwrap();
    store.create_journal("work").unwrap();

    let entry_dir = root.join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    let path = write_entry(
        &entry_dir,
        "a.md",
        "2026-07-01T10:00:00+02:00",
        "# A\nalpha",
    );
    // Old enough to be trusted: a whole-second mtime within the coarse-filesystem
    // window is refused outright, which would re-read the entry and prove nothing.
    let stamp = std::time::SystemTime::now() - Duration::from_secs(3600);
    let set_mtime = |at: std::time::SystemTime| {
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(at)
            .unwrap();
    };
    set_mtime(stamp);

    let snapshot = store.load_library(CachePolicy::Normal).unwrap();
    let mut app = AppModel::new_with_snapshot(
        config_path,
        config,
        store,
        snapshot,
        crate::tui::theme::Mode::Dark,
    )
    .unwrap();
    app.select_journal_by_name("work");
    let id = app.library.entries[0].id.clone();
    assert!(app.library.entry_by_id(&id).unwrap().body.contains("alpha"));

    // Same length, same mtime, different content.
    write_entry(
        &entry_dir,
        "a.md",
        "2026-07-01T10:00:00+02:00",
        "# A\nBRAVO",
    );
    set_mtime(stamp);

    app.request_library_reload(ReloadReason::Manual { rebuild: false });
    settle_library_reload(&mut app);
    assert!(
        app.library.entry_by_id(&id).unwrap().body.contains("alpha"),
        "a stamp check cannot see this change; if it could, the test proves nothing"
    );

    app.request_library_reload(ReloadReason::Manual { rebuild: true });
    settle_library_reload(&mut app);
    assert!(app.library.entry_by_id(&id).unwrap().body.contains("BRAVO"));
    assert_eq!(
        app.toasts
            .items()
            .last()
            .map(|toast| toast.message.as_str()),
        Some("Entry cache rebuilt from 1 entry")
    );
}

#[test]
fn a_manual_refresh_shows_its_progress_until_the_walk_lands() {
    let mut app = app_with_journals(&["work"]);

    app.request_library_reload(ReloadReason::Manual { rebuild: false });

    assert!(app.library_reload.has_pending());
    assert_eq!(
        app.toasts
            .items()
            .last()
            .map(|toast| toast.message.as_str()),
        Some("Refreshing from disk…")
    );

    settle_library_reload(&mut app);

    assert!(
        !app.toasts
            .items()
            .iter()
            .any(|toast| toast.message == "Refreshing from disk…")
    );
    assert_eq!(
        app.toasts
            .items()
            .last()
            .map(|toast| toast.message.as_str()),
        Some("Refreshed from disk")
    );
}

#[test]
fn an_automatic_reload_lands_without_saying_anything() {
    let mut app = app_with_journals(&["work"]);

    app.request_library_reload(ReloadReason::Automatic);
    settle_library_reload(&mut app);

    assert!(app.toasts.items().is_empty());
}

#[test]
fn a_second_refresh_folds_into_the_walk_already_running() {
    let mut app = app_with_journals(&["work"]);

    app.request_library_reload(ReloadReason::Automatic);
    // Pressing `r` while the quiet reload runs must not start a second walk,
    // but must still be reported when the running one lands.
    app.request_library_reload(ReloadReason::Manual { rebuild: false });

    assert_eq!(
        app.queued_reload,
        Some(ReloadReason::Manual { rebuild: false })
    );

    settle_library_reload(&mut app);

    assert_eq!(
        app.toasts
            .items()
            .last()
            .map(|toast| toast.message.as_str()),
        Some("Refreshed from disk")
    );
}

#[test]
fn a_reload_built_against_a_superseded_library_is_asked_for_again() {
    let mut app = app_with_journals(&["work"]);

    app.request_library_reload(ReloadReason::Automatic);
    // Something changed the library while the walk was out; its result now
    // describes a tree the app has moved past.
    app.library_generation = app.library_generation.wrapping_add(1);
    let generation = app.library_generation;

    // Stop on the drain that consumed the stale result, before the replacement
    // walk it asks for can land.
    let deadline = Instant::now() + Duration::from_secs(10);
    while !app.apply_library_reload_results() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }

    // Nothing was installed — installing bumps the generation — and a fresh
    // walk is out against the library now in hand.
    assert_eq!(app.library_generation, generation);
    assert!(app.library_reload.has_pending());

    settle_library_reload(&mut app);
    assert_ne!(app.library_generation, generation);
}

#[test]
fn refresh_preserves_journal_pixel_scroll_offset() {
    // The journal list scrolls in pixels, not item indices. A refresh must clamp
    // only the selection and leave the offset alone; the old index-based normalize
    // treated the pixel offset as an index and snapped it to `len - 1`, jumping
    // the scroll on every reload.
    let mut app = app_with_journals(&["a", "b", "c"]);
    // A pixel offset far above the 3-journal count — an index clamp would shrink it.
    *app.nav.journal_list.offset_mut() = 15;

    // Both paths that replace the journal list normalize the selection.
    app.reload_journal_list().unwrap();
    assert_eq!(app.nav.journal_list.offset(), 15);

    app.request_library_reload(ReloadReason::Automatic);
    settle_library_reload(&mut app);

    assert_eq!(app.nav.journal_list.offset(), 15);
}
