use crate::AppResult;
use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

const CONFIG_SCHEMA_VERSION: u32 = 1;
const STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub(crate) schema_version: u32,
    pub journal: JournalSection,
    #[serde(default)]
    pub attachments: AttachmentsSection,
    #[serde(default)]
    pub ui: UiSection,
    #[serde(default)]
    pub editor: EditorSection,
    #[serde(default)]
    pub location: LocationSection,
}

/// Which journals to open and where they live on disk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct JournalSection {
    /// Directory holding every journal. A relative path resolves against the
    /// directory containing `config.toml`, so a config dir can carry its
    /// journal root with it. Always the raw value as written in the file —
    /// resolve through [`Config::journal_root`] so saving never rewrites it.
    pub path: PathBuf,
    /// Journal selected on startup when the previous session didn't record one.
    #[serde(default)]
    pub default: Option<String>,
}

/// How entry attachments are handled.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttachmentsSection {
    /// Fetch images referenced by remote URLs into local attachments on import.
    #[serde(default = "default_true")]
    pub download_remote_images: bool,
}

/// Editor behaviour.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct EditorSection {
    /// Open the entry editor in fullscreen (hiding the other columns) instead
    /// of in-pane.
    #[serde(default)]
    pub start_fullscreen: bool,
}

/// How a new entry's location influences its timestamp.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocationSection {
    /// Stamp a new located entry with the timezone of its place rather than the
    /// machine's, so travelling without changing `TZ` doesn't skew the entry's
    /// local time, date, or sunrise/sunset. Falls back to the system zone when
    /// the location can't be resolved to one.
    #[serde(default = "default_true")]
    pub use_location_timezone: bool,
}

/// TUI presentation preferences.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiSection {
    /// Name of the theme file (without `.toml`) in the config directory's
    /// `themes/` folder. The bundled themes are materialized there on launch.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Whether `{ dark, light }` theme colors follow the detected terminal
    /// background, or are pinned to one variant.
    #[serde(default)]
    pub color_mode: ColorMode,
    /// Chrome style: `default` uses each theme's own `chrome.default_style`; `flat`
    /// or `bordered` force that chrome on every theme.
    #[serde(default)]
    pub chrome: ChromeMode,
    /// Per-device escape hatch: when true, this device ignores the per-journal
    /// themes set in `.journal.toml` sidecars and always uses `theme`. For
    /// low-capability terminals (e-ink) where most themes don't render well.
    #[serde(default)]
    pub ignore_journal_themes: bool,
    #[serde(default)]
    pub layout: LayoutSection,
}

impl Default for UiSection {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            color_mode: ColorMode::default(),
            chrome: ChromeMode::default(),
            ignore_journal_themes: false,
            layout: LayoutSection::default(),
        }
    }
}

fn default_theme() -> String {
    crate::tui::theme::DEFAULT_THEME.to_string()
}

/// Which variant of a theme's `{ dark, light }` colors to use. `Auto` asks the
/// terminal for its background color at startup and falls back to dark.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ColorMode {
    #[default]
    Auto,
    Dark,
    Light,
}

impl ColorMode {
    /// Parse the journal-sidecar spelling; `None` for anything unrecognized (a
    /// newer device's value), so callers fall back to this device's setting.
    /// Must mirror the serde `rename_all = "lowercase"` names.
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "auto" => Some(ColorMode::Auto),
            "dark" => Some(ColorMode::Dark),
            "light" => Some(ColorMode::Light),
            _ => None,
        }
    }

    /// The journal-sidecar spelling, inverse of [`Self::from_name`].
    pub(crate) fn name(self) -> &'static str {
        match self {
            ColorMode::Auto => "auto",
            ColorMode::Dark => "dark",
            ColorMode::Light => "light",
        }
    }
}

/// The `[ui] chrome` setting: `default` follows the active theme's
/// `chrome.default_style`; `flat`/`bordered` force that chrome on every theme.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ChromeMode {
    #[default]
    Default,
    Flat,
    Bordered,
}

