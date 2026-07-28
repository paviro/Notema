//! The filter browser: a tabbed overlay that lists every distinct
//! tag/person/activity/feeling/place value in scope with its post
//! count, and launches a filtered search on the chosen row.
//!
//! Scope is captured when the dialog opens — all journals from the journals
//! column, the selected journal from the entries column — and drives both the
//! listed counts and the launched search. The number shown on a row always
//! equals the number of results the search returns: a token row means one value,
//! so its count is a tally of that value and the search it launches asks for
//! exactly it, while a place row keeps containment and is counted by address,
//! which the `location:` predicate is a pure function of.

use std::rc::Rc;

use std::borrow::Cow;

use crate::tui::app::{AppModel, Focus, SearchScope};
use crate::tui::features::facets::{FilterRow, FilterRows};
use crate::tui::features::search::{Prefix, escape_filter_value, quote_filter_value};
use crate::tui::state::{ListNav, Overlay, SelectableList};

/// One facet of the filter dialog. Each tab lists that facet's distinct
/// values with post counts and, when a value is chosen, launches the matching
/// search (its [`search_prefix`](Self::search_prefix) + the row's value).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum FilterTab {
    #[default]
    Tags,
    People,
    Activities,
    Feelings,
    Locations,
}

impl FilterTab {
    pub(crate) const ALL: [FilterTab; 5] = [
        Self::Tags,
        Self::People,
        Self::Activities,
        Self::Feelings,
        Self::Locations,
    ];

    /// Number of tabs.
    pub(crate) const COUNT: usize = Self::ALL.len();

    /// This tab's slot in a `[_; COUNT]` keyed by tab — the facet arrays are
    /// indexed by it, so it is spelt out here rather than found by scanning
    /// `ALL` through a rendering trait whose miss would silently answer `Tags`.
    pub(crate) fn index(self) -> usize {
        match self {
            Self::Tags => 0,
            Self::People => 1,
            Self::Activities => 2,
            Self::Feelings => 3,
            Self::Locations => 4,
        }
    }

    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Tags => "Tags",
            Self::People => "People",
            Self::Activities => "Activities",
            Self::Feelings => "Feelings",
            Self::Locations => "Locations",
        }
    }

    /// A shorter label used when the full titles won't fit the tab strip.
    pub(crate) fn short_title(self) -> &'static str {
        match self {
            Self::Tags => "Tags",
            Self::People => "Ppl",
            Self::Activities => "Acts",
            Self::Feelings => "Feel",
            Self::Locations => "Locs",
        }
    }

    /// A single-letter label — the narrowest tab strip rung. Each is unique, so
    /// every tab stays visible and clickable on the tightest layout.
    pub(crate) fn initial(self) -> &'static str {
        match self {
            Self::Tags => "T",
            Self::People => "P",
            Self::Activities => "A",
            Self::Feelings => "F",
            Self::Locations => "L",
        }
    }

    /// The search prefix a chosen row launches (`tags:`, `location:`, …).
    /// The search prefix listing this tab's values, colon included.
    pub(crate) fn search_prefix(self) -> &'static str {
        match self {
            Self::Tags => Prefix::Tags,
            Self::People => Prefix::People,
            Self::Activities => Prefix::Activities,
            Self::Feelings => Prefix::Feelings,
            Self::Locations => Prefix::Location,
        }
        .token()
    }

    /// The tab a search prefix lists the values of, or `None` for the prefixes
    /// that have no vocabulary to list — a mood, a star or a date is written, not
    /// chosen.
    pub(crate) fn from_prefix(prefix: Prefix) -> Option<Self> {
        match prefix {
            Prefix::Tags => Some(Self::Tags),
            Prefix::People => Some(Self::People),
            Prefix::Activities => Some(Self::Activities),
            Prefix::Feelings => Some(Self::Feelings),
            Prefix::Location => Some(Self::Locations),
            Prefix::Star | Prefix::Mood | Prefix::Date(_) => None,
        }
    }

    /// `value` written the way this tab writes it into a query. Only the token
    /// facets quote, since a quoted value is an exact one and `location:` matches
    /// on words either way — a location quotes only to keep a structural
    /// character literal.
    ///
    /// The one place a value is written, so the browser's rows and the search
    /// box's suggestions cannot commit the same value two ways.
    pub(crate) fn launch_value(self, value: &str) -> Cow<'_, str> {
        match self {
            Self::Locations => escape_filter_value(value),
            _ => Cow::Owned(quote_filter_value(value)),
        }
    }

    /// The whole query a chosen row launches: this tab's prefix, then
    /// [`launch_value`](Self::launch_value).
    pub(crate) fn launch_query(self, value: &str) -> String {
        format!("{}{}", self.search_prefix(), self.launch_value(value))
    }
}

