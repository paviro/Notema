//! Derived-state caches — entry rows, entry bodies, filter rows — are reused
//! until their inputs change, and recompute is deferred while typing.

use super::*;

fn key_char(ch: char) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char(ch),
        crossterm::event::KeyModifiers::NONE,
    )
}

#[test]
fn entry_rows_cache_is_reused_until_inputs_change() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n+++\n\n# A\nBody\n",
    )
    .unwrap();
    fs::write(
        entry_dir.join("b.md"),
        "+++\nschema_version = 1\n[time]\ncreated_at = \"2026-07-01T11:00:00+02:00\"\n+++\n\n# B\nBody\n",
    )
    .unwrap();
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");

    let first = app.entry_rows(30);
    // Same inputs → same cached rows (identity, not just equality).
    assert!(Rc::ptr_eq(&first, &app.entry_rows(30)));
    // Moving the selection does not change the rows, so the cache holds.
    app.move_selection(1);
    assert!(Rc::ptr_eq(&first, &app.entry_rows(30)));
    // A different width rebuilds.
    assert!(!Rc::ptr_eq(&first, &app.entry_rows(20)));
    // Reloading the store rebuilds.
    app.request_library_reload(ReloadReason::Automatic);
    settle_library_reload(&mut app);
    assert!(!Rc::ptr_eq(&first, &app.entry_rows(30)));
}

#[test]
fn search_typing_defers_hit_recompute_until_committed() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n+++\n\n# A\nneedle\n",
    )
    .unwrap();
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    app.begin_search();

    for ch in "needle".chars() {
        app.search_input_key(key_char(ch));
    }
    // The query echoes immediately, but the whole-corpus scan is deferred.
    assert_eq!(app.search.query.as_str(), "needle");
    assert!(app.search.dirty);
    assert!(app.search.hits.is_empty());

    // Committing (what the event loop does after the debounce) runs the scan.
    app.update_search_results();
    assert!(!app.search.dirty);
    assert_eq!(app.search.hits.len(), 1);
}

#[test]
fn entry_body_cache_is_reused_until_entry_or_width_changes() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    write_entry(&entry_dir, "a.md", "2026-07-01T10:00:00+02:00", "# A\nBody");
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    let path = app.selected_entry_target().map(|target| target.path);

    let body = |text| RenderedEntryBody {
        lines: vec![Line::from(text)],
        ..RenderedEntryBody::default()
    };
    let first = app.cached_entry_body(path.as_deref(), 40, None, || body("x"));
    // Same entry + width → cached rows returned, the builder isn't re-run.
    let same = app.cached_entry_body(path.as_deref(), 40, None, || body("y"));
    assert!(Rc::ptr_eq(&first, &same));
    // A different width rebuilds.
    let narrower = app.cached_entry_body(path.as_deref(), 20, None, || body("z"));
    assert!(!Rc::ptr_eq(&first, &narrower));
    // Reloading the store bumps entries_version, invalidating the cache.
    app.request_library_reload(ReloadReason::Automatic);
    settle_library_reload(&mut app);
    let after = app.cached_entry_body(path.as_deref(), 40, None, || body("w"));
    assert!(!Rc::ptr_eq(&first, &after));
}

#[test]
fn search_recompute_keeps_body_and_analytics_caches_but_rebuilds_rows() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    write_entry(&entry_dir, "a.md", "2026-07-01T10:00:00+02:00", "# A\nbody");
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    let path = app.selected_entry_target().map(|target| target.path);

    // Prime all three caches.
    let body = app.cached_entry_body(path.as_deref(), 40, None, || RenderedEntryBody {
        lines: vec![Line::from("x")],
        ..RenderedEntryBody::default()
    });
    let analytics = app.cached_analytics().unwrap();
    let rows = app.entry_rows(30);

    // A search recompute changes the hits but not the entries, so it bumps
    // only rows_version.
    app.begin_search();
    for ch in "body".chars() {
        app.search_input_key(key_char(ch));
    }
    app.update_search_results();

    // Body and analytics caches key on entries_version, which is untouched:
    // requerying returns the same Rc (builder skipped).
    let body_after = app.cached_entry_body(path.as_deref(), 40, None, || RenderedEntryBody {
        lines: vec![Line::from("y")],
        ..RenderedEntryBody::default()
    });
    assert!(Rc::ptr_eq(&body, &body_after));
    let analytics_after = app.cached_analytics().unwrap();
    assert!(Rc::ptr_eq(&analytics, &analytics_after));

    // The row cache keys on rows_version, which the recompute bumped, so it
    // rebuilt.
    let rows_after = app.entry_rows(30);
    assert!(!Rc::ptr_eq(&rows, &rows_after));
}

/// Opening the filter browser walks every entry in scope, and the search box's
/// suggestions read the same rows. Both have to share one walk per scope, and it
/// has to be redone when the entries change.
#[test]
fn filter_rows_are_walked_once_per_scope_until_the_entries_change() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    write_entry(&entry_dir, "a.md", "2026-07-01T10:00:00+02:00", "# A\nbody");
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");

    let all = app.cached_filter_rows(&SearchScope::AllJournals);
    assert!(Rc::ptr_eq(
        &all,
        &app.cached_filter_rows(&SearchScope::AllJournals)
    ));

    // The scope is part of the key: a journal's rows are a different set.
    let journal = app.cached_filter_rows(&SearchScope::Journal("work".to_string()));
    assert!(!Rc::ptr_eq(&all, &journal));
    // …and the memo holds one scope at a time, so alternating rebuilds. Asserted
    // so a later switch to a per-scope map is a deliberate change, not a silent one.
    assert!(!Rc::ptr_eq(
        &all,
        &app.cached_filter_rows(&SearchScope::AllJournals)
    ));

    let before = app.cached_filter_rows(&SearchScope::AllJournals);
    app.install_library_snapshot(LibrarySnapshot {
        journals: app.library.journals.clone(),
        entries: app.library.entries.clone(),
        report: Default::default(),
    });
    assert!(!Rc::ptr_eq(
        &before,
        &app.cached_filter_rows(&SearchScope::AllJournals)
    ));
}