impl ChromeMode {
    /// Parse the journal-sidecar spelling; `None` for anything unrecognized (a
    /// newer device's value), so callers fall back to this device's setting.
    /// Must mirror the serde `rename_all = "lowercase"` names.
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "default" => Some(ChromeMode::Default),
            "flat" => Some(ChromeMode::Flat),
            "bordered" => Some(ChromeMode::Bordered),
            _ => None,
        }
    }

    /// The journal-sidecar spelling, inverse of [`Self::from_name`].
    pub(crate) fn name(self) -> &'static str {
        match self {
            ChromeMode::Default => "default",
            ChromeMode::Flat => "flat",
            ChromeMode::Bordered => "bordered",
        }
    }
}

/// Layout geometry: how the panels and their contents are sized. The reader and
/// editor settings sit in their own sub-tables.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct LayoutSection {
    #[serde(default)]
    pub reader: ReaderSection,
    /// Left out of the file entirely while every key inherits, so clearing the
    /// last override doesn't strand an empty table.
    #[serde(
        default,
        skip_serializing_if = "EditorLayoutSection::inherits_everything"
    )]
    pub editor: EditorLayoutSection,
}

impl LayoutSection {
    pub(crate) fn reader_body(&self) -> BodyLayout {
        BodyLayout {
            center_vertically: self.reader.body_center_vertically,
            max_width: self.reader.body_max_width,
            max_top_padding: self.reader.body_max_top_padding,
        }
    }

    pub(crate) fn editor_body(&self) -> BodyLayout {
        let reader = self.reader_body();
        BodyLayout {
            center_vertically: self
                .editor
                .body_center_vertically
                .unwrap_or(reader.center_vertically),
            max_width: self.editor.body_max_width.unwrap_or(reader.max_width),
            max_top_padding: self
                .editor
                .body_max_top_padding
                .unwrap_or(reader.max_top_padding),
        }
    }
}

/// How a pane frames the entry body, resolved for one surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BodyLayout {
    pub center_vertically: bool,
    pub max_width: u16,
    pub max_top_padding: u16,
}

/// How the reader pane presents an entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReaderSection {
    /// Vertically center the body in the reader when it fits without scrolling
    /// and centering sits it lower than `body_max_top_padding`.
    #[serde(default = "default_true")]
    pub body_center_vertically: bool,
    /// Max width, in cells, of the entry body; wider panels gutter the sides so
    /// long-form text stays readable. Metadata keeps the full width.
    #[serde(default = "default_body_max_width")]
    pub body_max_width: u16,
    /// Max blank lines above the entry body on wide panes, ramping in with the
    /// side gutters; 0 (or a pane no wider than `body_max_width`) keeps the body
    /// flush to the top. A floor the body never rises above: centering applies
    /// only where it sits the body lower still.
    #[serde(default = "default_body_max_top_padding")]
    pub body_max_top_padding: u16,
    /// Show each link's target URL as a faint `(url)` after its name. Off by
    /// default — the name is clickable either way, so the URL is just noise.
    #[serde(default)]
    pub show_link_urls: bool,
}

/// How the editor frames the entry body. Each key mirrors one in
/// [`ReaderSection`], and an absent key takes the reader's — so writing and
/// reading an entry look the same unless deliberately split, and turning an
/// override back to Inherit leaves no trace in the file.
///
/// [`Config::new`] seeds `body_center_vertically = false` for a fresh config,
/// since a baseline that shifts as you type is more disorienting than it is
/// while reading. An older config without the key still inherits.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct EditorLayoutSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_center_vertically: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_max_width: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_max_top_padding: Option<u16>,
}

impl EditorLayoutSection {
    /// True while every key inherits, so the whole table can be left out.
    pub(crate) fn inherits_everything(&self) -> bool {
        *self == Self::default()
    }
}

impl Default for AttachmentsSection {
    fn default() -> Self {
        Self {
            download_remote_images: true,
        }
    }
}

impl Default for LocationSection {
    fn default() -> Self {
        Self {
            use_location_timezone: true,
        }
    }
}

