//! The UI's semantic style seam. Widgets ask the theme for *meaning*
//! (`heading`, `positive`, `primary`, …) and get back a ratatui [`Style`], never
//! a bare [`Color`]. Themes are TOML files in `<config-dir>/themes/`; the
//! bundled ones are materialized there on first launch and stay user-editable.
//!
//! Every color in a theme file may be a single value or a `{ dark, light }`
//! pair; resolution against the terminal's detected [`Mode`] happens once at
//! load, so rendering only borrows an already-resolved [`Theme`].
//!
//! Monochrome contract: the modifiers that carry meaning (bold on signed
//! values, dim on secondary ink, inversion on selection fallbacks) are applied
//! in code, not read from theme data, so no theme file can make a positive
//! value render as plain body text on eclipse.

mod accessors;
mod loading;
mod schema;
#[cfg(test)]
mod tests;

use loading::builtin;
#[cfg(not(test))]
pub(crate) use loading::format_theme_warning;
pub(crate) use loading::{StartupTheme, ensure_bundled, load, load_file, load_startup, themes_dir};
use ratatui::style::{Color, Modifier, Style};
#[cfg(test)]
use schema::parse;
use serde::Deserialize;
#[cfg(test)]
use {loading::BUNDLED, std::fs};

/// The theme `load` falls back to when the configured one is missing or broken.
pub(crate) const DEFAULT_THEME: &str = "journal";

/// Which variant of a `{ dark, light }` color a load resolves to. Detected from
/// the terminal background once at startup and cached for the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Dark,
    Light,
}

/// A chart fill: which glyph is repeated and how it is styled. Eclipse themes
/// vary the glyph per series so data stays readable without hue.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Fill {
    pub(crate) glyph: char,
    pub(crate) style: Style,
}

/// How a theme wants its chrome drawn: `Flat` separates surfaces by background
/// layers (opencode-style), `Bordered` keeps the classic drawn borders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ChromeStyle {
    Flat,
    Bordered,
}

pub(crate) const fn chrome_style(mode: crate::config::ChromeMode) -> Option<ChromeStyle> {
    match mode {
        crate::config::ChromeMode::Default => None,
        crate::config::ChromeMode::Flat => Some(ChromeStyle::Flat),
        crate::config::ChromeMode::Bordered => Some(ChromeStyle::Bordered),
    }
}

pub(crate) const fn chrome_mode(style: Option<ChromeStyle>) -> crate::config::ChromeMode {
    match style {
        None => crate::config::ChromeMode::Default,
        Some(ChromeStyle::Flat) => crate::config::ChromeMode::Flat,
        Some(ChromeStyle::Bordered) => crate::config::ChromeMode::Bordered,
    }
}

/// How the reader's metadata chips (feelings, people, activities, tags) are
/// drawn: `Reversed` inverts the value's cell (the e-ink/classic look), `Bg`
/// fills with the per-category pill colors, `Bracket` is plain `[value]` text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PillStyle {
    #[default]
    Reversed,
    Bg,
    Bracket,
}

/// Which metadata chip category a pill styles — the render-side key into the
/// theme's per-category pill colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PillCategory {
    Feelings,
    People,
    Activities,
    Tags,
}

/// The line character set a theme draws boxes with: panel and dialog borders,
/// the hand-drawn entry/journal cards, and table grids.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum BorderGlyphs {
    #[default]
    Plain,
    Rounded,
    Double,
    Thick,
    Ascii,
    /// A theme-authored set (`[borders.glyphs]`), assembled by the schema.
    /// Never spellable as `style = "custom"` — it only exists resolved.
    #[serde(skip)]
    Custom(std::sync::Arc<CustomBorderSet>),
}

/// Owned ratatui border and line glyph sets resolved from `[borders.glyphs]`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CustomBorderSet {
    pub(super) border: OwnedBorderSet,
    pub(super) line: OwnedLineSet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OwnedBorderSet {
    pub(super) top_left: String,
    pub(super) top_right: String,
    pub(super) bottom_left: String,
    pub(super) bottom_right: String,
    pub(super) vertical_left: String,
    pub(super) vertical_right: String,
    pub(super) horizontal_top: String,
    pub(super) horizontal_bottom: String,
}

impl OwnedBorderSet {
    pub(super) fn from_set(set: ratatui::symbols::border::Set<'_>) -> Self {
        Self {
            top_left: set.top_left.to_string(),
            top_right: set.top_right.to_string(),
            bottom_left: set.bottom_left.to_string(),
            bottom_right: set.bottom_right.to_string(),
            vertical_left: set.vertical_left.to_string(),
            vertical_right: set.vertical_right.to_string(),
            horizontal_top: set.horizontal_top.to_string(),
            horizontal_bottom: set.horizontal_bottom.to_string(),
        }
    }

