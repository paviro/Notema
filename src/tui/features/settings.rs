//! Library administration driven by the settings dialog and its sub-actions:
//! journal creation, and the theme picker with its scoped (journal vs global)
//! preview/confirm/cancel lifecycle.

use crate::config::Config;
use crate::tui::{
    app::AppModel,
    state::{ListNav, Overlay, SettingsState, ThemePickerState, ToastVariant},
    text_input::TextInput,
};

/// A group of related settings, shown as one sub-header in the settings dialog.
/// Grouped by what the user is doing, not by config-file section: e.g. the
/// location and attachment toggles live under [`Editor`](Self::Editor) because
/// they take effect while editing/saving an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingCategory {
    Appearance,
    Reader,
    Editor,
}

impl SettingCategory {
    pub(crate) const ALL: [SettingCategory; 3] = [
        SettingCategory::Appearance,
        SettingCategory::Reader,
        SettingCategory::Editor,
    ];

    pub(crate) fn title(self) -> &'static str {
        match self {
            SettingCategory::Appearance => "Appearance",
            SettingCategory::Reader => "Reader",
            SettingCategory::Editor => "Editor",
        }
    }

    pub(crate) fn rows(self) -> &'static [SettingRow] {
        match self {
            SettingCategory::Appearance => APPEARANCE_SETTINGS,
            SettingCategory::Reader => READER_SETTINGS,
            SettingCategory::Editor => EDITOR_SETTINGS,
        }
    }
}

/// Extra work a setting needs after it changes, beyond writing the config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AfterChange {
    None,
    /// Re-resolve the active theme — the setting feeds theme resolution
    /// (e.g. `ignore_journal_themes`).
    ReresolveTheme,
}

/// One setting under a category. The `get`/`set` function pointers read and
/// write the real config field, so adding a setting is a single entry here — no
/// per-setting action or handler plumbing.
pub(crate) enum SettingRow {
    /// Opens the theme picker; its value column shows the current theme name.
    Theme,
    Bool {
        label: &'static str,
        description: &'static str,
        get: fn(&Config) -> bool,
        set: fn(&mut Config, bool),
        after: AfterChange,
        inherit: Option<Inherit>,
    },
    Number {
        label: &'static str,
        description: &'static str,
        get: fn(&Config) -> u16,
        set: fn(&mut Config, u16),
        step: u16,
        min: u16,
        max: u16,
        /// When set, a value of 0 sits just below `min` as a special "off"
        /// state, shown with this label; stepping down from `min` snaps to 0,
        /// up snaps back.
        off_label: Option<&'static str>,
        inherit: Option<Inherit>,
    },
}

/// A row's third state, below its own values: following another surface's
/// setting instead of carrying one. Stepping left off the lowest value lands
/// here, stepping right leaves it.
#[derive(Clone, Copy)]
pub(crate) struct Inherit {
    pub is: fn(&Config) -> bool,
    pub set: fn(&mut Config),
}

const INHERIT_LABEL: &str = "Inherit";

impl SettingRow {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            // The Theme row opens the picker; the dialog trails it with the
            // themable disclosure glyph rather than an ellipsis.
            SettingRow::Theme => "Theme",
            SettingRow::Bool { label, .. } | SettingRow::Number { label, .. } => label,
        }
    }

    pub(crate) fn description(&self) -> &'static str {
        match self {
            SettingRow::Theme => "Choose the color theme (also its color mode and chrome).",
            SettingRow::Bool { description, .. } | SettingRow::Number { description, .. } => {
                description
            }
        }
    }

    fn inherit(&self) -> Option<Inherit> {
        match self {
            SettingRow::Theme => None,
            SettingRow::Bool { inherit, .. } | SettingRow::Number { inherit, .. } => *inherit,
        }
    }

    fn inheriting(&self, config: &Config) -> bool {
        self.inherit().is_some_and(|inherit| (inherit.is)(config))
    }

    /// The value column text for the current config: `Inherit`, `On`/`Off`, the
    /// number, or the current theme name.
    pub(crate) fn value(&self, config: &Config) -> String {
        if self.inheriting(config) {
            return INHERIT_LABEL.to_string();
        }
        match self {
            SettingRow::Theme => config.ui.theme.clone(),
            SettingRow::Bool { get, .. } => if get(config) { "On" } else { "Off" }.to_string(),
            SettingRow::Number { get, off_label, .. } => match off_label {
                Some(label) if get(config) == 0 => label.to_string(),
                _ => get(config).to_string(),
            },
        }
    }
}