impl Default for ReaderSection {
    fn default() -> Self {
        Self {
            body_center_vertically: true,
            body_max_width: default_body_max_width(),
            body_max_top_padding: default_body_max_top_padding(),
            show_link_urls: false,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_body_max_width() -> u16 {
    100
}

fn default_body_max_top_padding() -> u16 {
    6
}

impl Config {
    pub(crate) fn new(journal_root: PathBuf) -> Self {
        let mut ui = UiSection::default();
        // Only a fresh config starts the editor un-centered; see
        // [`EditorLayoutSection`].
        ui.layout.editor.body_center_vertically = Some(false);
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            journal: JournalSection {
                path: expand_tilde(journal_root),
                default: None,
            },
            attachments: AttachmentsSection::default(),
            ui,
            editor: EditorSection::default(),
            location: LocationSection::default(),
        }
    }

    /// The journal root with `~` expanded and a relative path anchored to the
    /// config file's directory. `journal.path` itself stays raw so it survives
    /// a save.
    pub(crate) fn journal_root(&self, config_path: &Path) -> PathBuf {
        resolve_journal_root(self.journal.path.clone(), config_path)
    }
}

/// Per-device, machine-written UI state kept next to `config.toml` in `state.toml`
/// (never synced). Separated from [`Config`] so the user's hand-edited settings
/// stay free of values the app rewrites on its own — and so the file has room to
/// grow as more session state is remembered.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct State {
    pub(crate) schema_version: u32,
    /// The stable id of the journal selected when the TUI last exited, restored on
    /// next launch. Stored by id (not name) so it survives a rename or archive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_journal_id: Option<String>,
    #[serde(default)]
    pub ui: UiState,
}

impl Default for State {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            last_journal_id: None,
            ui: UiState::default(),
        }
    }
}

/// Toggle states for optional TUI chrome, flipped by keybindings and remembered
/// across launches.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiState {
    /// Whether the footer keybinding hints are shown.
    #[serde(default = "default_true")]
    pub show_hints: bool,
    /// Whether the journals panel is shown.
    #[serde(default = "default_true")]
    pub show_journals: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            show_hints: true,
            show_journals: true,
        }
    }
}

pub(crate) fn default_config_path() -> AppResult<PathBuf> {
    // An explicit XDG_CONFIG_HOME always wins, on every platform. Per the XDG
    // spec an empty value counts as unset, so fall through to the default.
    if let Ok(config_home) = env::var("XDG_CONFIG_HOME")
        && !config_home.is_empty()
    {
        return Ok(PathBuf::from(config_home)
            .join("notema")
            .join("config.toml"));
    }

    // macOS keeps app data under Application Support; other Unixes use ~/.config,
    // where the app name is already the namespace.
    #[cfg(target_os = "macos")]
    let dir = env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library/Application Support/de.paviro.notema"))
        .context("HOME is not set")?;
    #[cfg(not(target_os = "macos"))]
    let dir = dirs::home_dir()
        .context("could not determine home directory")?
        .join(".config")
        .join("notema");
    Ok(dir.join("config.toml"))
}

pub(crate) fn load_config(path: &Path) -> AppResult<Config> {
    let text =
        fs::read_to_string(path).with_context(|| format!("reading config {}", path.display()))?;
    let config: Config =
        toml::from_str(&text).with_context(|| format!("parsing config {}", path.display()))?;
    if config.schema_version != CONFIG_SCHEMA_VERSION {
        bail!(
            "unsupported config schema version {}; expected {CONFIG_SCHEMA_VERSION}",
            config.schema_version
        );
    }
    Ok(config)
}

/// Expand `~` and anchor a relative journal root to the config file's
/// directory, so a config dir can reference a journal root beside itself
/// regardless of the process's working directory.
fn resolve_journal_root(root: PathBuf, config_path: &Path) -> PathBuf {
    let root = expand_tilde(root);
    match config_path.parent() {
        Some(dir) if root.is_relative() => dir.join(root),
        _ => root,
    }
}

pub(crate) fn save_config(path: &Path, config: &Config) -> AppResult<()> {
    write_toml_atomic(path, &toml::to_string_pretty(config)?)
}

