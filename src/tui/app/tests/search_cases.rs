//! Search scope — global or one journal — and what a query matches.

use super::*;

#[test]
fn search_from_journal_focus_is_global() {
    let config = Config::new(tempdir().unwrap().path().to_path_buf());
    let mut app = new_app(config);
    app.nav.focus = Focus::Journals;

    app.begin_search();

    assert_eq!(app.search.scope, SearchScope::AllJournals);
}

#[test]
fn search_from_entries_focus_is_scoped_to_selected_journal() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("work")).unwrap();
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    app.nav.focus = Focus::Entries;

    app.begin_search();

    assert_eq!(app.search.scope, SearchScope::Journal("work".to_string()));
}

/// Drilling into a tag runs it under the journal it was clicked in, not under
/// whatever scope the last search left on the app. The hits and the query box
/// have to agree — otherwise the first keystroke re-runs the visible query and
/// the list silently shrinks.
#[test]
fn a_tag_drill_down_is_scoped_to_the_journal_it_came_from() {
    let dir = tempdir().unwrap();
    for journal in ["work", "home"] {
        let entry_dir = dir.path().join(journal).join("2026-07-01");
        fs::create_dir_all(&entry_dir).unwrap();
        fs::write(
            entry_dir.join("a.md"),
            "+++\nschema_version = 1\n\n[entry]\ntags = [\"admin\"]\n+++\n\n# A\n",
        )
        .unwrap();
    }
    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    app.nav.focus = Focus::Entries;

    app.begin_tag_search("admin");

    assert_eq!(app.search.scope, SearchScope::Journal("work".to_string()));
    assert_eq!(app.search.hits.len(), 1);
    assert!(app.search.hits.iter().all(|hit| hit.journal == "work"));
    // Re-running the query the user can see reproduces exactly these hits.
    assert_eq!(app.search_results().len(), app.search.hits.len());
}

#[test]
fn empty_search_has_no_selected_entry() {
    let config = Config::new(tempdir().unwrap().path().to_path_buf());
    let mut app = new_app(config);

    app.begin_search();

    assert_eq!(app.nav.selected_entry_index, None);
    assert!(app.selected_entry_target().is_none());
}

#[test]
fn feelings_search_matches_full_and_partial_labels() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n\n[entry]\nfeelings = [\"calm\"]\n+++\n\n# A\n",
    )
    .unwrap();
    fs::write(
        entry_dir.join("b.md"),
        "+++\nschema_version = 1\n\n[entry]\nfeelings = [\"anxious\"]\n+++\n\n# B\n",
    )
    .unwrap();
    fs::write(
        entry_dir.join("c.md"),
        "+++\nschema_version = 1\n\n[entry]\nfeelings = [\"relaxed\"]\n+++\n\n# C\n",
    )
    .unwrap();
    fs::write(
        entry_dir.join("d.md"),
        "+++\nschema_version = 1\n\n[entry]\nfeelings = [\"grateful\"]\n+++\n\n# D\n",
    )
    .unwrap();

    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    app.begin_search();

    let titles = |app: &AppModel| {
        let mut t: Vec<_> = app.search.hits.iter().map(|h| h.title.clone()).collect();
        t.sort();
        t
    };

    // Full label still matches exactly one entry.
    app.search.query = "feelings:calm".into();
    app.update_search_results();
    assert_eq!(titles(&app), vec!["A"]);

    // Partial label matches: `cal` -> calm, and the reported bug `relaxe`/`relax` -> relaxed.
    app.search.query = "feelings:cal".into();
    app.update_search_results();
    assert_eq!(titles(&app), vec!["A"]);

    app.search.query = "feelings:relaxe".into();
    app.update_search_results();
    assert_eq!(titles(&app), vec!["C"]);

    app.search.query = "feelings:relax".into();
    app.update_search_results();
    assert_eq!(titles(&app), vec!["C"]);

    // Partial alias resolves onto the canonical feeling: `thank` -> grateful.
    app.search.query = "feelings:thank".into();
    app.update_search_results();
    assert_eq!(titles(&app), vec!["D"]);
}

#[test]
fn starred_search_filters_by_flag() {
    let dir = tempdir().unwrap();
    let entry_dir = dir.path().join("work").join("2026-07-01");
    fs::create_dir_all(&entry_dir).unwrap();
    fs::write(
        entry_dir.join("a.md"),
        "+++\nschema_version = 1\n\n[entry]\nstarred = true\n+++\n\n# Fav\n",
    )
    .unwrap();
    fs::write(
        entry_dir.join("b.md"),
        "+++\nschema_version = 1\n+++\n\n# Plain\n",
    )
    .unwrap();

    let config = Config::new(dir.path().to_path_buf());
    let mut app = new_app(config);
    app.select_journal_by_name("work");
    app.begin_search();

    app.search.query = "star:true".into();
    app.update_search_results();
    assert_eq!(app.search.hits.len(), 1);
    assert_eq!(app.search.hits[0].title, "Fav");

    app.search.query = "star:false".into();
    app.update_search_results();
    assert_eq!(app.search.hits.len(), 1);
    assert_eq!(app.search.hits[0].title, "Plain");

    // 1/0 are accepted as boolean aliases.
    app.search.query = "star:1".into();
    app.update_search_results();
    assert_eq!(app.search.hits.len(), 1);
    assert_eq!(app.search.hits[0].title, "Fav");

    app.search.query = "star:0".into();
    app.update_search_results();
    assert_eq!(app.search.hits.len(), 1);
    assert_eq!(app.search.hits[0].title, "Plain");

    // An unparseable flag matches nothing.
    app.search.query = "star:maybe".into();
    app.update_search_results();
    assert!(app.search.hits.is_empty());
}