    fn as_set(&self) -> ratatui::symbols::border::Set<'_> {
        ratatui::symbols::border::Set {
            top_left: &self.top_left,
            top_right: &self.top_right,
            bottom_left: &self.bottom_left,
            bottom_right: &self.bottom_right,
            vertical_left: &self.vertical_left,
            vertical_right: &self.vertical_right,
            horizontal_top: &self.horizontal_top,
            horizontal_bottom: &self.horizontal_bottom,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OwnedLineSet {
    pub(super) vertical: String,
    pub(super) horizontal: String,
    pub(super) top_right: String,
    pub(super) top_left: String,
    pub(super) bottom_right: String,
    pub(super) bottom_left: String,
    pub(super) vertical_left: String,
    pub(super) vertical_right: String,
    pub(super) horizontal_down: String,
    pub(super) horizontal_up: String,
    pub(super) cross: String,
}

impl OwnedLineSet {
    pub(super) fn from_set(set: ratatui::symbols::line::Set<'_>) -> Self {
        Self {
            vertical: set.vertical.to_string(),
            horizontal: set.horizontal.to_string(),
            top_right: set.top_right.to_string(),
            top_left: set.top_left.to_string(),
            bottom_right: set.bottom_right.to_string(),
            bottom_left: set.bottom_left.to_string(),
            vertical_left: set.vertical_left.to_string(),
            vertical_right: set.vertical_right.to_string(),
            horizontal_down: set.horizontal_down.to_string(),
            horizontal_up: set.horizontal_up.to_string(),
            cross: set.cross.to_string(),
        }
    }

    fn as_set(&self) -> ratatui::symbols::line::Set<'_> {
        ratatui::symbols::line::Set {
            vertical: &self.vertical,
            horizontal: &self.horizontal,
            top_right: &self.top_right,
            top_left: &self.top_left,
            bottom_right: &self.bottom_right,
            bottom_left: &self.bottom_left,
            vertical_left: &self.vertical_left,
            vertical_right: &self.vertical_right,
            horizontal_down: &self.horizontal_down,
            horizontal_up: &self.horizontal_up,
            cross: &self.cross,
        }
    }
}

/// The `+-|` sets for [`BorderGlyphs::Ascii`], for terminals or looks that
/// want no box-drawing characters at all.
const ASCII_BORDER_SET: ratatui::symbols::border::Set<'static> = ratatui::symbols::border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

const ASCII_LINE_SET: ratatui::symbols::line::Set<'static> = ratatui::symbols::line::Set {
    vertical: "|",
    horizontal: "-",
    top_right: "+",
    top_left: "+",
    bottom_right: "+",
    bottom_left: "+",
    vertical_left: "+",
    vertical_right: "+",
    horizontal_down: "+",
    horizontal_up: "+",
    cross: "+",
};

impl BorderGlyphs {
    /// The set ratatui `Block` borders draw with.
    pub(crate) fn border_set(&self) -> ratatui::symbols::border::Set<'_> {
        use ratatui::symbols::border;
        match self {
            BorderGlyphs::Plain => border::PLAIN,
            BorderGlyphs::Rounded => border::ROUNDED,
            BorderGlyphs::Double => border::DOUBLE,
            BorderGlyphs::Thick => border::THICK,
            BorderGlyphs::Ascii => ASCII_BORDER_SET,
            BorderGlyphs::Custom(set) => set.border.as_set(),
        }
    }

    /// The full line set (corners, junctions, cross) for hand-drawn boxes and
    /// table grids.
    pub(crate) fn line_set(&self) -> ratatui::symbols::line::Set<'_> {
        use ratatui::symbols::line;
        match self {
            BorderGlyphs::Plain => line::NORMAL,
            BorderGlyphs::Rounded => line::ROUNDED,
            BorderGlyphs::Double => line::DOUBLE,
            BorderGlyphs::Thick => line::THICK,
            BorderGlyphs::Ascii => ASCII_LINE_SET,
            BorderGlyphs::Custom(set) => set.line.as_set(),
        }
    }

    /// The `Block` border set for a panel, thickened when focused — thickness
    /// is how focus survives monochrome. Ascii and custom sets have no thick
    /// variant; there focus is carried by the bold border style alone.
    pub(crate) fn block_set(&self, focused: bool) -> ratatui::symbols::border::Set<'_> {
        let promotes = matches!(
            self,
            BorderGlyphs::Plain
                | BorderGlyphs::Rounded
                | BorderGlyphs::Double
                | BorderGlyphs::Thick
        );
        if focused && promotes {
            ratatui::symbols::border::THICK
        } else {
            self.border_set()
        }
    }
}