/// State for the filter overlay: the captured scope, the active tab, each
/// tab's rows (built once at open), and the selection/scroll of the active tab.
pub(crate) struct FilterState {
    /// Captured at open — drives the listed counts *and* the launched search.
    pub(crate) scope: SearchScope,
    pub(crate) tab: FilterTab,
    /// Shared with the memo the dialog opened from, so reopening it neither
    /// rebuilds nor copies the rows.
    pub(crate) rows: Rc<FilterRows>,
    pub(crate) list: SelectableList,
}

impl FilterState {
    fn new(scope: SearchScope, rows: Rc<FilterRows>) -> Self {
        // Open on the first tab that has anything to show, so the dialog never
        // greets the user with an empty list when other tabs are populated.
        let tab = FilterTab::ALL
            .into_iter()
            .find(|tab| !rows[tab.index()].is_empty())
            .unwrap_or_default();
        let mut state = Self {
            scope,
            tab,
            rows,
            list: SelectableList::default(),
        };
        state.set_tab(tab);
        state
    }

    pub(crate) fn current_rows(&self) -> &[FilterRow] {
        &self.rows[self.tab.index()]
    }

    pub(crate) fn selected_row(&self) -> Option<&FilterRow> {
        self.selected_index()
            .and_then(|index| self.current_rows().get(index))
    }

    /// Switch to `tab`, resetting the selection to its first row.
    pub(crate) fn set_tab(&mut self, tab: FilterTab) {
        self.tab = tab;
        self.list = SelectableList::default();
        self.normalize_list_state();
        self.select_index(0);
    }
}

impl ListNav for FilterState {
    fn list(&self) -> &SelectableList {
        &self.list
    }

    fn list_mut(&mut self) -> &mut SelectableList {
        &mut self.list
    }

    fn item_count(&self) -> usize {
        self.current_rows().len()
    }
}

impl AppModel {
    pub(crate) fn begin_filter(&mut self) {
        let scope = if self.nav.focus == Focus::Journals {
            SearchScope::AllJournals
        } else {
            self.current_journal_scope()
        };
        let rows = self.cached_filter_rows(&scope);
        self.overlay = Overlay::Filter(Box::new(FilterState::new(scope, rows)));
    }

    pub(crate) fn filter_state(&self) -> Option<&FilterState> {
        match &self.overlay {
            Overlay::Filter(state) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn filter_state_mut(&mut self) -> Option<&mut FilterState> {
        match &mut self.overlay {
            Overlay::Filter(state) => Some(state),
            _ => None,
        }
    }

    /// Close the filter browser and run the search for the highlighted row under
    /// the captured scope.
    pub(crate) fn launch_filter_search(&mut self) {
        let Some(state) = self.filter_state() else {
            return;
        };
        let Some(row) = state.selected_row() else {
            return;
        };
        let scope = state.scope.clone();
        let query = state.tab.launch_query(&row.search_value);
        self.close_overlay();
        self.enter_search(scope, query);
    }
}
