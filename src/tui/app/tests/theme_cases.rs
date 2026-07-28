//! Theme resolution — journal override vs global config — and the settings
//! menu's theme picker: its scopes, previews, saves, and failure paths.

use super::*;

fn journal_theme(name: &str) -> notema_storage::JournalTheme {
    notema_storage::JournalTheme {
        name: name.to_string(),
        color_mode: None,
        chrome: None,
    }
}

/// An app with a `work` journal that has its own theme, selected, with a global
/// theme saved to disk — the setup for a Global-scope save that clears the
/// override.
fn app_with_a_journal_override() -> AppModel {
    let mut app = app_with_journals(&["work"]);
    app.services.config.ui.theme = "blossom".to_string();
    crate::config::save_config(&app.services.config_path, &app.services.config).unwrap();
    app.select_journal(0);
    app.services
        .store
        .set_journal_theme("work", Some(&journal_theme("gameboy")))
        .unwrap();
    app.library.journals[0].theme = Some(journal_theme("gameboy"));
    app
}

/// Move the picker to Global scope with `fjord` highlighted, ready to confirm.
fn arm_a_global_save(app: &mut AppModel) {
    app.open_theme_picker();
    app.theme_picker_toggle_scope();
    let fjord = app
        .theme_picker_state()
        .unwrap()
        .entries
        .iter()
        .position(|entry| entry.name == "fjord")
        .unwrap();
    app.theme_picker_select(fjord);
}

#[test]
fn appearance_reports_each_theme_warning_once_until_recovery() {
    let mut appearance = Appearance {
        theme: crate::tui::theme::Theme::terminal_default(),
        color_mode: crate::config::ColorMode::Auto,
        chrome_override: None,
        detected_mode: crate::tui::theme::Mode::Dark,
        warned_themes: BTreeSet::new(),
    };

    assert_eq!(
        appearance.warning("broken", Some("invalid theme".to_string())),
        Some("invalid theme".to_string())
    );
    assert_eq!(
        appearance.warning("broken", Some("invalid theme".to_string())),
        None
    );
    assert_eq!(appearance.warning("broken", None), None);
    assert_eq!(
        appearance.warning("broken", Some("invalid again".to_string())),
        Some("invalid again".to_string())
    );
}

#[test]
fn effective_theme_prefers_journal_then_global_and_respects_ignore() {
    let mut app = app_with_journals(&["work"]);
    app.services.config.ui.theme = "globaltheme".to_string();
    app.select_journal(0);

    // No per-journal theme → the global theme.
    assert_eq!(app.effective_theme_name(), "globaltheme");

    // A per-journal theme wins over the global one.
    app.library.journals[0].theme = Some(journal_theme("journaltheme"));
    assert_eq!(app.effective_theme_name(), "journaltheme");

    // ignore_journal_themes forces the global theme regardless.
    app.services.config.ui.ignore_journal_themes = true;
    assert_eq!(app.effective_theme_name(), "globaltheme");
}

#[test]
fn effective_selection_falls_back_to_config_per_field() {
    use crate::config::{ChromeMode, ColorMode};
    let mut app = app_with_journals(&["work"]);
    app.services.config.ui.theme = "globaltheme".to_string();
    app.services.config.ui.color_mode = ColorMode::Dark;
    app.services.config.ui.chrome = ChromeMode::Bordered;
    app.select_journal(0);

    // The journal sets a theme and mode but no chrome; an unknown spelling (from
    // a newer device) counts as unset. Both fall back to the config.
    app.library.journals[0].theme = Some(notema_storage::JournalTheme {
        name: "journaltheme".to_string(),
        color_mode: Some("light".to_string()),
        chrome: Some("holographic".to_string()),
    });
    let selection = app.effective_selection();
    assert_eq!(selection.name, "journaltheme");
    assert_eq!(selection.color_mode, ColorMode::Light);
    assert_eq!(selection.chrome, ChromeMode::Bordered);
}

