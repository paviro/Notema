use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use super::schema::parse;
use super::{DEFAULT_THEME, Mode, Theme};

/// The bundled themes, embedded so the binary can materialize and fall back to
/// them without touching the network or the repo.
pub(super) const BUNDLED: [(&str, &str); 26] = [
    ("journal", include_str!("../themes/journal.toml")),
    ("classic", include_str!("../themes/classic.toml")),
    ("eclipse", include_str!("../themes/eclipse.toml")),
    ("blossom", include_str!("../themes/blossom.toml")),
    ("fjord", include_str!("../themes/fjord.toml")),
    ("grove", include_str!("../themes/grove.toml")),
    ("tokyonight", include_str!("../themes/tokyonight.toml")),
    ("lavender", include_str!("../themes/lavender.toml")),
    ("matcha", include_str!("../themes/matcha.toml")),
    ("indigo", include_str!("../themes/indigo.toml")),
    ("maple", include_str!("../themes/maple.toml")),
    ("celadon", include_str!("../themes/celadon.toml")),
    ("rose-pine", include_str!("../themes/rose-pine.toml")),
    ("dungeon", include_str!("../themes/dungeon.toml")),
    ("synthwave", include_str!("../themes/synthwave.toml")),
    ("crt", include_str!("../themes/crt.toml")),
    ("cyberpunk", include_str!("../themes/cyberpunk.toml")),
    ("vaporwave", include_str!("../themes/vaporwave.toml")),
    ("matrix", include_str!("../themes/matrix.toml")),
    ("tron", include_str!("../themes/tron.toml")),
    ("eldritch", include_str!("../themes/eldritch.toml")),
    ("hal", include_str!("../themes/hal.toml")),
    ("gameboy", include_str!("../themes/gameboy.toml")),
    ("wasteland", include_str!("../themes/wasteland.toml")),
    ("arcade", include_str!("../themes/arcade.toml")),
    ("deep-space", include_str!("../themes/deep-space.toml")),
];

// --- loading ---

/// The directory holding the user-editable theme files, next to `config.toml`.
pub(crate) fn themes_dir(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("themes")
}

/// Write every bundled theme that isn't on disk yet. Existing files are never
/// touched — user edits win over bundled updates.
pub(crate) fn ensure_bundled(dir: &Path) -> Result<()> {
    for (name, text) in BUNDLED {
        let path = dir.join(format!("{name}.toml"));
        if !path.exists() {
            crate::config::write_toml_atomic(&path, text)
                .with_context(|| format!("materializing bundled theme {}", path.display()))?;
        }
    }
    Ok(())
}

/// The toast text shown when the configured theme can't be loaded and the app
/// falls back to the default. Only reached from the non-test theme-install path.
#[cfg(not(test))]
pub(crate) fn format_theme_warning(name: &str, err: &anyhow::Error) -> String {
    format!(
        "Theme '{name}' couldn't load ({}); using default",
        crate::tui::concise_error(err)
    )
}

/// Load the named theme, materializing the bundled files first. On any failure
/// (missing file, bad TOML, unknown color) returns the built-in
/// [`DEFAULT_THEME`] alongside the error so the caller can surface it — the app
/// always starts.
pub(crate) fn load(config_path: &Path, name: &str, mode: Mode) -> (Theme, Option<anyhow::Error>) {
    match try_load(config_path, name, mode) {
        Ok(theme) => (theme, None),
        Err(err) => (
            builtin(DEFAULT_THEME, mode).unwrap_or_else(Theme::terminal_default),
            Some(err),
        ),
    }
}

/// Load and resolve one theme file. Errors carry the path and token context so
/// a typo in a user file names itself.
pub(crate) fn load_file(path: &Path, mode: Mode) -> Result<Theme> {
    let text =
        fs::read_to_string(path).with_context(|| format!("reading theme {}", path.display()))?;
    parse(&text, mode).with_context(|| format!("in theme {}", path.display()))
}

fn try_load(config_path: &Path, name: &str, mode: Mode) -> Result<Theme> {
    let dir = themes_dir(config_path);
    ensure_bundled(&dir)?;
    load_file(&dir.join(format!("{name}.toml")), mode)
}

/// Resolve a bundled theme straight from its embedded text.
pub(super) fn builtin(name: &str, mode: Mode) -> Option<Theme> {
    let (_, text) = BUNDLED.iter().find(|(n, _)| *n == name)?;
    parse(text, mode).ok()
}

/// Query the terminal for its background luminance, defaulting to dark when the
/// terminal doesn't answer.
fn detect_terminal_background() -> Mode {
    match terminal_colorsaurus::theme_mode(terminal_colorsaurus::QueryOptions::default()) {
        Ok(terminal_colorsaurus::ThemeMode::Light) => Mode::Light,
        Ok(terminal_colorsaurus::ThemeMode::Dark) | Err(_) => Mode::Dark,
    }
}

pub(crate) struct StartupTheme {
    pub(crate) theme: Theme,
    pub(crate) detected_mode: Mode,
}

/// Resolve the theme to open with: pick the mode (honoring an explicit
/// dark/light config or the detected terminal background under `Auto`), load it,
/// and apply the configured chrome override.
pub(crate) fn load_startup(config_path: &Path, ui: &crate::config::UiSection) -> StartupTheme {
    use crate::config::ColorMode;

    let detected_mode = detect_terminal_background();
    let mode = match ui.color_mode {
        ColorMode::Dark => Mode::Dark,
        ColorMode::Light => Mode::Light,
        ColorMode::Auto => detected_mode,
    };
    let (theme, _) = load(config_path, &ui.theme, mode);
    StartupTheme {
        theme: theme.with_chrome_override(super::chrome_style(ui.chrome)),
        detected_mode,
    }
}
