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

use crate::tui::app::{AppModel, Focus, SearchScope};
use crate::tui::features::facets::{FilterRow, FilterRows};
use crate::tui::state::{FilterTab, ListNav, Overlay, SelectableList};

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