#[test]
fn theme_picker_opens_on_the_active_theme_with_bundled_entries() {
    let mut app = app_with_journals(&["work"]);
    app.services.config.ui.theme = "eclipse".to_string();

    app.open_theme_picker();

    let state = app.theme_picker_state().expect("picker open");
    let names: Vec<&str> = state.entries.iter().map(|e| e.name.as_str()).collect();
    // Every bundled theme, in sort order — asserted against the list itself so
    // adding one does not break a test that is not about it.
    let mut bundled = crate::tui::theme::bundled_names();
    bundled.sort_unstable();
    assert_eq!(names, bundled);
    assert!(names.contains(&"eclipse") && names.contains(&"gameboy"));
    assert!(state.entries.iter().all(|entry| entry.theme.is_some()));
    // Selection seeds on the configured theme.
    assert_eq!(
        state.selected_index(),
        names.iter().position(|n| *n == "eclipse")
    );
    assert_eq!(state.previous_name, "eclipse");
}

#[test]
fn theme_picker_confirm_saves_the_config_and_closes() {
    let mut app = app_with_journals(&["work"]);
    app.open_theme_picker();
    let fjord = app
        .theme_picker_state()
        .unwrap()
        .entries
        .iter()
        .position(|entry| entry.name == "fjord")
        .unwrap();

    app.theme_picker_select(fjord);
    app.theme_picker_confirm();

    assert!(!app.has_overlay());
    assert_eq!(app.services.config.ui.theme, "fjord");
    // The change was persisted, not just held in memory.
    let saved = crate::config::load_config(&app.services.config_path).unwrap();
    assert_eq!(saved.ui.theme, "fjord");
    assert!(
        app.toasts
            .items()
            .iter()
            .any(|toast| toast.message == "Global theme set to fjord")
    );
}

/// A journal that follows the global theme has no override to clear, so the
/// global scope never writes its sidecar. Writing one anyway churns its mtime on
/// a synced folder, and — when the sidecar cannot be written — rolls the global
/// theme back over an override that was never there.
#[test]
fn a_global_theme_leaves_an_unwritable_sidecar_alone() {
    let mut app = app_with_journals(&["work"]);
    app.select_journal(0);

    // Nothing can be renamed over a directory, so any write to the sidecar fails.
    let sidecar = app.services.store.root().join("work/.journal.toml");
    let _ = fs::remove_file(&sidecar);
    fs::create_dir(&sidecar).unwrap();

    app.open_theme_picker();
    let fjord = app
        .theme_picker_state()
        .unwrap()
        .entries
        .iter()
        .position(|entry| entry.name == "fjord")
        .unwrap();
    app.theme_picker_select(fjord);
    app.theme_picker_confirm();

    assert_eq!(app.services.config.ui.theme, "fjord");
    assert_eq!(
        crate::config::load_config(&app.services.config_path)
            .unwrap()
            .ui
            .theme,
        "fjord"
    );
    assert!(
        !app.toasts
            .items()
            .iter()
            .any(|toast| toast.message.contains("Couldn't clear theme")),
        "there was no override to clear"
    );
    assert!(
        app.toasts
            .items()
            .iter()
            .any(|toast| toast.message == "Global theme set to fjord")
    );
}

#[test]
fn theme_picker_journal_scope_writes_the_sidecar_not_the_global_theme() {
    let mut app = app_with_journals(&["work"]);
    app.services.config.ui.theme = "blossom".to_string();
    crate::config::save_config(&app.services.config_path, &app.services.config).unwrap();
    app.select_journal(0);
    app.open_theme_picker();
    // Switch to this-journal scope, pick a theme, preview a chrome, confirm.
    app.theme_picker_toggle_scope();
    let gameboy = app
        .theme_picker_state()
        .unwrap()
        .entries
        .iter()
        .position(|entry| entry.name == "gameboy")
        .unwrap();
    app.theme_picker_select(gameboy);
    app.theme_picker_cycle_chrome();

    app.theme_picker_confirm();

    // The journal carries the theme with the previewed mode and chrome; the
    // global settings are untouched, in memory and on disk.
    let expected = notema_storage::JournalTheme {
        name: "gameboy".to_string(),
        color_mode: Some("auto".to_string()),
        chrome: Some("flat".to_string()),
    };
    assert_eq!(app.library.journals[0].theme, Some(expected.clone()));
    assert_eq!(app.effective_theme_name(), "gameboy");
    assert_eq!(app.services.config.ui.theme, "blossom");
    assert_eq!(
        app.services.config.ui.chrome,
        crate::config::ChromeMode::Default
    );
    let saved = crate::config::load_config(&app.services.config_path).unwrap();
    assert_eq!(saved.ui.theme, "blossom");
    assert_eq!(saved.ui.chrome, crate::config::ChromeMode::Default);
    // Persisted to the sidecar, reloadable.
    let reloaded = app.services.store.list_journals().unwrap();
    let work = reloaded.iter().find(|j| j.name == "work").unwrap();
    assert_eq!(work.theme, Some(expected));
}