const APPEARANCE_SETTINGS: &[SettingRow] = &[
    SettingRow::Theme,
    SettingRow::Bool {
        label: "Ignore per-journal themes",
        description: "Always use the global theme, ignoring themes set on individual journals. Useful on low-capability terminals (e-ink).",
        get: |c| c.ui.ignore_journal_themes,
        set: |c, v| c.ui.ignore_journal_themes = v,
        after: AfterChange::ReresolveTheme,
        inherit: None,
    },
];

const READER_SETTINGS: &[SettingRow] = &[
    SettingRow::Bool {
        label: "Center body vertically",
        description: "Vertically center the entry body when it fits without scrolling.",
        get: |c| c.ui.layout.reader.body_center_vertically,
        set: |c, v| c.ui.layout.reader.body_center_vertically = v,
        after: AfterChange::None,
        inherit: None,
    },
    SettingRow::Number {
        label: "Max body width",
        description: "Maximum entry-body width in columns; wider panes gutter the sides for readability. Step below the minimum for Unlimited (no cap, full width).",
        get: |c| c.ui.layout.reader.body_max_width,
        set: |c, v| c.ui.layout.reader.body_max_width = v,
        step: 5,
        min: 40,
        max: 240,
        off_label: Some("Unlimited"),
        inherit: None,
    },
    SettingRow::Number {
        label: "Max body top padding",
        description: "Extra blank lines above the body on wide panes, ramping in with the side gutters. 0 keeps the body flush to the top.",
        get: |c| c.ui.layout.reader.body_max_top_padding,
        set: |c, v| c.ui.layout.reader.body_max_top_padding = v,
        step: 1,
        min: 0,
        max: 20,
        off_label: Some("None"),
        inherit: None,
    },
    SettingRow::Bool {
        label: "Show link URLs",
        description: "Show each link's target URL in faint text after its name.",
        get: |c| c.ui.layout.reader.show_link_urls,
        set: |c, v| c.ui.layout.reader.show_link_urls = v,
        after: AfterChange::None,
        inherit: None,
    },
];