/// The device's `state.toml`, kept beside `config.toml` in the same directory.
pub(crate) fn state_path(config_path: &Path) -> PathBuf {
    config_path.with_file_name("state.toml")
}

/// Load this device's UI state, defaulting when `state.toml` doesn't exist yet.
pub(crate) fn load_state(config_path: &Path) -> AppResult<State> {
    let path = state_path(config_path);
    if !path.exists() {
        return Ok(State::default());
    }
    let text = fs::read_to_string(&path)?;
    match toml::from_str::<State>(&text) {
        Ok(state) if state.schema_version == STATE_SCHEMA_VERSION => Ok(state),
        Ok(state) => reset_invalid_state(
            &path,
            format!("unsupported schema version {}", state.schema_version),
        ),
        Err(error) => reset_invalid_state(&path, error.to_string()),
    }
}

fn reset_invalid_state(path: &Path, reason: String) -> AppResult<State> {
    let backup = path.with_extension("toml.invalid");
    fs::rename(path, &backup)?;
    eprintln!(
        "Ignored invalid UI state ({reason}); backup saved to {}",
        backup.display()
    );
    Ok(State::default())
}

pub(crate) fn save_state(config_path: &Path, state: &State) -> AppResult<()> {
    write_toml_atomic(&state_path(config_path), &toml::to_string_pretty(state)?)
}

/// Write `text` to `path` atomically: a same-directory temp file plus a rename, so
/// a crash mid-write leaves the previous file intact rather than a truncated one.
/// Backed by the shared write-fsync-rename-fsync primitive, whose temp names are
/// unique per process so two notema instances saving state can't clobber each
/// other's temp.
pub(crate) fn write_toml_atomic(path: &Path, text: &str) -> AppResult<()> {
    Ok(notema_encryption::atomic_write(path, text.as_bytes())?)
}