#[test]
fn theme_picker_global_scope_clears_a_journal_override() {
    let mut app = app_with_journals(&["work"]);
    app.select_journal(0);
    app.services
        .store
        .set_journal_theme("work", Some(&journal_theme("gameboy")))
        .unwrap();
    app.library.journals[0].theme = Some(journal_theme("gameboy"));

    app.open_theme_picker();
    // Opens in Journal scope (the journal has a theme); switch to Global and save.
    app.theme_picker_toggle_scope();
    let fjord = app
        .theme_picker_state()
        .unwrap()
        .entries
        .iter()
        .position(|entry| entry.name == "fjord")
        .unwrap();
    app.theme_picker_select(fjord);
    app.theme_picker_confirm();

    assert_eq!(app.services.config.ui.theme, "fjord");
    // The journal's override was removed; it now follows global.
    assert_eq!(app.library.journals[0].theme, None);
    let reloaded = app.services.store.list_journals().unwrap();
    let work = reloaded.iter().find(|j| j.name == "work").unwrap();
    assert_eq!(work.theme, None);
    assert_eq!(app.effective_theme_name(), "fjord");
}

#[test]
fn theme_picker_keeps_the_journal_override_when_the_config_cant_be_saved() {
    let mut app = app_with_a_journal_override();
    arm_a_global_save(&mut app);
    // A directory where the config file goes: the atomic rename can't land.
    fs::remove_file(&app.services.config_path).unwrap();
    fs::create_dir(&app.services.config_path).unwrap();

    app.theme_picker_confirm();

    assert!(app.theme_picker_state().is_some(), "picker stays open");
    assert!(
        app.toasts
            .items()
            .iter()
            .any(|toast| toast.message.starts_with("Couldn't save config:"))
    );
    // Nothing was written, so nothing needed undoing — including in memory.
    assert_eq!(app.services.config.ui.theme, "blossom");
    assert_eq!(
        app.library.journals[0].theme,
        Some(journal_theme("gameboy"))
    );
    let reloaded = app.services.store.list_journals().unwrap();
    let work = reloaded.iter().find(|j| j.name == "work").unwrap();
    assert_eq!(work.theme, Some(journal_theme("gameboy")));
}

#[test]
fn theme_picker_restores_the_global_theme_when_the_override_cant_be_cleared() {
    let mut app = app_with_a_journal_override();
    arm_a_global_save(&mut app);
    // A directory where the sidecar goes: the atomic rename can't land.
    let sidecar = app
        .services
        .config
        .journal
        .path
        .join("work")
        .join(".journal.toml");
    fs::remove_file(&sidecar).unwrap();
    fs::create_dir(&sidecar).unwrap();

    app.theme_picker_confirm();

    assert!(app.theme_picker_state().is_some(), "picker stays open");
    assert!(
        app.toasts
            .items()
            .iter()
            .any(|toast| toast.message.starts_with("Couldn't clear theme:"))
    );
    // The config had already been written with fjord; it was put back.
    assert_eq!(app.services.config.ui.theme, "blossom");
    let saved = crate::config::load_config(&app.services.config_path).unwrap();
    assert_eq!(saved.ui.theme, "blossom");
    assert_eq!(
        app.library.journals[0].theme,
        Some(journal_theme("gameboy"))
    );
}

#[test]
fn theme_picker_offers_no_journal_scope_when_journal_themes_are_ignored() {
    use crate::tui::state::ThemePickerScope;
    let mut app = app_with_journals(&["work"]);
    app.services.config.ui.ignore_journal_themes = true;
    app.select_journal(0);
    app.services
        .store
        .set_journal_theme("work", Some(&journal_theme("gameboy")))
        .unwrap();
    app.library.journals[0].theme = Some(journal_theme("gameboy"));

    app.open_theme_picker();

    let state = app.theme_picker_state().unwrap();
    assert_eq!(state.journal, None);
    assert_eq!(state.scope, ThemePickerScope::Global);
    // No journal in context, so the scope hint isn't offered either.
    let hints = state.hint_state(app.appearance.chrome_override, app.appearance.color_mode);
    assert!(!hints.has_journal);
}

