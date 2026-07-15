use super::*;

impl Theme {
    /// The look the app has always had on a bare terminal: default colors,
    /// bordered chrome, meaning carried by modifiers. Resolved from the
    /// bundled `classic.toml` — the e-ink/no-assumptions theme, which also
    /// swaps the metadata glyphs to ASCII — so the fallback for a missing or
    /// broken theme never assumes more than a bare terminal renders. A test
    /// pins the two to each other in both modes.
    pub(crate) fn terminal_default() -> Self {
        builtin("classic", Mode::Dark).expect("bundled classic theme resolves")
    }

    // --- surfaces ---

    /// The bottom surface layer, painted under every frame: app margins,
    /// full-screen modal screens, and the footer's default.
    pub(crate) fn base_bg(&self) -> Color {
        self.base
    }

    /// The main content panels (entries, journals, insights, the entry viewer),
    /// and toasts on bordered chrome. Defaults to the base surface.
    pub(crate) fn content_bg(&self) -> Color {
        self.content
    }

    /// Dialog surfaces, defaulting to the content surface unless a theme splits them.
    pub(crate) fn dialog_bg(&self) -> Color {
        self.dialog
    }

    /// Raised items sitting on a panel: inputs, cards, list rows, status bars.
    pub(crate) fn raised_bg(&self) -> Color {
        self.raised
    }

    /// The hint/footer bar. Defaults to the base surface, so a theme can tint
    /// the footer separately or leave it flush with the backdrop.
    pub(crate) fn footer_bg(&self) -> Color {
        self.footer
    }

    // --- text ---

    /// Primary body text.
    pub(crate) fn text(&self) -> Style {
        self.text
    }

    /// Section titles and emphasised labels. Bold in every theme — weight is
    /// how headings survive monochrome — but the ink is the theme's to pick.
    pub(crate) fn heading(&self) -> Style {
        self.heading.add_modifier(Modifier::BOLD)
    }

    /// Secondary text: captions, units, "+k more", empty hints.
    pub(crate) fn muted(&self) -> Style {
        self.muted.add_modifier(Modifier::DIM)
    }

    /// Placeholder text in empty inputs. Dim in every theme so a prompt never
    /// reads as entered text.
    pub(crate) fn placeholder(&self) -> Style {
        self.placeholder.add_modifier(Modifier::DIM)
    }

    // --- accents ---

    /// The primary accent as a style: focused titles, current-item markers.
    pub(crate) fn primary(&self) -> Style {
        self.primary
    }

    /// The second accent hue: the active tab, and anywhere a theme wants a hero
    /// color distinct from `primary`. Defaults to `primary` when a theme sets no
    /// `accents.secondary`, so single-accent themes are unaffected. A third hue,
    /// `accents.tertiary`, has no dedicated render site but is seeded as a
    /// palette name (`fg = "tertiary"`) alongside `primary`/`secondary`.
    pub(crate) fn secondary(&self) -> Style {
        self.secondary
    }

    // --- signed / status values ---

    /// A positive/above-zero value. Bold in every theme so it survives
    /// monochrome; sign and bar direction carry the meaning.
    pub(crate) fn positive(&self) -> Style {
        self.chart_positive.style
    }

    /// A negative/below-zero value. Bold; see [`Self::positive`].
    pub(crate) fn negative(&self) -> Style {
        self.chart_negative.style
    }

    /// A neutral/at-zero value.
    pub(crate) fn neutral(&self) -> Style {
        Style::default()
    }

    /// Style a signed value (mood, mood delta, trend) by its sign. The single
    /// place +/- becomes a style, so the whole panel stays consistent.
    pub(crate) fn signed(&self, value: f32) -> Style {
        if value > 0.0 {
            self.positive()
        } else if value < 0.0 {
            self.negative()
        } else {
            self.neutral()
        }
    }

    /// A success/confirmation state (toasts, status glyphs).
    pub(crate) fn success(&self) -> Style {
        self.success
    }

    /// A warning state.
    pub(crate) fn warning(&self) -> Style {
        self.warning
    }

    /// An error state.
    pub(crate) fn error(&self) -> Style {
        self.error
    }

    /// An informational state.
    pub(crate) fn info(&self) -> Style {
        self.info
    }

    // --- interaction ---