pub(crate) fn expand_tilde(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if text == "~" {
        return dirs::home_dir().unwrap_or(path);
    }

    if let Some(rest) = text.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }

    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn mode_and_chrome_names_round_trip_and_match_serde() {
        for mode in [ColorMode::Auto, ColorMode::Dark, ColorMode::Light] {
            assert_eq!(ColorMode::from_name(mode.name()), Some(mode));
            // The sidecar spelling must stay the config-file spelling.
            assert_eq!(
                toml::Value::try_from(mode).unwrap().as_str(),
                Some(mode.name())
            );
        }
        for chrome in [ChromeMode::Default, ChromeMode::Flat, ChromeMode::Bordered] {
            assert_eq!(ChromeMode::from_name(chrome.name()), Some(chrome));
            assert_eq!(
                toml::Value::try_from(chrome).unwrap().as_str(),
                Some(chrome.name())
            );
        }
        assert_eq!(ColorMode::from_name("neon"), None);
        assert_eq!(ChromeMode::from_name("neon"), None);
    }

    #[test]
    fn journal_root_expands_tilde_and_keeps_the_raw_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "schema_version = 1\n\n[journal]\npath = \"~/Journals\"\n",
        )
        .unwrap();

        let config = load_config(&path).unwrap();

        assert_eq!(config.journal.path, PathBuf::from("~/Journals"));
        let root = config.journal_root(&path);
        assert!(root.is_absolute());
        assert!(root.ends_with("Journals"));
        assert_eq!(config.journal.default, None);
    }

    #[test]
    fn journal_root_resolves_relative_root_against_config_dir() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "schema_version = 1\n\n[journal]\npath = \"journals\"\n",
        )
        .unwrap();

        let config = load_config(&path).unwrap();

        assert_eq!(config.journal.path, PathBuf::from("journals"));
        assert_eq!(config.journal_root(&path), dir.path().join("journals"));
    }

    #[test]
    fn save_config_preserves_portable_journal_roots() {
        for raw in ["~/Journals", "journals"] {
            let dir = tempdir().unwrap();
            let path = dir.path().join("config.toml");
            fs::write(
                &path,
                format!("schema_version = 1\n\n[journal]\npath = \"{raw}\"\n"),
            )
            .unwrap();

            let config = load_config(&path).unwrap();
            save_config(&path, &config).unwrap();

            let text = fs::read_to_string(&path).unwrap();
            assert!(
                text.contains(&format!("path = \"{raw}\"")),
                "saved config lost the raw form {raw:?}: {text}"
            );
        }
    }

    #[test]
    fn save_and_load_config_round_trips_all_fields() {
        let dir = tempdir().unwrap();
        // A nested path also exercises that save creates missing parent dirs.
        let path = dir.path().join("nested").join("config.toml");
        let mut config = Config::new(dir.path().join("root"));
        config.journal.default = Some("work".to_string());
        config.attachments.download_remote_images = false;
        config.ui.theme = "eclipse".to_string();
        config.ui.color_mode = ColorMode::Light;
        config.ui.layout.reader.body_center_vertically = false;
        config.ui.layout.reader.body_max_width = 80;
        config.ui.layout.editor.body_max_width = Some(120);

        save_config(&path, &config).unwrap();
        let loaded = load_config(&path).unwrap();

        assert_eq!(loaded, config);
    }

    #[test]
    fn missing_optional_fields_use_defaults() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "schema_version = 1\n\n[journal]\npath = \"~/Journals\"\n",
        )
        .unwrap();

        let config = load_config(&path).unwrap();

        assert!(config.attachments.download_remote_images);
        assert_eq!(config.ui.theme, crate::tui::theme::DEFAULT_THEME);
        assert_eq!(config.ui.color_mode, ColorMode::Auto);
        assert!(config.ui.layout.reader.body_center_vertically);
        assert_eq!(config.ui.layout.reader.body_max_width, 100);
        assert_eq!(config.ui.layout.editor, EditorLayoutSection::default());
    }

    /// An absent editor key takes the reader's; a set one wins.
    #[test]
    fn editor_body_layout_falls_back_to_the_reader() {
        let mut layout = LayoutSection::default();
        layout.reader.body_max_width = 80;
        layout.reader.body_max_top_padding = 4;
        assert_eq!(layout.editor_body(), layout.reader_body());

        layout.editor.body_max_top_padding = Some(0);
        assert!(!layout.editor.inherits_everything());
        assert_eq!(layout.editor_body().max_width, 80);
        assert_eq!(layout.editor_body().max_top_padding, 0);

        layout.editor = EditorLayoutSection::default();
        assert_eq!(layout.editor_body(), layout.reader_body());
    }

    /// A fresh config starts the editor un-centered; clearing every override
    /// takes the whole table back out of the file.
    #[test]
    fn editor_layout_table_is_written_only_while_overridden() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = Config::new(dir.path().join("root"));
        assert!(!config.ui.layout.editor_body().center_vertically);

        save_config(&path, &config).unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(
            written.contains("body_center_vertically = false"),
            "{written}"
        );
        assert_eq!(load_config(&path).unwrap(), config);

        config.ui.layout.editor.body_center_vertically = None;
        save_config(&path, &config).unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(!written.contains("[ui.layout.editor]"), "{written}");
        let loaded = load_config(&path).unwrap();
        assert_eq!(
            loaded.ui.layout.editor_body(),
            loaded.ui.layout.reader_body()
        );
    }

    #[test]
    fn save_and_load_state_round_trips_and_defaults_when_missing() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("config.toml");

        // Missing state.toml loads as the default: no journal remembered, chrome shown.
        let default = load_state(&config_path).unwrap();
        assert_eq!(default, State::default());
        assert!(default.ui.show_hints);
        assert!(default.ui.show_journals);

        let state = State {
            schema_version: STATE_SCHEMA_VERSION,
            last_journal_id: Some("home1234".to_string()),
            ui: UiState {
                show_hints: false,
                show_journals: true,
            },
        };
        save_state(&config_path, &state).unwrap();

        assert_eq!(state_path(&config_path), dir.path().join("state.toml"));
        assert_eq!(load_state(&config_path).unwrap(), state);
    }
}