#[test]
fn theme_picker_global_save_keeps_the_sidecar_when_journal_themes_are_ignored() {
    let mut app = app_with_journals(&["work"]);
    app.services.config.ui.ignore_journal_themes = true;
    app.select_journal(0);
    app.services
        .store
        .set_journal_theme("work", Some(&journal_theme("gameboy")))
        .unwrap();
    app.library.journals[0].theme = Some(journal_theme("gameboy"));

    app.open_theme_picker();
    let fjord = app
        .theme_picker_state()
        .unwrap()
        .entries
        .iter()
        .position(|entry| entry.name == "fjord")
        .unwrap();
    app.theme_picker_select(fjord);
    app.theme_picker_confirm();

    assert_eq!(app.services.config.ui.theme, "fjord");
    // The sidecar syncs to the user's other devices, which do honor it.
    let reloaded = app.services.store.list_journals().unwrap();
    let work = reloaded.iter().find(|j| j.name == "work").unwrap();
    assert_eq!(work.theme, Some(journal_theme("gameboy")));
    assert_eq!(
        app.library.journals[0].theme,
        Some(journal_theme("gameboy"))
    );
    // This device still shows the global theme.
    assert_eq!(app.effective_theme_name(), "fjord");
}

#[test]
fn theme_picker_toggle_scope_moves_the_highlight_to_that_scopes_theme() {
    let mut app = app_with_journals(&["work"]);
    app.services.config.ui.theme = "blossom".to_string();
    app.select_journal(0);
    app.services
        .store
        .set_journal_theme("work", Some(&journal_theme("gameboy")))
        .unwrap();
    app.library.journals[0].theme = Some(journal_theme("gameboy"));

    app.open_theme_picker();
    // Opens in Journal scope, highlighting the journal's own theme.
    assert_eq!(
        app.theme_picker_state()
            .unwrap()
            .selected_entry()
            .unwrap()
            .name,
        "gameboy"
    );
    // Toggling to Global moves the highlight to the global default, not just the
    // preview, so the selected row matches the applied theme.
    app.theme_picker_toggle_scope();
    assert_eq!(
        app.theme_picker_state()
            .unwrap()
            .selected_entry()
            .unwrap()
            .name,
        "blossom"
    );
    // And back again.
    app.theme_picker_toggle_scope();
    assert_eq!(
        app.theme_picker_state()
            .unwrap()
            .selected_entry()
            .unwrap()
            .name,
        "gameboy"
    );
}

#[test]
fn theme_picker_toggle_scope_previews_that_scopes_mode_and_chrome() {
    use crate::config::{ChromeMode, ColorMode};
    let mut app = app_with_journals(&["work"]);
    app.services.config.ui.theme = "blossom".to_string();
    app.services.config.ui.color_mode = ColorMode::Light;
    app.services.config.ui.chrome = ChromeMode::Bordered;
    app.select_journal(0);
    app.library.journals[0].theme = Some(notema_storage::JournalTheme {
        name: "gameboy".to_string(),
        color_mode: Some("dark".to_string()),
        chrome: Some("flat".to_string()),
    });

    // Opens in Journal scope; toggling to Global snaps the previewed mode and
    // chrome to the config values, and back to the journal's own.
    app.open_theme_picker();
    app.theme_picker_toggle_scope();
    assert_eq!(app.appearance.color_mode, ColorMode::Light);
    assert_eq!(
        app.appearance.chrome_override,
        Some(crate::tui::theme::ChromeStyle::Bordered)
    );
    app.theme_picker_toggle_scope();
    assert_eq!(app.appearance.color_mode, ColorMode::Dark);
    assert_eq!(
        app.appearance.chrome_override,
        Some(crate::tui::theme::ChromeStyle::Flat)
    );
}

#[test]
fn switching_journals_switches_the_effective_theme() {
    let mut app = app_with_journals(&["personal", "work"]);
    app.services.config.ui.theme = "globaltheme".to_string();
    let work = app
        .library
        .journals
        .iter()
        .position(|j| j.name == "work")
        .unwrap();
    app.library.journals[work].theme = Some(journal_theme("worktheme"));

    app.select_journal_by_name("work");
    assert_eq!(app.effective_theme_name(), "worktheme");
    app.select_journal_by_name("personal");
    assert_eq!(app.effective_theme_name(), "globaltheme");
}

