use crossterm::event::KeyEvent;

use crate::tui::{
    features::{insights::InsightsTab, location::EditLocationFocus},
    state::{FilterTab, HelpTab, HoverTarget, MetadataKind},
    ui::interaction::{PanelId, TextFieldId},
};

pub(crate) use crate::tui::ui::interaction::ScrollbarMetrics;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextFieldTarget {
    Search,
    NewJournal,
    Metadata,
    Feelings,
    LocationQuery,
    LocationName,
}

impl From<TextFieldId> for TextFieldTarget {
    fn from(value: TextFieldId) -> Self {
        match value {
            TextFieldId::Search => Self::Search,
            TextFieldId::NewJournal => Self::NewJournal,
            TextFieldId::Metadata => Self::Metadata,
            TextFieldId::Feelings => Self::Feelings,
            TextFieldId::LocationQuery => Self::LocationQuery,
            TextFieldId::LocationName => Self::LocationName,
        }
    }
}

impl From<TextFieldTarget> for TextFieldId {
    fn from(value: TextFieldTarget) -> Self {
        match value {
            TextFieldTarget::Search => Self::Search,
            TextFieldTarget::NewJournal => Self::NewJournal,
            TextFieldTarget::Metadata => Self::Metadata,
            TextFieldTarget::Feelings => Self::Feelings,
            TextFieldTarget::LocationQuery => Self::LocationQuery,
            TextFieldTarget::LocationName => Self::LocationName,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DialogListTarget {
    Metadata,
    Feelings,
    Location,
    ThemePicker,
    Filter,
    Settings,
    SearchSuggestions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MetadataSearchTarget {
    Feelings,
    Metadata(MetadataKind),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MouseAction {
    DismissToast(usize),
    TextFieldPress {
        target: TextFieldTarget,
        column: u16,
    },
    TextFieldSelectWord {
        target: TextFieldTarget,
        column: u16,
    },
    TextFieldDrag {
        column: u16,
        /// Cells the pointer is outside the field, signed — negative left of it,
        /// positive right. Non-zero is what scrolls a selection past the edge.
        overshoot: i16,
    },
    TextFieldRelease,
    JournalClick {
        index: Option<usize>,
        compact: bool,
    },
    EntryClick {
        index: Option<usize>,
        open_reader: bool,
        clear_empty: bool,
    },
    InsightsClick(Option<InsightsTab>),
    ReaderClick,
    MetadataSearch {
        kind: MetadataSearchTarget,
        value: String,
    },
    ScrollPanel {
        panel: PanelId,
        delta: i16,
        content_length: usize,
        viewport: u16,
    },
    ScrollbarPress {
        metrics: ScrollbarMetrics,
        row: u16,
    },
    ScrollbarDrag {
        metrics: ScrollbarMetrics,
        row: u16,
    },
    ScrollbarRelease,
    DialogRow {
        target: DialogListTarget,
        index: usize,
    },
    DialogFocusMetadata(EditMetadataFocusTarget),
    DialogFocusLocation(EditLocationFocus),
    DialogScroll {
        target: DialogListTarget,
        delta: i16,
        viewport: u16,
    },
    SetMood(i8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditMetadataFocusTarget {
    List,
    Input,
}

/// Which live reload a failed watcher registration costs the user. Named for
/// the feature they lose, not the directory being watched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WatchTarget {
    Journal,
    Theme,
}

impl WatchTarget {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Journal => "journal",
            Self::Theme => "theme",
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum BackgroundAction {
    LibraryValidated(Box<notema_storage::LibrarySnapshot>),
    LibraryValidationStale,
    LibraryValidationFailed(String),
    WatcherUnavailable {
        target: WatchTarget,
        error: String,
    },
    /// The journal watcher missed changes and cannot say which. Only re-reading
    /// the tree can reconcile it.
    WatcherLostTrack,
    ExternalOpenCompleted(String),
    ExternalOpenFailed(String),
    PollImages,
    PollGeocode,
    PollEnvironment,
    PollLibraryReload,
    PollTimers,
    LibraryPathsChanged(Vec<std::path::PathBuf>),
    ReloadTheme(String),
    CommitSearch,
}

#[derive(Debug, PartialEq)]
pub(crate) enum BrowserAction {
    FocusLeft,
    FocusRight,
    MoveSelection(isize),
    EditSelected,
    ViewSelected,
    OpenReaderLink {
        target: crate::tui::app::ReaderLinkTarget,
        heading_line: Option<usize>,
    },
    BeginDelete,
    ConfirmDelete,
    ToggleStarred,
    NewEntry,
}

#[derive(Debug, PartialEq)]
pub(crate) enum SearchAction {
    Begin,
    Exit,
    /// Step the suggestion highlight, entering the list on the first `Down`.
    MoveSuggestion(isize),
    /// Write the highlighted value into the query — or the first one, which is
    /// what `Tab` means when the list has not been arrowed into.
    CommitSuggestion,
    /// Write a row chosen outright, as a click on it does.
    CommitSuggestionAt(usize),
    DismissSuggestions,
}

#[derive(Debug, PartialEq)]
pub(crate) enum EditorAction {
    Save,
    RequestDiscard,
    Discard,
    ToggleFullscreen,
    OpenMetadataMenu,
    OpenHelp,
    ClosePrompt,
    ScrollHelp(i16),
    Input(KeyEvent),
    /// Insert a block of text at the caret in one edit (a bracketed paste),
    /// instead of replaying it as individual key events.
    InsertText(String),
    SelectAll,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Scroll(i16),
    StartSelection {
        col: u16,
        row: u16,
    },
    SelectWord {
        col: u16,
        row: u16,
    },
    DragSelection {
        col: u16,
        row: u16,
    },
    EndSelection,
}

#[derive(Debug, PartialEq)]
pub(crate) enum MetadataAction {
    OpenMenu,
    BeginEdit(MetadataKind),
    BeginFeelings,
    BeginMood,
    MoveSelection(isize),
    Toggle,
    SwitchFocus,
    AddFromInput,
    Save,
    FeelingsToggle,
    FeelingsExpand,
    FeelingsCollapse,
    FeelingsSwitchFocus,
    FeelingsSave,
    AdjustMood(i8),
    MoodSave,
    MoodClear,
}

#[derive(Debug, PartialEq)]
pub(crate) enum LocationAction {
    BeginEdit,
    SwitchFocus,
    Resolve,
    GrabDevice,
    SelectRow,
    Save,
    Clear,
}

#[derive(Debug, PartialEq)]
pub(crate) enum SettingsAction {
    NewJournal,
    ToggleArchiveJournal,
    JournalInputSubmit,
    /// Open the settings dialog.
    OpenSettings,
    /// Enter/Space on the highlighted row: toggle a bool or open the theme picker.
    Activate,
    /// ← / → on a number row: adjust by one step in the given direction (-1/+1).
    Adjust(i16),
    /// Click setting-row `index`: select it, or activate it if already selected.
    Click(usize),
    ThemePickerSelect(usize),
    /// Click theme-row `index`: preview it, or confirm it if already selected.
    ThemePickerClick(usize),
    ThemePickerConfirm,
    ThemePickerCancel,
    ThemePickerCycleChrome,
    ThemePickerCycleMode,
    ThemePickerToggleScope,
}

#[derive(Debug, PartialEq)]
pub(crate) enum ImageAction {
    OpenViewer(usize),
    StepViewer(isize),
}

#[derive(Debug, PartialEq)]
pub(crate) enum ReaderAction {
    ScrollLines(i16),
    ScrollPages(i16),
    ScrollToStart,
    ScrollToEnd,
    SetFullscreen(bool),
}

#[derive(Debug, PartialEq)]
pub(crate) enum InsightsAction {
    ScrollLines(i16),
    ScrollPages(i16),
    ScrollToStart,
    ScrollToEnd,
    SetFullscreen(bool),
    ToggleScope,
    CycleTimeframe,
}

#[derive(Debug, PartialEq)]
pub(crate) enum FilterAction {
    Open,
    NextTab,
    PrevTab,
    SelectTab(FilterTab),
    MoveSelection(isize),
    /// Launch the search for the highlighted row. A mouse click on a row selects
    /// it first (via [`MouseAction::DialogRow`]) and then fires this.
    Launch,
}

#[derive(Debug, PartialEq)]
pub(crate) enum OverlayAction {
    ConfirmSelect(bool),
    Cancel,
    OpenHelp,
    HelpScroll(i16),
    HelpNextTab,
    HelpPrevTab,
    HelpSelectTab(HelpTab),
    InputKey(KeyEvent),
    InputPaste(String),
    InputSelectAll,
    ToggleHints,
    ToggleJournals,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Action {
    Mouse(MouseAction),
    SetHover(HoverTarget),
    ViewRendered {
        reader_scroll: Option<u16>,
        insights_scroll: Option<u16>,
        journal_offset: Option<usize>,
        entry_offset: Option<usize>,
        /// The link-hint labels this frame painted. Always sent, so a frame that
        /// painted none is what ends the mode.
        reader_hints: Vec<crate::tui::app::ReaderHint>,
        /// How many openable targets the drawn entry holds, gating the `o` key.
        reader_openable: usize,
    },
    SyncImages(ratatui::layout::Size),
    // Global
    Quit,
    /// Reload the whole library. `rebuild` ignores the entry cache's stamps and
    /// re-reads every entry from source — the recovery path when a cached entry
    /// has gone stale.
    ReloadLibrary {
        rebuild: bool,
    },
    Background(BackgroundAction),
    Browser(BrowserAction),
    Search(SearchAction),
    Editor(EditorAction),
    Metadata(MetadataAction),
    Location(LocationAction),
    Settings(SettingsAction),
    Images(ImageAction),
    Overlay(OverlayAction),
    Reader(ReaderAction),
    ReaderHint(ReaderHintAction),
    Insights(InsightsAction),
    Filter(FilterAction),
}

/// Driving link-hint mode: the reader's keyboard route to everything the mouse
/// can already click.
#[derive(Debug, PartialEq)]
pub(crate) enum ReaderHintAction {
    Begin,
    Cancel,
    /// Type one label character; the handler decides prefix, match, or miss.
    Push(char),
    /// Undo one typed character, leaving the mode when none is left.
    Pop,
}
