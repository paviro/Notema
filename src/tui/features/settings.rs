//! Library administration driven by the settings menu and its sub-actions:
//! journal creation, and the theme picker with its scoped (journal vs global)
//! preview/confirm/cancel lifecycle.

use crate::tui::{
    app::AppModel,
    state::{ListNav, Overlay, ThemePickerState, ToastVariant},
    text_input::TextInput,
};

impl AppModel {
    pub(crate) fn begin_new_journal_input(&mut self) {
        self.overlay = Overlay::NewJournal(Box::default());
    }

    pub(crate) fn new_journal_input(&self) -> Option<&TextInput> {
        match &self.overlay {
            Overlay::NewJournal(name) => Some(name),
            _ => None,
        }
    }

    pub(crate) fn new_journal_input_mut(&mut self) -> Option<&mut TextInput> {
        match &mut self.overlay {
            Overlay::NewJournal(name) => Some(name),
            _ => None,
        }
    }

    pub(crate) fn open_settings_menu(&mut self) {
        self.overlay = Overlay::SettingsMenu;
    }

    /// Open the theme picker: list the theme files on disk (parse results
    /// cached per row), seed the selection on the configured theme, and
    /// remember the installed theme so Esc can restore it.
    pub(crate) fn open_theme_picker(&mut self) {
        use crate::tui::state::{SelectableList, ThemePickerEntry, ThemePickerState};

        let dir = crate::tui::theme::themes_dir(&self.services.config_path);
        if let Err(err) = crate::tui::theme::ensure_bundled(&dir) {
            self.toast(
                ToastVariant::Error,
                format!(
                    "Couldn't prepare themes: {}",
                    crate::tui::concise_error(&err)
                ),
            );
        }
        let mode = self.appearance.mode();
        let mut entries: Vec<ThemePickerEntry> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|dirent| {
                let path = dirent.path();
                if path.extension().is_none_or(|ext| ext != "toml") {
                    return None;
                }
                let name = path.file_stem()?.to_str()?.to_string();
                let dark = crate::tui::theme::load_file(&path, crate::tui::theme::Mode::Dark).ok();
                let light =
                    crate::tui::theme::load_file(&path, crate::tui::theme::Mode::Light).ok();
                let mode_agnostic = dark == light;
                Some(ThemePickerEntry {
                    theme: match mode {
                        crate::tui::theme::Mode::Dark => dark,
                        crate::tui::theme::Mode::Light => light,
                    },
                    name,
                    mode_agnostic,
                })
            })
            .collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        use crate::tui::state::{JournalThemeChoice, ThemePickerScope};
        let context = self.context_journal();
        let journal = context.map(|j| j.name.clone());
        let journal_theme = context
            .and_then(|j| j.theme.as_ref())
            .map(|t| JournalThemeChoice {
                name: t.name.clone(),
                color_mode: t
                    .color_mode
                    .as_deref()
                    .and_then(crate::config::ColorMode::from_name),
                chrome: t
                    .chrome
                    .as_deref()
                    .and_then(crate::config::ChromeMode::from_name),
            });
        // Open on the journal's own theme when it has one, otherwise on the
        // global default.
        let scope = if journal.is_some() && journal_theme.is_some() {
            ThemePickerScope::Journal
        } else {
            ThemePickerScope::Global
        };
        let seed_name = match &journal_theme {
            Some(theme) if scope == ThemePickerScope::Journal => theme.name.clone(),
            _ => self.services.config.ui.theme.clone(),
        };