#[test]
fn all_journals_search_uses_the_global_theme_and_exit_restores() {
    let mut app = app_with_journals(&["work"]);
    app.services.config.ui.theme = "globaltheme".to_string();
    app.select_journal(0);
    app.library.journals[0].theme = Some(journal_theme("journaltheme"));
    assert_eq!(app.effective_theme_name(), "journaltheme");

    // From the journal column, search covers all journals: cross-journal hits
    // shouldn't re-theme per hit, so the global theme applies.
    app.nav.focus = Focus::Journals;
    app.begin_search();
    assert_eq!(app.effective_theme_name(), "globaltheme");
    app.exit_search();
    assert_eq!(app.effective_theme_name(), "journaltheme");
}

#[test]
fn journal_scoped_search_keeps_that_journals_theme() {
    let mut app = app_with_journals(&["work"]);
    app.services.config.ui.theme = "globaltheme".to_string();
    app.select_journal(0);
    app.library.journals[0].theme = Some(journal_theme("journaltheme"));

    app.nav.focus = Focus::Entries;
    app.begin_search();
    assert_eq!(app.search.scope, SearchScope::Journal("work".to_string()));
    assert_eq!(app.effective_theme_name(), "journaltheme");
}

#[test]
fn compose_uses_the_target_journals_theme() {
    let mut app = app_with_journals(&["personal", "work"]);
    app.services.config.ui.theme = "globaltheme".to_string();
    let work = app
        .library
        .journals
        .iter()
        .position(|j| j.name == "work")
        .unwrap();
    app.library.journals[work].theme = Some(journal_theme("worktheme"));
    // A different journal is selected (as when state restores the last one).
    app.select_journal_by_name("personal");
    assert_eq!(app.effective_theme_name(), "globaltheme");

    app.begin_compose("work".to_string(), notema_domain::Metadata::default());
    assert_eq!(app.effective_theme_name(), "worktheme");
}

#[test]
fn theme_picker_cancel_reverts_the_preview_and_leaves_config_untouched() {
    let mut app = app_with_journals(&["work"]);
    app.open_theme_picker();
    let previous = app.theme_picker_state().unwrap().previous.clone();
    let eclipse = app
        .theme_picker_state()
        .unwrap()
        .entries
        .iter()
        .position(|entry| entry.name == "eclipse")
        .unwrap();

    // Moving the selection shows the entry immediately…
    app.theme_picker_select(eclipse);
    assert_ne!(app.appearance.theme, previous);

    // …and Esc restores the open-time theme without touching the config.
    app.theme_picker_cancel();

    assert!(!app.has_overlay());
    assert_eq!(app.appearance.theme, previous);
    assert_eq!(
        app.services.config.ui.theme,
        crate::tui::theme::DEFAULT_THEME
    );
    assert!(
        !app.services.config_path.exists(),
        "cancel must not write the config"
    );
}

#[test]
fn theme_picker_confirm_on_a_broken_theme_toasts_and_stays_open() {
    let mut app = app_with_journals(&["work"]);
    let themes = crate::tui::theme::themes_dir(&app.services.config_path);
    fs::create_dir_all(&themes).unwrap();
    fs::write(themes.join("busted.toml"), "surfaces = 12\n").unwrap();
    app.open_theme_picker();
    let busted = app
        .theme_picker_state()
        .unwrap()
        .entries
        .iter()
        .position(|entry| entry.name == "busted")
        .unwrap();
    assert!(
        app.theme_picker_state().unwrap().entries[busted]
            .theme
            .is_none(),
        "broken file should fail to parse"
    );

    let before = app.appearance.theme.clone();
    app.theme_picker_select(busted);
    // A broken row never shows an entry.
    assert_eq!(app.appearance.theme, before);

    app.theme_picker_confirm();

    assert!(app.theme_picker_state().is_some(), "picker stays open");
    assert_eq!(
        app.services.config.ui.theme,
        crate::tui::theme::DEFAULT_THEME
    );
    assert!(
        app.toasts
            .items()
            .iter()
            .any(|toast| toast.message.contains("broken"))
    );
}