/// Resolved syntax-highlight colors for fenced code blocks. `Reset` means the
/// category renders in the plain code style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Syntax {
    pub(crate) comment: Color,
    pub(crate) keyword: Color,
    pub(crate) string: Color,
    pub(crate) string_escape: Color,
    pub(crate) number: Color,
    pub(crate) constant: Color,
    pub(crate) function: Color,
    pub(crate) r#type: Color,
    pub(crate) variable: Color,
    pub(crate) property: Color,
    pub(crate) operator: Color,
    pub(crate) punctuation: Color,
    pub(crate) attribute: Color,
    pub(crate) tag: Color,
    pub(crate) label: Color,
    pub(crate) error: Color,
}

impl Syntax {
    /// Whether the theme colors any category at all. Plain themes skip the
    /// highlighter entirely, keeping their classic un-highlighted code blocks.
    pub(crate) fn any_color(self) -> bool {
        // Keep this list in sync with the struct fields.
        [
            self.comment,
            self.keyword,
            self.string,
            self.string_escape,
            self.number,
            self.constant,
            self.function,
            self.r#type,
            self.variable,
            self.property,
            self.operator,
            self.punctuation,
            self.attribute,
            self.tag,
            self.label,
            self.error,
        ]
        .into_iter()
        .any(|color| color != Color::Reset)
    }
}

/// The resolved `[metadata]` section: pill and air-quality styles plus the
/// environment strip's glyph vocabulary.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MetadataTheme {
    pub(super) pill_style: PillStyle,
    pub(super) pill_feelings: Style,
    pub(super) pill_people: Style,
    pub(super) pill_activities: Style,
    pub(super) pill_tags: Style,
    pub(super) aqi_poor: Style,
    pub(super) aqi_very_poor: Style,
    pub(super) aqi_extremely_poor: Style,
    pub(super) pollen_high: Style,
    pub(super) mood_negative: Style,
    pub(super) mood_positive: Style,
    pub(super) glyphs: EnvGlyphs,
}

/// The eighths ramps a theme's column charts, histograms, and sparklines fill
/// with. Held behind a `&'static` (like [`MetadataTheme`]) so [`Glyphs`] stays
/// a cheap `Copy` even though the ramps are 13 chars.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChartRamps {
    /// Bars growing *up* from a baseline: index 0 blank, 8 a full cell.
    pub(crate) up: [char; 9],
    /// Bars hanging *below* a baseline, quantised to the four universally-drawn
    /// upper block glyphs: index 0 blank, 3 a full cell.
    pub(crate) down: [char; 4],
}

/// The markdown reader's structural chrome — the code-fence frame and the
/// quote/code left rails. Multi-character (a rail is `│ `, a fence corner `╭─`),
/// so held behind a `&'static` to keep [`Glyphs`] a cheap `Copy`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkdownGlyphs {
    /// The left rail of a blockquote (`markdown.glyphs.quote_rail`).
    pub(crate) quote_rail: String,
    /// The left rail of a fenced code block (`markdown.glyphs.code_rail`).
    pub(crate) code_rail: String,
    /// The top of a code fence, before the language label (`markdown.glyphs.code_top`).
    pub(crate) code_top: String,
    /// The bottom of a code fence (`markdown.glyphs.code_bottom`).
    pub(crate) code_bottom: String,
    /// The unordered-list bullet (`markdown.glyphs.bullet`).
    pub(crate) bullet: char,
    /// The done / to-do task checkboxes (`markdown.glyphs.task_done` / `task_todo`).
    pub(crate) task_done: String,
    pub(crate) task_todo: String,
    /// The GitHub-alert icons (`[markdown.glyphs.alert]`).
    pub(crate) alert: AlertGlyphs,
}

/// The icon leading each GitHub-style alert blockquote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct AlertGlyphs {
    pub(crate) note: char,
    pub(crate) tip: char,
    pub(crate) important: char,
    pub(crate) warning: char,
    pub(crate) caution: char,
}

