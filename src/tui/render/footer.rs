//! The footer hint bar: the per-view hint sets, the wrap/justify grid the
//! hints render through, and the hit-testing that maps clicks back to hints.

use ratatui::{
    style::Style,
    text::{Line, Span, Text},
};
use unicode_width::UnicodeWidthStr;

use crate::tui::app::{AppModel, Focus, Mode};
use crate::tui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HintId {
    /// Select all text in whichever single-line field owns the caret.
    InputSelectAll,
    NewJournal,
    ToggleArchiveJournal,
    NewEntry,
    BeginSearch,
    Quit,
    EditSelected,
    BeginDelete,
    ToggleStarred,
    ExitSearch,
    CancelOverlay,
    /// Step the highlighted setting (← / →) — keyboard-only, so no click action.
    SettingsAdjust,
    MetadataToggle,
    MetadataSwitchFocus,
    MetadataAddFromInput,
    MetadataSave,
    FeelingsToggle,
    FeelingsExpand,
    FeelingsCollapse,
    FeelingsSwitchFocus,
    FeelingsSave,
    MoodDecrease,
    MoodIncrease,
    MoodSave,
    MoodClear,
    LocationSwitchFocus,
    LocationResolve,
    LocationGrabDevice,
    LocationSelectRow,
    LocationSave,
    LocationClear,
    OpenImageViewer,
    OpenReaderLinks,
    /// The alphabet chip shown while link-hint mode is up. Inert: the keys it
    /// names are the labels in the body, not something to click.
    ReaderHintKeys,
    CancelReaderHints,
    // The per-type metadata editors, each a direct footer chip (and mouse button)
    // for the selected entry.
    EditTags,
    EditPeople,
    EditActivities,
    EditFeelings,
    EditMood,
    EditLocation,
    ThemePickerApply,
    ThemePickerRevert,
    ThemePickerChrome,
    ThemePickerMode,
    ThemePickerScope,
    Help,
    InsightsScope,
    InsightsTimeframe,
    ExpandInsights,
    CloseInsights,
    EditorSave,
    EditorDiscard,
    EditorFullscreen,
    EditorMetadata,
    EditorHelp,
    OpenFilter,
    FilterNextTab,
    FilterLaunch,
    HelpSwitchTab,
    /// Step the suggestion highlight. Inert: it names two keys with opposite
    /// effects, and a click cannot say which was meant.
    MoveSuggestion,
    CommitSuggestion,
    DismissSuggestions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hint {
    pub(crate) label: &'static str,
    pub(crate) key_hint: &'static str,
    pub(crate) id: HintId,
}

impl Hint {
    pub(crate) const fn new(label: &'static str, key_hint: &'static str, id: HintId) -> Self {
        Self {
            label,
            key_hint,
            id,
        }
    }

    fn text(self) -> String {
        format!("{} {}", key_chip_text(self.key_hint), self.label)
    }
}

/// Minimum blank columns kept around and between hints when a row is justified.
const HINT_MIN_GAP: usize = 2;

/// Saturating `usize`→`u16`, for column math that can never realistically overflow
/// a terminal but must stay in bounds.
pub(super) fn clamp_u16(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

/// Space-around gap distribution: with `content_total` columns of content and
/// `gap_count` gaps (each already reserving [`HINT_MIN_GAP`]), spread the leftover
/// width evenly. Returns `(base, remainder)` — every gap grows by `base`, and the
/// first `remainder` gaps grow by one more.
fn spread_gaps(area: usize, content_total: usize, gap_count: usize) -> (usize, usize) {
    let extra = area.saturating_sub(content_total + gap_count * HINT_MIN_GAP);
    (extra / gap_count, extra % gap_count)
}

/// The key portion of a hint as plain text: a space on each side so the reversed
/// chip reads as a padded button. Kept in one place so the styled rendering and
/// the width/hit-test math stay in lockstep.
pub(super) fn key_chip_text(key: &str) -> String {
    format!(" {key} ")
}

/// The style for a hint's key chip. The token's default is the classic
/// inverted chip, so themes that never touch `key_hint` keep the pre-theme
/// footer on both chromes.
pub(super) fn key_chip_style(theme: &Theme) -> Style {
    theme.key_hint()
}

#[derive(Debug, Clone)]
struct RenderedHintLine {
    /// The row's full visual text of justified hints, identical to what is
    /// drawn — so `find`-based column lookups line up with hit-testing.
    text: String,
    /// `(start column, hint)` for each hint, columns absolute within the row.
    placements: Vec<(u16, Hint)>,
}

fn hint_width(hint: &Hint) -> usize {
    UnicodeWidthStr::width(hint.text().as_str())
}

/// The id of the hint whose justified span contains `col` (relative to `origin_x`).
fn placement_at(placements: &[(u16, Hint)], origin_x: u16, col: u16) -> Option<HintId> {
    let rel = col.checked_sub(origin_x)?;
    placements.iter().find_map(|(start, hint)| {
        let width = hint_width(hint) as u16;
        (rel >= *start && rel < start.saturating_add(width)).then_some(hint.id)
    })
}

/// Render a laid-out hint row as styled spans: the gaps stay plain and each key
/// chip is drawn reversed + bold. Columns match [`RenderedHintLine::text`]
/// exactly, so the visual output lines up with hit-testing. The hovered hint's
/// label lifts out of the muted row as the click affordance.
fn styled_hint_line(
    theme: &Theme,
    rendered: &RenderedHintLine,
    hovered: Option<HintId>,
) -> Line<'static> {
    if rendered.placements.is_empty() {
        return Line::from(rendered.text.clone());
    }
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut col = 0u16;
    for (start, hint) in &rendered.placements {
        if *start > col {
            spans.push(Span::raw(" ".repeat((*start - col) as usize)));
            col = *start;
        }
        let chip = key_chip_text(hint.key_hint);
        col += clamp_u16(UnicodeWidthStr::width(chip.as_str()));
        spans.push(Span::styled(chip, key_chip_style(theme)));
        let label = format!(" {}", hint.label);
        col += clamp_u16(UnicodeWidthStr::width(label.as_str()));
        spans.push(if hovered == Some(hint.id) {
            Span::styled(label, theme.text())
        } else {
            Span::styled(label, theme.muted())
        });
    }
    Line::from(spans)
}

pub(crate) fn hint_lines(
    theme: &Theme,
    hints: &[Hint],
    width: u16,
    hovered: Option<HintId>,
) -> Vec<Line<'static>> {
    rendered_hint_lines(hints, width)
        .iter()
        .map(|line| styled_hint_line(theme, line, hovered))
        .collect()
}