    /// The selected row/item. Flat themes fill with the primary hue and an
    /// explicit contrast foreground; the fallback stays REVERSED so selection
    /// reads without color.
    pub(crate) fn selection(&self) -> Style {
        self.selection
    }

    /// The row/chip under the mouse cursor. Defaults to the element surface,
    /// which resolves to the terminal default on classic/bordered themes (no
    /// visible hover) and to a subtle lift on flat themes. Layers under the
    /// row's existing ink, so no contrast foreground is required.
    pub(crate) fn hover(&self) -> Style {
        self.hover
    }

    /// A primary action button chip.
    pub(crate) fn button(&self) -> Style {
        self.button
    }

    /// The style patched onto a button chip under the mouse. Defaults to an
    /// underline; a theme can restyle it via `interaction.button_hover`.
    pub(crate) fn button_hover(&self) -> Style {
        self.button_hover
    }

    /// A keybinding chip/hint in the footer and dialogs.
    pub(crate) fn key_hint(&self) -> Style {
        self.key_hint
    }

    /// The editor/input cursor while *not* selecting. The REVERSED block shown
    /// during a selection stays code-enforced so a selection always reads.
    pub(crate) fn cursor(&self) -> Style {
        self.cursor
    }

    /// The line under the cursor in the multi-line editor. Defaults to no
    /// highlight; themes may add a subtle background tint.
    pub(crate) fn cursor_line(&self) -> Style {
        self.cursor_line
    }

    /// The active tab in the tab strip while the panel is focused: the secondary
    /// accent + bold on flat chrome (a theme can split it from the primary hue
    /// its titles use; it falls back to primary), selection-styled on bordered
    /// chrome so it reads even without colour. Unfocused it's just bold either
    /// way, so it still stands apart from the muted inactive tabs.
    pub(crate) fn active_tab(&self, focused: bool) -> Style {
        if !focused {
            return Style::default().add_modifier(Modifier::BOLD);
        }
        if self.chrome == ChromeStyle::Flat {
            self.secondary().add_modifier(Modifier::BOLD)
        } else {
            self.selection.add_modifier(Modifier::BOLD)
        }
    }

    /// A non-active tab.
    pub(crate) fn inactive_tab(&self) -> Style {
        self.muted()
    }

    /// The ink of the separator glyph between tab labels. Defaults to the muted
    /// ink unless a theme sets `tabs.separator`.
    pub(crate) fn tab_separator(&self) -> Style {
        self.tab_separator
    }

    // --- borders ---

    /// The border of the focused panel, paired with its thick border type so
    /// focus reads without colour.
    pub(crate) fn focus_border(&self) -> Style {
        self.border_active.add_modifier(Modifier::BOLD)
    }

    /// The border of an unfocused panel; pairs with [`Self::focus_border`].
    pub(crate) fn inactive_border(&self) -> Style {
        self.border_inactive
    }

    /// The frame of a dialog or full-screen modal: the active surface's hue
    /// without the focused panel's bold weight.
    pub(crate) fn dialog_border(&self) -> Style {
        self.border_active
    }

    /// The inter-row grid lines of a table, drawn fainter than the outer
    /// borders and header rule so the rows separate without the grid competing
    /// with the data.
    pub(crate) fn faint_rule(&self) -> Style {
        self.border_subtle
    }

    /// The rule of a section divider (month headers, the "Archived" break).
    /// Defaults to the muted ink; a theme can give it a hue via
    /// `borders.divider`.
    pub(crate) fn divider(&self) -> Style {
        self.divider
    }

    /// A recessed box outline — a touch brighter than [`Self::faint_rule`] so
    /// card and panel borders read as present-but-quiet. Defaults to the normal
    /// border unless a theme sets `borders.card`.
    pub(crate) fn card_border(&self) -> Style {
        self.card_border
    }

    /// The scrollbar's draggable thumb. Recedes when its panel is unfocused,
    /// mirroring how the border quiets — so a background panel's bar doesn't
    /// compete with the focused one.
    pub(crate) fn scrollbar_thumb(&self, focused: bool) -> Style {
        Self::recede_scrollbar(self.scrollbar_thumb, focused)
    }

    /// The scrollbar's track behind the thumb.
    pub(crate) fn scrollbar_track(&self, focused: bool) -> Style {
        Self::recede_scrollbar(self.scrollbar_track, focused)
    }