/// The environment strip's glyphs (`[metadata.glyphs]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EnvGlyphs {
    /// The full-width rule above the metadata block (`metadata.glyphs.rule`).
    pub(crate) rule: char,
    /// The dot between strip items; always rendered with a space each side so
    /// the strip's width math stays fixed.
    pub(crate) separator: char,
    /// The marker leading the location item.
    pub(crate) location: char,
    /// The sunrise marker inside the sun item.
    pub(crate) sunrise: char,
    /// The sunset marker inside the sun item.
    pub(crate) sunset: char,
    /// The dot leading the air-quality badge.
    pub(crate) aqi: char,
    /// The marker leading the high-pollen badge.
    pub(crate) pollen: char,
    /// The mood bar's filled cells; the valence hue rides
    /// `metadata.environment.mood_negative`/`mood_positive`.
    pub(crate) mood_fill: char,
    /// The mood bar's empty cells. The center marker stays the shared
    /// `charts.glyphs.diverge_center` (its heavy at-zero variant is code-side —
    /// weight is the meaning).
    pub(crate) mood_track: char,
    /// The glyph leading each chip pill, by category.
    pub(crate) feelings: char,
    pub(crate) people: char,
    pub(crate) activities: char,
    pub(crate) tags: char,
    /// The weather glyph per condition slug (`[metadata.glyphs.weather]`).
    pub(crate) weather: WeatherGlyphs,
    /// The moon glyph per phase slug (`[metadata.glyphs.moon]`).
    pub(crate) moon: MoonGlyphs,
}

/// The environment strip's weather glyph per condition slug the context
/// provider emits (`[metadata.glyphs.weather]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WeatherGlyphs {
    pub(super) clear: char,
    pub(super) mostly_clear: char,
    pub(super) partly_cloudy: char,
    pub(super) cloudy: char,
    pub(super) fog: char,
    pub(super) drizzle: char,
    pub(super) rain: char,
    pub(super) snow: char,
    pub(super) thunderstorm: char,
}

impl WeatherGlyphs {
    /// The glyph for a stored condition slug; `None` for slugs this build
    /// doesn't know (future providers), which render without a glyph.
    pub(crate) fn for_slug(self, slug: &str) -> Option<char> {
        Some(match slug {
            "clear" => self.clear,
            "mostly-clear" => self.mostly_clear,
            "partly-cloudy" => self.partly_cloudy,
            "cloudy" => self.cloudy,
            "fog" => self.fog,
            "drizzle" => self.drizzle,
            "rain" => self.rain,
            "snow" => self.snow,
            "thunderstorm" => self.thunderstorm,
            _ => return None,
        })
    }
}

/// The environment strip's moon glyph per phase slug the celestial provider
/// emits (`[metadata.glyphs.moon]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MoonGlyphs {
    pub(super) new: char,
    pub(super) waxing_crescent: char,
    pub(super) first_quarter: char,
    pub(super) waxing_gibbous: char,
    pub(super) full: char,
    pub(super) waning_gibbous: char,
    pub(super) last_quarter: char,
    pub(super) waning_crescent: char,
}

impl MoonGlyphs {
    /// The glyph for a stored phase slug; `None` for unknown slugs.
    pub(crate) fn for_slug(self, slug: &str) -> Option<char> {
        Some(match slug {
            "new" => self.new,
            "waxing-crescent" => self.waxing_crescent,
            "first-quarter" => self.first_quarter,
            "waxing-gibbous" => self.waxing_gibbous,
            "full" => self.full,
            "waning-gibbous" => self.waning_gibbous,
            "last-quarter" => self.last_quarter,
            "waning-crescent" => self.waning_crescent,
            _ => return None,
        })
    }
}