const EDITOR_SETTINGS: &[SettingRow] = &[
    SettingRow::Bool {
        label: "Center body vertically",
        description: "Vertically center the entry body while it fits without scrolling. Off by default — unlike reading, a baseline that shifts as you type is distracting.",
        get: |c| c.ui.layout.editor_body().center_vertically,
        set: |c, v| c.ui.layout.editor.body_center_vertically = Some(v),
        after: AfterChange::None,
        inherit: Some(Inherit {
            is: |c| c.ui.layout.editor.body_center_vertically.is_none(),
            set: |c| c.ui.layout.editor.body_center_vertically = None,
        }),
    },
    SettingRow::Number {
        label: "Max body width",
        description: "Maximum entry-body width in columns; wider panes gutter the sides for readability. Step below the minimum for Unlimited (no cap), then Inherit (follow the reader).",
        get: |c| c.ui.layout.editor_body().max_width,
        set: |c, v| c.ui.layout.editor.body_max_width = Some(v),
        step: 5,
        min: 40,
        max: 240,
        off_label: Some("Unlimited"),
        inherit: Some(Inherit {
            is: |c| c.ui.layout.editor.body_max_width.is_none(),
            set: |c| c.ui.layout.editor.body_max_width = None,
        }),
    },
    SettingRow::Number {
        label: "Max body top padding",
        description: "Extra blank lines above the body on wide panes, ramping in with the side gutters. They scroll away with the text. Step below 0 (None) for Inherit (follow the reader).",
        get: |c| c.ui.layout.editor_body().max_top_padding,
        set: |c, v| c.ui.layout.editor.body_max_top_padding = Some(v),
        step: 1,
        min: 0,
        max: 20,
        off_label: Some("None"),
        inherit: Some(Inherit {
            is: |c| c.ui.layout.editor.body_max_top_padding.is_none(),
            set: |c| c.ui.layout.editor.body_max_top_padding = None,
        }),
    },
    SettingRow::Bool {
        label: "Start in fullscreen",
        description: "Open the entry editor fullscreen, hiding the other columns.",
        get: |c| c.editor.start_fullscreen,
        set: |c, v| c.editor.start_fullscreen = v,
        after: AfterChange::None,
        inherit: None,
    },
    SettingRow::Bool {
        label: "Use location's timezone",
        description: "Stamp a new located entry with its place's timezone instead of the machine's, so travelling doesn't skew its local time.",
        get: |c| c.location.use_location_timezone,
        set: |c, v| c.location.use_location_timezone = v,
        after: AfterChange::None,
        inherit: None,
    },
    SettingRow::Bool {
        label: "Download remote images",
        description: "Fetch images referenced by remote URLs into local attachments when the entry is saved.",
        get: |c| c.attachments.download_remote_images,
        set: |c, v| c.attachments.download_remote_images = v,
        after: AfterChange::None,
        inherit: None,
    },
];

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

    /// Open the settings dialog, seeded on its first setting row (Theme).
    pub(crate) fn open_settings(&mut self) {
        self.overlay = Overlay::Settings(Box::new(SettingsState::new()));
    }

    pub(crate) fn settings_state(&self) -> Option<&SettingsState> {
        match &self.overlay {
            Overlay::Settings(state) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn settings_state_mut(&mut self) -> Option<&mut SettingsState> {
        match &mut self.overlay {
            Overlay::Settings(state) => Some(state),
            _ => None,
        }
    }

    /// Select item `index` (a setting row) in the settings dialog.
    pub(crate) fn settings_select(&mut self, index: usize) {
        if let Some(state) = self.settings_state_mut() {
            state.select_index(index);
        }
    }

    /// Activate the highlighted setting: toggle a bool, open the theme picker, or
    /// (for a number, adjusted with ← / →) do nothing.
    pub(crate) fn settings_activate(&mut self) {
        let Some((_, row)) = self.settings_state().and_then(|s| s.selected_row()) else {
            return;
        };
        match row {
            SettingRow::Theme => {
                self.open_theme_picker();
                // Returning from the picker reopens this dialog, not browse.
                if let Overlay::ThemePicker(state) = &mut self.overlay {
                    state.reopen_settings = true;
                }
            }
            // Cycles the ladder, so enter alone still reaches every state.
            SettingRow::Bool { inherit, .. } if inherit.is_some() => self.settings_adjust(1),
            SettingRow::Bool {
                get, set, after, ..
            } => {
                let next = !get(&self.services.config);
                set(&mut self.services.config, next);
                self.apply_setting_change(*after);
            }
            SettingRow::Number { .. } => {}
        }
    }

    /// Step the highlighted setting one state in `dir` (-1/+1) along its ladder
    /// — `Inherit`, then the off state, then the numbers (or `Off`/`On`) — and
    /// persist. Every row answers to this, so ← / → alone drives the dialog.
    pub(crate) fn settings_adjust(&mut self, dir: i16) {
        let Some((_, row)) = self.settings_state().and_then(|s| s.selected_row()) else {
            return;
        };
        let inheriting = row.inheriting(&self.services.config);
        match row {
            SettingRow::Theme if dir > 0 => self.settings_activate(),
            SettingRow::Theme => {}
            // A two-state ladder wraps either way, so both arrows just flip it.
            SettingRow::Bool {
                get,
                set,
                after,
                inherit: None,
                ..
            } => {
                let next = !get(&self.services.config);
                set(&mut self.services.config, next);
                self.apply_setting_change(*after);
            }
            // Ladder: Inherit, Off, On — wrapping, so enter cycles too.
            SettingRow::Bool {
                get,
                set,
                after,
                inherit: Some(inherit),
                ..
            } => {
                let current = if inheriting {
                    0
                } else if get(&self.services.config) {
                    2
                } else {
                    1
                };
                match (current + dir).rem_euclid(3) {
                    0 => (inherit.set)(&mut self.services.config),
                    1 => set(&mut self.services.config, false),
                    _ => set(&mut self.services.config, true),
                }
                self.apply_setting_change(*after);
            }
            SettingRow::Number {
                get,
                set,
                step,
                min,
                max,
                off_label,
                inherit,
                ..
            } => {
                // The bottom of the ladder, below which only Inherit sits.
                let bottom = if off_label.is_some() { 0 } else { *min };
                if inheriting {
                    if dir > 0 {
                        set(&mut self.services.config, bottom);
                        self.apply_setting_change(AfterChange::None);
                    }
                    return;
                }
                let current = get(&self.services.config);
                if current == bottom && dir < 0 {
                    if let Some(inherit) = inherit {
                        (inherit.set)(&mut self.services.config);
                        self.apply_setting_change(AfterChange::None);
                    }
                    return;
                }
                let next = match off_label {
                    // When the off state (0) sits below `min`: from off up lands
                    // on min; from min down turns off. (Down from off is the
                    // ladder bottom, handled above.) With `min == 0` the off
                    // state is just the min, so normal clamping applies and only
                    // the label changes.
                    Some(_) if *min > 0 && current == 0 && dir > 0 => *min,
                    Some(_) if *min > 0 && current == *min && dir < 0 => 0,
                    _ => {
                        let delta = i32::from(dir) * i32::from(*step);
                        (i32::from(current) + delta).clamp(i32::from(*min), i32::from(*max)) as u16
                    }
                };
                if next != current {
                    set(&mut self.services.config, next);
                    self.apply_setting_change(AfterChange::None);
                }
            }
        }
    }

    /// Persist the config after a setting change, running any follow-up work; a
    /// failed save toasts and leaves the (already in-memory) change in place.
    fn apply_setting_change(&mut self, after: AfterChange) {
        if let Err(err) =
            crate::config::save_config(&self.services.config_path, &self.services.config)
        {
            self.toast(
                ToastVariant::Error,
                format!("Couldn't save config: {}", crate::tui::concise_error(&err)),
            );
            return;
        }
        if after == AfterChange::ReresolveTheme {
            self.apply_effective_theme();
        }
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
            reopen_settings: false,
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
    /// save toasts and keeps the picker open, with both files left as they were.
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
                let previous_ui = self.services.config.ui.clone();
                self.services.config.ui.theme = name.clone();
                self.services.config.ui.color_mode = self.appearance.color_mode;
                self.services.config.ui.chrome =
                    crate::tui::theme::chrome_mode(self.appearance.chrome_override);
                // The config goes first so a failure here has nothing to undo:
                // the journal's override is still on disk, untouched.
                if let Err(err) =
                    crate::config::save_config(&self.services.config_path, &self.services.config)
                {
                    self.services.config.ui = previous_ui;
                    self.toast(
                        ToastVariant::Error,
                        format!("Couldn't save config: {}", crate::tui::concise_error(&err)),
                    );
                    return;
                }
                // Switching a journal to Global removes its own override so it
                // follows the (possibly just-changed) global theme.
                if let Some(journal_name) = journal {
                    if let Err(err) = self.services.store.set_journal_theme(&journal_name, None) {
                        self.services.config.ui = previous_ui;
                        let rollback = crate::config::save_config(
                            &self.services.config_path,
                            &self.services.config,
                        );
                        let mut message =
                            format!("Couldn't clear theme: {}", crate::tui::concise_error(&err));
                        if let Err(err) = rollback {
                            message.push_str(&format!(
                                "; the global theme also couldn't be put back: {}",
                                crate::tui::concise_error(&err)
                            ));
                        }
                        self.toast(ToastVariant::Error, message);
                        return;
                    }
                    self.set_local_journal_theme(&journal_name, None);
                }
                format!("Global theme set to {name}")
            }
        };

        self.apply_effective_theme();
        self.toast(ToastVariant::Success, toast);
        self.close_theme_picker();
    }

    /// Close the theme picker: reopen the settings dialog it was launched from,
    /// or return straight to browse.
    fn close_theme_picker(&mut self) {
        let reopen = matches!(&self.overlay, Overlay::ThemePicker(state) if state.reopen_settings);
        if reopen {
            self.open_settings();
        } else {
            self.close_overlay();
        }
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
        self.close_theme_picker();
    }
}