    /// The scrollbar's up/down arrow caps. Defaults to the thumb hue unless a
    /// theme sets `scrollbar.arrow`.
    pub(crate) fn scrollbar_arrow(&self, focused: bool) -> Style {
        Self::recede_scrollbar(self.scrollbar_arrow, focused)
    }

    /// Dim a scrollbar style for an unfocused panel. Drops any bold weight and
    /// adds `DIM` so the bar visibly recedes even under the terminal-default
    /// theme, where the parts carry no colour of their own.
    fn recede_scrollbar(style: Style, focused: bool) -> Style {
        if focused {
            style
        } else {
            style
                .remove_modifier(Modifier::BOLD)
                .add_modifier(Modifier::DIM)
        }
    }

    // --- charts ---

    /// The filled part of count/frequency bars.
    pub(crate) fn chart_bar(&self) -> Fill {
        self.chart_bar
    }

    /// The empty remainder of a bar.
    pub(crate) fn chart_track(&self) -> Fill {
        self.chart_track
    }

    /// The positive sentiment series.
    pub(crate) fn chart_positive(&self) -> Fill {
        self.chart_positive
    }

    /// The neutral sentiment series.
    pub(crate) fn chart_neutral(&self) -> Fill {
        self.chart_neutral
    }

    /// The negative sentiment series.
    pub(crate) fn chart_negative(&self) -> Fill {
        self.chart_negative
    }

    /// The zero baseline of signed column charts.
    pub(crate) fn chart_baseline(&self) -> Style {
        self.chart_baseline
    }

    /// Chart captions and column labels.
    pub(crate) fn chart_label(&self) -> Style {
        self.chart_label
    }

    // --- markdown ---

    /// The top-level markdown heading (H1) in the entry viewer.
    pub(crate) fn md_heading(&self) -> Style {
        self.md_heading
    }

    /// The second-level markdown heading (H2), defaulting to `md_heading` so
    /// H1 and H2 read alike until a theme splits them.
    pub(crate) fn md_heading2(&self) -> Style {
        self.md_heading2
    }

    /// Faded markdown sub-headings (H3 and deeper), for themes that step down
    /// the hierarchy.
    pub(crate) fn md_subheading(&self) -> Style {
        self.md_subheading
    }

    /// Markdown links.
    pub(crate) fn md_link(&self) -> Style {
        self.md_link
    }

    /// Fenced code blocks.
    pub(crate) fn md_code(&self) -> Style {
        self.md_code
    }

    /// Inline `` `code` `` spans, defaulting to `md_code`.
    pub(crate) fn md_inline_code(&self) -> Style {
        self.md_inline_code
    }

    /// Block quotes.
    pub(crate) fn md_blockquote(&self) -> Style {
        self.md_blockquote
    }

    /// `==highlight==` spans, defaulting to the primary accent (reversed + bold).
    pub(crate) fn md_highlight(&self) -> Style {
        self.md_highlight
    }

    /// Syntax-highlight colors for fenced code blocks.
    pub(crate) fn syntax(&self) -> Syntax {
        self.syntax
    }

    // --- entry metadata ---

    /// How the reader's metadata chips are drawn (`metadata.pills.style`).
    pub(crate) fn pill_style(&self) -> PillStyle {
        self.metadata.pill_style
    }

    /// The style layered onto one metadata pill. `Reversed` inversion is
    /// code-enforced (monochrome contract) and ignores the category colors;
    /// `Bracket` pills are plain text; `Bg` uses the per-category pill styles,
    /// which layer under the value's own ink like hover does.
    pub(crate) fn pill(&self, category: PillCategory) -> Style {
        match self.metadata.pill_style {
            PillStyle::Reversed => Style::default().add_modifier(Modifier::REVERSED),
            PillStyle::Bracket => Style::default(),
            PillStyle::Bg => match category {
                PillCategory::Feelings => self.metadata.pill_feelings,
                PillCategory::People => self.metadata.pill_people,
                PillCategory::Activities => self.metadata.pill_activities,
                PillCategory::Tags => self.metadata.pill_tags,
            },
        }
    }

    /// The glyph leading a chip pill of the given category, so the pill row
    /// echoes the environment strip's glyph-led grammar.
    pub(crate) fn pill_glyph(&self, category: PillCategory) -> char {
        match category {
            PillCategory::Feelings => self.metadata.glyphs.feelings,
            PillCategory::People => self.metadata.glyphs.people,
            PillCategory::Activities => self.metadata.glyphs.activities,
            PillCategory::Tags => self.metadata.glyphs.tags,
        }
    }