/// The theme's identity glyphs — every meaning-free character the UI repeats.
/// Meaning-carrying glyph *variance* (heavy vs light at a zero mood, distinct
/// chart-series glyphs) stays in code and [`Fill`]s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Glyphs {
    /// The stripe down a focused panel's left edge (flat chrome).
    pub(crate) focus_stripe: char,
    /// The accent edges of a toast card (flat chrome).
    pub(crate) toast_edge: char,
    /// The dismissal countdown line along a toast's bottom edge; the filled
    /// span shrinks as the toast nears its deadline.
    pub(crate) toast_progress: char,
    /// The separator between tab labels; always rendered with a space each
    /// side so the strip's width math stays fixed.
    pub(crate) tab_separator: char,
    /// The rule of section dividers (month headers, "Archived").
    pub(crate) divider: char,
    /// The plain full-width rule separating dialog sections (`borders.glyphs.separator`).
    pub(crate) separator: char,
    /// The zero-line tick shown in the gaps/edges of a signed column chart
    /// (`charts.glyphs.baseline`).
    pub(crate) chart_baseline: char,
    /// The zero-line drawn directly under each column (`charts.glyphs.rule`).
    pub(crate) chart_rule: char,
    /// The empty cell of a diverging (Δ / mood) bar (`charts.glyphs.diverge_track`).
    pub(crate) diverge_track: char,
    /// The center pivot of a diverging bar (`charts.glyphs.diverge_center`). The
    /// heavy variant shown at an exact zero stays code-side (weight carries
    /// meaning).
    pub(crate) diverge_center: char,
    /// The eighths ramps for vertical bars (`charts.glyphs.ramp_up`/`ramp_down`).
    pub(crate) ramps: std::sync::Arc<ChartRamps>,
    /// The scrollbar's draggable handle (`glyphs.scrollbar_thumb`).
    pub(crate) scrollbar_thumb: char,
    /// The scrollbar's track behind the handle (`glyphs.scrollbar_track`).
    pub(crate) scrollbar_track: char,
    /// The arrow capping the scrollbar's top (`glyphs.scrollbar_up`).
    pub(crate) scrollbar_up: char,
    /// The arrow capping the scrollbar's bottom (`glyphs.scrollbar_down`).
    pub(crate) scrollbar_down: char,
    /// The disclosure marker for an expanded/collapsed group (`indicators.glyphs`).
    pub(crate) expanded: char,
    pub(crate) collapsed: char,
    /// The marker trailing a starred entry (`indicators.glyphs.starred`).
    pub(crate) starred: char,
    /// The multi-character markdown chrome (fence frame, quote/code rails).
    pub(crate) markdown: std::sync::Arc<MarkdownGlyphs>,
    /// The box-drawing set for borders, cards, and table grids (`borders.style`
    /// or `[borders.glyphs]`).
    pub(crate) borders: BorderGlyphs,
    /// What a focused panel's border is drawn with (`borders.focused_style` /
    /// `[borders.focused_glyphs]`). `None` keeps the classic thick promotion.
    pub(crate) focused_borders: Option<BorderGlyphs>,
}

impl Glyphs {
    /// The border set for a panel: the theme's focus override when focused,
    /// otherwise the base set with its thick promotion.
    pub(crate) fn block_set(&self, focused: bool) -> ratatui::symbols::border::Set<'_> {
        match (focused, self.focused_borders.as_ref()) {
            (true, Some(borders)) => borders.border_set(),
            _ => self.borders.block_set(focused),
        }
    }
}

/// A fully resolved theme: plain styles and colors, no variants left.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Theme {
    base: Color,
    content: Color,
    dialog: Color,
    raised: Color,
    footer: Color,
    text: Style,
    muted: Style,
    heading: Style,
    placeholder: Style,
    primary: Style,
    secondary: Style,
    border_subtle: Style,
    border_active: Style,
    border_inactive: Style,
    divider: Style,
    card_border: Style,
    tab_separator: Style,
    success: Style,
    warning: Style,
    error: Style,
    info: Style,
    selection: Style,
    hover: Style,
    button: Style,
    button_hover: Style,
    key_hint: Style,
    cursor: Style,
    cursor_line: Style,
    scrollbar_thumb: Style,
    scrollbar_track: Style,
    scrollbar_arrow: Style,
    chart_positive: Fill,
    chart_neutral: Fill,
    chart_negative: Fill,
    chart_bar: Fill,
    chart_track: Fill,
    chart_baseline: Style,
    chart_label: Style,
    md_heading: Style,
    md_heading2: Style,
    md_subheading: Style,
    md_link: Style,
    md_code: Style,
    md_inline_code: Style,
    md_blockquote: Style,
    md_highlight: Style,
    syntax: Syntax,
    metadata: std::sync::Arc<MetadataTheme>,
    glyphs: Glyphs,
    chrome: ChromeStyle,
    scrim: f32,
}

impl Theme {
    pub(crate) fn with_chrome_override(mut self, style: Option<ChromeStyle>) -> Self {
        if let Some(style) = style {
            self.chrome = style;
        }
        self
    }
}

/// The resolved bundled default theme for flat-chrome render tests.
#[cfg(test)]
pub(crate) fn test_flat_theme() -> Theme {
    builtin(DEFAULT_THEME, Mode::Dark).expect("bundled default theme resolves")
}

/// The resolved bundled eclipse theme, for tests asserting the monochrome
/// glyph-differentiation contract end to end.
#[cfg(test)]
pub(crate) fn test_eclipse_theme() -> Theme {
    builtin("eclipse", Mode::Dark).expect("bundled eclipse theme resolves")
}

/// Resolve a theme snippet (dark mode) for tests that pin specific tokens.
#[cfg(test)]
pub(crate) fn test_theme_from_toml(text: &str) -> Theme {
    parse(&format!("schema_version = 1\n{text}"), Mode::Dark).expect("test theme snippet resolves")
}