        let mut state = ThemePickerState {
            entries,
            list: SelectableList::default(),
            previous: self.appearance.theme.clone(),
            previous_name: self.services.config.ui.theme.clone(),
            previous_chrome: self.appearance.chrome_override,
            previous_color_mode: self.appearance.color_mode,
            scope,
            journal,
            journal_theme,
        };
        let active = state
            .entries
            .iter()
            .position(|entry| entry.name == seed_name)
            .unwrap_or(0);
        state.select_index(active);
        self.overlay = Overlay::ThemePicker(Box::new(state));
        // Install the seeded row so the preview matches the highlight from the
        // first frame, not only after the selection moves.
        self.theme_picker_preview();
    }

    /// Move the picker selection to the row named `name` (if present) and preview.
    fn theme_picker_select_named(&mut self, name: &str) {
        if let Some(index) = self
            .theme_picker_state()
            .and_then(|state| state.entries.iter().position(|entry| entry.name == name))
        {
            self.theme_picker_select(index);
        }
    }

    /// Toggle the scope between this journal and the global default, snapping the
    /// selection — theme, color mode, and chrome — to that scope's saved values,
    /// so the preview shows exactly what confirming would keep. A no-op with no
    /// journal in context.
    pub(crate) fn theme_picker_toggle_scope(&mut self) {
        use crate::tui::state::ThemePickerScope;
        let Some(state) = self.theme_picker_state() else {
            return;
        };
        if state.journal.is_none() {
            return;
        }
        let (next, name, color_mode, chrome) = match state.scope {
            ThemePickerScope::Journal => (
                ThemePickerScope::Global,
                self.services.config.ui.theme.clone(),
                self.services.config.ui.color_mode,
                self.services.config.ui.chrome,
            ),
            // Seed Journal scope on the journal's own theme, falling back to the
            // global values for anything it doesn't set — including the whole
            // theme when the journal has none yet (so you can pick one).
            ThemePickerScope::Global => {
                let theme = state.journal_theme.as_ref();
                (
                    ThemePickerScope::Journal,
                    theme
                        .map(|t| t.name.clone())
                        .unwrap_or_else(|| self.services.config.ui.theme.clone()),
                    theme
                        .and_then(|t| t.color_mode)
                        .unwrap_or(self.services.config.ui.color_mode),
                    theme
                        .and_then(|t| t.chrome)
                        .unwrap_or(self.services.config.ui.chrome),
                )
            }
        };
        if let Some(state) = self.theme_picker_state_mut() {
            state.scope = next;
        }
        let mode_before = self.appearance.mode();
        self.appearance.color_mode = color_mode;
        self.appearance.chrome_override = crate::tui::theme::chrome_style(chrome);
        if self.appearance.mode() != mode_before {
            self.theme_picker_reresolve_rows();
        }
        self.theme_picker_select_named(&name);
    }

    pub(crate) fn theme_picker_state(&self) -> Option<&ThemePickerState> {
        match &self.overlay {
            Overlay::ThemePicker(state) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn theme_picker_state_mut(&mut self) -> Option<&mut ThemePickerState> {
        match &mut self.overlay {
            Overlay::ThemePicker(state) => Some(state),
            _ => None,
        }
    }

    /// Live preview: install the highlighted theme if it parsed. Broken rows
    /// leave whatever is installed untouched.
    pub(crate) fn theme_picker_preview(&mut self) {
        if let Some(theme) = self
            .theme_picker_state()
            .and_then(|state| state.selected_entry())
            .and_then(|entry| entry.theme.clone())
        {
            self.appearance.theme = self.appearance.resolve(theme);
        }
    }

    /// Move the picker selection to `index` and preview that row.
    pub(crate) fn theme_picker_select(&mut self, index: usize) {
        if let Some(state) = self.theme_picker_state_mut() {
            state.select_index(index);
        }
        self.theme_picker_preview();
    }

    /// Cycle the chrome override (default → flat → bordered → default),
    /// previewing live on the next frame.
    /// Persisted on confirm; cancel restores the value from open time.
    pub(crate) fn theme_picker_cycle_chrome(&mut self) {
        use crate::tui::theme::ChromeStyle;
        self.appearance.chrome_override = match self.appearance.chrome_override {
            None => Some(ChromeStyle::Flat),
            Some(ChromeStyle::Flat) => Some(ChromeStyle::Bordered),
            Some(ChromeStyle::Bordered) => None,
        };
        self.theme_picker_preview();
    }

    /// Cycle the color mode (auto → dark → light → auto), previewing live.
    /// Unlike the chrome override, a mode change invalidates every resolved
    /// theme (variants are flattened at load), so the picker's rows re-resolve
    /// and the highlighted one re-installs.
    pub(crate) fn theme_picker_cycle_mode(&mut self) {
        use crate::config::ColorMode;
        // No-op on rows where the switch is hidden (its hint is gone too).
        if !self
            .theme_picker_state()
            .is_some_and(|state| state.mode_switchable())
        {
            return;
        }
        self.appearance.color_mode = match self.appearance.color_mode {
            ColorMode::Auto => ColorMode::Dark,
            ColorMode::Dark => ColorMode::Light,
            ColorMode::Light => ColorMode::Auto,
        };
        self.theme_picker_reresolve_rows();
    }

    /// Re-resolve every row at the current mode and re-install the highlighted
    /// one — a mode change invalidates the flattened variants cached per row.
    fn theme_picker_reresolve_rows(&mut self) {
        let dir = crate::tui::theme::themes_dir(&self.services.config_path);
        let mode = self.appearance.mode();
        if let Some(state) = self.theme_picker_state_mut() {
            for entry in &mut state.entries {
                let path = dir.join(format!("{}.toml", entry.name));
                entry.theme = crate::tui::theme::load_file(&path, mode).ok();
            }
        }
        self.theme_picker_preview();
    }

    /// Confirm the highlighted theme: persist it — with the previewed color mode
    /// and chrome — to the active scope (the journal's sidecar, or the config
    /// plus clearing the journal's override) and close. A broken row or a failed
    /// save toasts and keeps the picker open.
    pub(crate) fn theme_picker_confirm(&mut self) {
        let Some(entry) = self
            .theme_picker_state()
            .and_then(|state| state.selected_entry())
        else {
            return;
        };
        use crate::tui::state::ThemePickerScope;
        let name = entry.name.clone();
        if entry.theme.is_none() {
            self.toast(
                ToastVariant::Error,
                format!("Theme '{name}' is broken; fix its file or pick another"),
            );
            return;
        }

        let (scope, journal) = self
            .theme_picker_state()
            .map(|state| (state.scope, state.journal.clone()))
            .unwrap_or((ThemePickerScope::Global, None));

        // The scope only ever becomes Journal with a journal in context, so the
        // catch-all arm is the Global scope.
        let toast = match (scope, journal) {
            (ThemePickerScope::Journal, Some(journal_name)) => {
                // The journal's theme carries the previewed color mode and chrome
                // with it, so it looks the same on every device.
                let theme = notema_storage::JournalTheme {
                    name: name.clone(),
                    color_mode: Some(self.appearance.color_mode.name().to_string()),
                    chrome: Some(
                        crate::tui::theme::chrome_mode(self.appearance.chrome_override)
                            .name()
                            .to_string(),
                    ),
                };
                if let Err(err) = self
                    .services
                    .store
                    .set_journal_theme(&journal_name, Some(&theme))
                {
                    self.toast(
                        ToastVariant::Error,
                        format!("Couldn't set theme: {}", crate::tui::concise_error(&err)),
                    );
                    return;
                }
                self.set_local_journal_theme(&journal_name, Some(theme));
                format!(
                    "Theme for {} set to {name}",
                    notema_storage::journal_display_name(&journal_name)
                )
            }
            (_, journal) => {
                self.services.config.ui.theme = name.clone();
                self.services.config.ui.color_mode = self.appearance.color_mode;
                self.services.config.ui.chrome =
                    crate::tui::theme::chrome_mode(self.appearance.chrome_override);
                // Switching a journal to Global removes its own override so it
                // follows the (possibly just-changed) global theme.
                if let Some(journal_name) = journal {
                    if let Err(err) = self.services.store.set_journal_theme(&journal_name, None) {
                        self.toast(
                            ToastVariant::Error,
                            format!("Couldn't clear theme: {}", crate::tui::concise_error(&err)),
                        );
                        return;
                    }
                    self.set_local_journal_theme(&journal_name, None);
                }
                if let Err(err) =
                    crate::config::save_config(&self.services.config_path, &self.services.config)
                {
                    self.toast(
                        ToastVariant::Error,
                        format!("Couldn't save config: {}", crate::tui::concise_error(&err)),
                    );
                    return;
                }
                format!("Global theme set to {name}")
            }
        };

        self.apply_effective_theme();
        self.toast(ToastVariant::Success, toast);
        self.close_overlay();
    }

    /// Update the in-memory `Journal.theme` for `name` so the next render and
    /// journal switch see the change without a rescan.
    fn set_local_journal_theme(&mut self, name: &str, theme: Option<notema_storage::JournalTheme>) {
        if let Some(journal) = self
            .library
            .journals
            .iter_mut()
            .find(|journal| journal.name == name)
        {
            journal.theme = theme;
            self.library_generation = self.library_generation.wrapping_add(1);
        }
    }

    /// Cancel the picker: restore the theme, chrome override, and color mode
    /// from open time; the config was never touched.
    pub(crate) fn theme_picker_cancel(&mut self) {
        if let Some((color_mode, chrome, theme)) = self.theme_picker_state().map(|state| {
            (
                state.previous_color_mode,
                state.previous_chrome,
                state.previous.clone(),
            )
        }) {
            self.appearance.color_mode = color_mode;
            self.appearance.chrome_override = chrome;
            self.appearance.theme = theme;
        }
        self.close_overlay();
    }
}