    /// The style of the air-quality badge for a European AQI reading, or
    /// `None` below 60 — clean air never renders. Bands: 60–80 poor, 80–100
    /// very poor, 100+ extremely poor (bold in code so the worst band survives
    /// monochrome).
    pub(crate) fn aqi_band(&self, aqi: i64) -> Option<Style> {
        if aqi < 60 {
            None
        } else if aqi < 80 {
            Some(self.metadata.aqi_poor)
        } else if aqi < 100 {
            Some(self.metadata.aqi_very_poor)
        } else {
            Some(
                self.metadata
                    .aqi_extremely_poor
                    .add_modifier(Modifier::BOLD),
            )
        }
    }

    /// The style of the strip's high-pollen badge — like the AQI bands it
    /// only renders when there is something to warn about, so it defaults to
    /// the warning hue.
    pub(crate) fn pollen_high(&self) -> Style {
        self.metadata.pollen_high
    }

    /// The mood gauge's filled-cell style for a valence: negative fills read as
    /// the theme's error hue, positive as its success hue.
    pub(crate) fn mood_fill(&self, positive: bool) -> Style {
        if positive {
            self.metadata.mood_positive
        } else {
            self.metadata.mood_negative
        }
    }

    /// The environment strip's glyph vocabulary (`[metadata.glyphs]`).
    pub(crate) fn env_glyphs(&self) -> EnvGlyphs {
        self.metadata.glyphs
    }

    // --- glyphs ---

    /// The theme's identity glyphs.
    pub(crate) fn glyphs(&self) -> &Glyphs {
        &self.glyphs
    }

    // --- chrome ---

    /// Whether this theme separates surfaces by background or drawn borders.
    pub(crate) fn chrome(&self) -> ChromeStyle {
        self.chrome
    }

    /// How strongly the screen dims behind dialogs, `0.0..=1.0`. Zero means
    /// the DIM-modifier fallback.
    pub(crate) fn scrim_strength(&self) -> f32 {
        self.scrim
    }

    /// Every style the theme carries, for whole-theme assertions in tests.
    #[cfg(test)]
    pub(super) fn all_styles(&self) -> Vec<(&'static str, Style)> {
        vec![
            ("text", self.text),
            ("muted", self.muted),
            ("heading", self.heading),
            ("placeholder", self.placeholder),
            ("primary", self.primary),
            ("border_subtle", self.border_subtle),
            ("border_active", self.border_active),
            ("border_inactive", self.border_inactive),
            ("success", self.success),
            ("warning", self.warning),
            ("error", self.error),
            ("info", self.info),
            ("selection", self.selection),
            ("hover", self.hover),
            ("button", self.button),
            ("key_hint", self.key_hint),
            ("cursor", self.cursor),
            ("cursor_line", self.cursor_line),
            ("scrollbar_thumb", self.scrollbar_thumb),
            ("scrollbar_track", self.scrollbar_track),
            ("chart_positive", self.chart_positive.style),
            ("chart_neutral", self.chart_neutral.style),
            ("chart_negative", self.chart_negative.style),
            ("chart_bar", self.chart_bar.style),
            ("chart_track", self.chart_track.style),
            ("chart_baseline", self.chart_baseline),
            ("chart_label", self.chart_label),
            ("md_heading", self.md_heading),
            ("md_heading2", self.md_heading2),
            ("md_subheading", self.md_subheading),
            ("md_link", self.md_link),
            ("md_code", self.md_code),
            ("md_inline_code", self.md_inline_code),
            ("md_blockquote", self.md_blockquote),
            ("md_highlight", self.md_highlight),
            ("pill_feelings", self.metadata.pill_feelings),
            ("pill_people", self.metadata.pill_people),
            ("pill_activities", self.metadata.pill_activities),
            ("pill_tags", self.metadata.pill_tags),
            ("aqi_poor", self.metadata.aqi_poor),
            ("aqi_very_poor", self.metadata.aqi_very_poor),
            ("aqi_extremely_poor", self.metadata.aqi_extremely_poor),
            ("mood_negative", self.metadata.mood_negative),
            ("mood_positive", self.metadata.mood_positive),
        ]
    }
}