pub(crate) fn hint_height(hints: &[Hint], width: u16) -> u16 {
    clamp_u16(rendered_hint_lines(hints, width).len().max(1))
}

/// The hint grid's rows joined by newlines, for tests to locate hints by text.
#[cfg(test)]
pub(crate) fn hint_grid_text(hints: &[Hint], width: u16) -> String {
    rendered_hint_lines(hints, width)
        .iter()
        .map(|row| row.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn hint_id_at_wrapped(
    hints: &[Hint],
    origin_x: u16,
    origin_y: u16,
    width: u16,
    col: u16,
    row: u16,
) -> Option<HintId> {
    let relative_row = row.checked_sub(origin_y)? as usize;
    let lines = rendered_hint_lines(hints, width);
    let line = lines.get(relative_row)?;
    placement_at(&line.placements, origin_x, col)
}

/// Lay the hints out as a column grid: pick a column count that fits, then align
/// every row to the same column x-positions (each hint left-aligned in its column)
/// so wrapped rows line up vertically. Leftover width is spread across the gaps so
/// the grid still fills the row.
fn rendered_hint_lines(hints: &[Hint], width: u16) -> Vec<RenderedHintLine> {
    if hints.is_empty() {
        return Vec::new();
    }
    let mut columns = columns_that_fit(hints, width);
    let (col_x, rows) = loop {
        let rows: Vec<&[Hint]> = hints.chunks(columns).collect();
        let mut col_widths = vec![0usize; columns];
        for row in &rows {
            for (index, hint) in row.iter().enumerate() {
                col_widths[index] = col_widths[index].max(hint_width(hint));
            }
        }
        let total: usize = col_widths.iter().sum();
        let gap_count = columns + 1;
        if columns == 1 || total + gap_count * HINT_MIN_GAP <= width as usize {
            let (base, remainder) = spread_gaps(width as usize, total, gap_count);
            let mut col_x = Vec::with_capacity(columns);
            let mut x = 0usize;
            for (index, col_width) in col_widths.iter().enumerate() {
                x += HINT_MIN_GAP + base + usize::from(index < remainder);
                col_x.push(clamp_u16(x));
                x += col_width;
            }
            break (col_x, rows);
        }
        columns -= 1;
    };
    rows.iter().map(|row| build_grid_row(&col_x, row)).collect()
}

/// How many equal grid columns the hints can use: greedily fit as many as possible
/// on the first row (with minimum gaps), at least one.
fn columns_that_fit(hints: &[Hint], width: u16) -> usize {
    let width = width as usize;
    let mut used = HINT_MIN_GAP; // trailing edge gap
    let mut columns = 0;
    for hint in hints {
        let need = HINT_MIN_GAP + hint_width(hint);
        if columns > 0 && used + need > width {
            break;
        }
        used += need;
        columns += 1;
    }
    columns.max(1)
}

/// Place a row's hints at the shared column x-positions, left-aligned in each column.
fn build_grid_row(col_x: &[u16], hints: &[Hint]) -> RenderedHintLine {
    let mut text = String::new();
    let mut col = 0u16;
    let mut placements = Vec::with_capacity(hints.len());
    for (index, hint) in hints.iter().enumerate() {
        let start = col_x[index];
        while col < start {
            text.push(' ');
            col += 1;
        }
        placements.push((start, *hint));
        text.push_str(&hint.text());
        col += hint_width(hint) as u16;
    }
    RenderedHintLine { text, placements }
}

/// The footer's justified rows joined by newlines, for tests to inspect.
#[cfg(test)]
pub(crate) fn footer_text(app: &AppModel, width: u16) -> String {
    let Some(line) = active_footer_line(app) else {
        return String::new();
    };
    line.rendered_lines(width)
        .iter()
        .map(|row| row.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Footer hints shown while the internal editor is open, in both its in-pane and
/// full-screen forms.
fn editor_footer_line() -> HintLine {
    HintLine {
        hints: vec![
            Hint::new("save", "ctrl+s", HintId::EditorSave),
            Hint::new("discard", "esc", HintId::EditorDiscard),
            Hint::new("fullscreen", "ctrl+o", HintId::EditorFullscreen),
            Hint::new("metadata", "ctrl+g", HintId::EditorMetadata),
            Hint::new("shortcuts", "ctrl+t", HintId::EditorHelp),
        ],
    }
}

/// The footer in force, or `None` when the hints are hidden. Every reader of
/// the footer goes through here, so what is drawn, what is clickable and the
/// height reserved for it cannot describe different lines.
fn active_footer_line(app: &AppModel) -> Option<HintLine> {
    if !app.state.ui.show_hints {
        return None;
    }
    if app.editor.is_some() {
        return Some(editor_footer_line());
    }
    // Link-hint mode claims every letter for its labels, so the usual chips
    // would advertise keys that do nothing while it is up. Ahead of the mode
    // split because the hints span browse and search alike.
    if app.reader_hints.is_active() {
        return Some(reader_hint_footer_line());
    }
    Some(match app.nav.mode {
        Mode::Search => search_footer_line(app),
        Mode::Browse => browse_footer_line(app),
    })
}

pub(crate) fn footer_lines(theme: &Theme, app: &AppModel, width: u16) -> Text<'static> {
    let Some(line) = active_footer_line(app) else {
        return Text::default();
    };
    Text::from(line.lines(theme, width, app.hovered_footer_hint()))
}

pub(crate) fn footer_height(app: &AppModel, width: u16) -> u16 {
    active_footer_line(app).map_or(0, |line| line.height(width))
}

#[cfg(test)]
pub(crate) fn footer_hint_id_at(
    app: &AppModel,
    origin_x: u16,
    width: u16,
    col: u16,
) -> Option<HintId> {
    active_footer_line(app)?
        .rendered_lines(width)
        .first()
        .and_then(|row| placement_at(&row.placements, origin_x, col))
}

pub(crate) fn footer_hint_id_at_point(
    app: &AppModel,
    origin_x: u16,
    origin_y: u16,
    width: u16,
    col: u16,
    row: u16,
) -> Option<HintId> {
    active_footer_line(app)?.hint_id_at_point(origin_x, origin_y, width, col, row)
}

pub(crate) fn footer_hint_regions(app: &AppModel, width: u16) -> Vec<(u16, u16, u16, HintId)> {
    let Some(line) = active_footer_line(app) else {
        return Vec::new();
    };
    line.rendered_lines(width)
        .into_iter()
        .enumerate()
        .flat_map(|(row, line)| {
            line.placements.into_iter().map(move |(start, hint)| {
                (row as u16, start, clamp_u16(hint_width(&hint)), hint.id)
            })
        })
        .collect()
}

/// The expanded footer's justified rows joined by newlines, for tests.
#[derive(Debug, Clone)]
struct HintLine {
    hints: Vec<Hint>,
}

impl HintLine {
    fn rendered_lines(&self, width: u16) -> Vec<RenderedHintLine> {
        rendered_hint_lines(&self.hints, width)
    }

    fn lines(&self, theme: &Theme, width: u16, hovered: Option<HintId>) -> Vec<Line<'static>> {
        self.rendered_lines(width)
            .iter()
            .map(|line| styled_hint_line(theme, line, hovered))
            .collect()
    }

    fn height(&self, width: u16) -> u16 {
        clamp_u16(self.rendered_lines(width).len().max(1))
    }

    fn hint_id_at_point(
        &self,
        origin_x: u16,
        origin_y: u16,
        width: u16,
        col: u16,
        row: u16,
    ) -> Option<HintId> {
        let relative_row = row.checked_sub(origin_y)? as usize;
        let lines = self.rendered_lines(width);
        let line = lines.get(relative_row)?;
        placement_at(&line.placements, origin_x, col)
    }
}

fn reader_hint_footer_line() -> HintLine {
    HintLine {
        hints: vec![
            Hint::new("open", "a–z", HintId::ReaderHintKeys),
            Hint::new("cancel", "esc", HintId::CancelReaderHints),
        ],
    }
}

fn search_footer_line(app: &AppModel) -> HintLine {
    // With the suggestion list up, the footer teaches the one thing that is not
    // guessable: nothing is highlighted yet, so `enter` still opens the result
    // and `tab` is what takes the value.
    if app.suggestions_visible() {
        let mut hints = vec![Hint::new(
            "choose",
            if app.search.suggestions.highlighted().is_some() {
                "↑↓"
            } else {
                "↓"
            },
            HintId::MoveSuggestion,
        )];
        if app.search.suggestions.highlighted().is_some() {
            hints.push(Hint::new("insert", "enter", HintId::CommitSuggestion));
        } else {
            hints.push(Hint::new("insert", "tab", HintId::CommitSuggestion));
        }
        hints.push(Hint::new("close", "esc", HintId::DismissSuggestions));
        return HintLine { hints };
    }
    // The query lives on the entry panel's top-right border (see
    // `draw_entry_list`), so the footer only carries the action hints.
    let hints = match app.nav.focus {
        Focus::Reader if app.has_selected_entry_target() => {
            let mut hints = selected_entry_action_hints();
            hints.extend(image_hint(app));
            hints.push(Hint::new("exit search", "esc", HintId::ExitSearch));
            hints.push(Hint::new("quit", "q", HintId::Quit));
            hints
        }
        Focus::Reader => vec![
            Hint::new("exit search", "esc", HintId::ExitSearch),
            Hint::new("quit", "q", HintId::Quit),
        ],
        _ => vec![Hint::new("exit search", "esc", HintId::ExitSearch)],
    };

    HintLine { hints }
}

fn browse_footer_line(app: &AppModel) -> HintLine {
    let hints = match app.nav.focus {
        Focus::Journals => {
            let mut hints = vec![Hint::new("new journal", "n", HintId::NewJournal)];
            hints.extend(archive_hint(app));
            hints.extend(browse_footer_tail());
            hints
        }
        Focus::Insights => {
            let mut hints = vec![Hint::new("scope", "g", HintId::InsightsScope)];
            if app.nav.insights_tab.uses_timeframe() {
                hints.push(Hint::new("timeframe", "w", HintId::InsightsTimeframe));
            }
            if app.nav.insights_fullscreen {
                hints.push(Hint::new("close", "enter/esc", HintId::CloseInsights));
            } else {
                hints.push(Hint::new("expand", "enter", HintId::ExpandInsights));
            }
            hints.extend(help_quit_tail());
            hints
        }
        Focus::Entries if app.has_selected_entry_target() => {
            // `b` opens the index from the entries column but is inert in the reader,
            // so the shared reader footer below omits it.
            let mut hints = focused_entry_footer(app);
            hints.push(FILTER_HINT);
            hints
        }
        Focus::Entries => {
            let mut hints = vec![Hint::new("new entry", "n", HintId::NewEntry)];
            hints.extend(browse_footer_tail());
            hints
        }
        Focus::Reader if app.has_selected_entry_target() => {
            let mut hints = focused_entry_footer(app);
            hints.extend(open_links_hint(app));
            hints
        }
        Focus::Reader => vec![Hint::new("new entry", "n", HintId::NewEntry)],
    };

    HintLine { hints }
}

/// The `images (i)` hint, shown only when the selected entry has images.
fn image_hint(app: &AppModel) -> Option<Hint> {
    (app.selected_entry_image_count() > 0).then_some(Hint::new(
        "images",
        "i",
        HintId::OpenImageViewer,
    ))
}

/// The `open link (o)` chip: link-hint mode over everything the entry can open.
/// Shown only when there is something to open, like [`image_hint`], and only on
/// the focused reader — hence not in [`focused_entry_footer`], which the entries
/// column shares.
fn open_links_hint(app: &AppModel) -> Option<Hint> {
    app.reader_hints
        .has_openable()
        .then_some(Hint::new("open link", "o", HintId::OpenReaderLinks))
}

/// The cheatsheet pointer (`?`). The full binding set — the journals/settings/hints
/// toggles and the bare metadata keys — lives behind it.
const HELP_HINT: Hint = Hint::new("help", "?", HintId::Help);

/// The quit chip, shared by the footer tails.
const QUIT_HINT: Hint = Hint::new("quit", "q", HintId::Quit);

/// The filter chip (`b`), shown in the columns where `b` opens it — the
/// journals and entries columns.
const FILTER_HINT: Hint = Hint::new("filter", "b", HintId::OpenFilter);

/// Trailing hints for the columns where search has a clear scope — journals (all)
/// and entries (this journal): search, the filter browser, the `?` cheatsheet, and
/// quit.
fn browse_footer_tail() -> [Hint; 4] {
    [
        Hint::new("search", "/", HintId::BeginSearch),
        FILTER_HINT,
        HELP_HINT,
        QUIT_HINT,
    ]
}

/// Trailing hints without search, for the insights column: the `?` cheatsheet
/// and quit.
fn help_quit_tail() -> [Hint; 2] {
    [HELP_HINT, QUIT_HINT]
}

/// The `archive`/`unarchive (a)` hint, shown only when a journal is selected. The
/// label reflects the selected journal's current state.
fn archive_hint(app: &AppModel) -> Option<Hint> {
    app.selected_journal().map(|journal| {
        let label = if journal.archived {
            "unarchive"
        } else {
            "archive"
        };
        Hint::new(label, "a", HintId::ToggleArchiveJournal)
    })
}

/// The action hints for a selected entry: edit, the per-type metadata editors,
/// star, and delete. Each chip is also the only pointer path to its action.
fn selected_entry_action_hints() -> Vec<Hint> {
    vec![
        Hint::new("edit", "e", HintId::EditSelected),
        Hint::new("tags", "t", HintId::EditTags),
        Hint::new("people", "p", HintId::EditPeople),
        Hint::new("activities", "a", HintId::EditActivities),
        Hint::new("feelings", "f", HintId::EditFeelings),
        Hint::new("mood", "m", HintId::EditMood),
        Hint::new("location", "l", HintId::EditLocation),
        Hint::new("star", "s", HintId::ToggleStarred),
        Hint::new("del", "d", HintId::BeginDelete),
    ]
}

/// The footer for a selected entry — new-entry, the entry actions, and the image
/// chip when the entry has images. Shared by the entries list and the reader.
fn focused_entry_footer(app: &AppModel) -> Vec<Hint> {
    let mut hints = vec![Hint::new("new entry", "n", HintId::NewEntry)];
    hints.extend(selected_entry_action_hints());
    hints.extend(image_hint(app));
    hints
}

// ── Chrome style: flat (bg-layered) vs bordered ───────────────────────────────
