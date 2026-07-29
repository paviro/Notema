//! The search box's suggestion list: which values it offers for the filter value
//! the caret is in, and what committing one writes.
//!
//! The candidates are the filter browser's rows for that facet, so a suggestion
//! and a browser row are the same value ranked the same way, and neither pays for
//! a walk the other already did.
//!
//! Nothing is highlighted until the list is arrowed into; see [`SuggestionState`].

use crate::tui::features::filter::FilterTab;
use std::rc::Rc;

use crate::tui::{
    app::AppModel,
    features::facets::{FilterRow, FilterRows},
    state::{ListNav, SelectableList},
};

use super::{offsets, parse::unquote};

/// The values on offer for the filter value the caret is in, and which one is
/// highlighted.
///
/// The rows are the scope's facet rows, held whole and narrowed to a list of
/// indexes rather than copied: a keystroke re-filters a vocabulary that runs to
/// thousands of values on a large library, and cloning each match's two strings
/// is a cost paid per key for nothing.
///
/// Nothing is highlighted until the list is arrowed into: a recompute follows a
/// keystroke, and a keystroke means the typed fragment is still what was meant,
/// so `Enter` has to keep opening the selected result until a row is chosen
/// deliberately.
#[derive(Default)]
pub(crate) struct SuggestionState {
    /// Every facet's rows for the scope, shared with the memo that built them.
    source: Option<Rc<FilterRows>>,
    /// Which facet `matches` indexes into.
    tab: FilterTab,
    /// Positions in `source[tab]` matching the fragment, in the browser's order.
    matches: Vec<usize>,
    pub(crate) list: SelectableList,
    /// Where the value the list was dismissed for starts, if the caret is still
    /// in it. An offset rather than a flag so dismissing survives typing more of
    /// the same value — a flag cleared on the next keystroke would undo itself —
    /// while moving to another filter brings the list back.
    pub(crate) dismissed: Option<usize>,
}

impl SuggestionState {
    /// How many values are on offer.
    pub(crate) fn len(&self) -> usize {
        self.matches.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }

    /// The row at `index` in the offered order.
    pub(crate) fn row(&self, index: usize) -> Option<&FilterRow> {
        let facet = &self.source.as_ref()?[self.tab.index()];
        facet.get(*self.matches.get(index)?)
    }

    /// The offered rows in order, for drawing.
    pub(crate) fn rows(&self) -> impl ExactSizeIterator<Item = &FilterRow> {
        (0..self.len()).map(|index| self.row(index).expect("index is in range"))
    }

    /// Replace what is on offer.
    pub(crate) fn offer(&mut self, source: Rc<FilterRows>, tab: FilterTab, matches: Vec<usize>) {
        self.source = Some(source);
        self.tab = tab;
        self.matches = matches;
    }

    /// Whether the list is on screen: it has rows and was not dismissed for the
    /// fragment being typed.
    pub(crate) fn is_open(&self) -> bool {
        !self.is_empty() && self.dismissed.is_none()
    }

    /// The highlighted row, or `None` while the list is only advisory.
    pub(crate) fn highlighted(&self) -> Option<&FilterRow> {
        self.highlighted_index().and_then(|index| self.row(index))
    }

    /// The row a deliberate choice landed on. A dismissed list has none, whatever
    /// was armed before it went down — it is no longer on screen to be meant.
    pub(crate) fn highlighted_index(&self) -> Option<usize> {
        self.is_open().then(|| self.selected_index()).flatten()
    }

    pub(crate) fn clear(&mut self) {
        self.source = None;
        self.matches.clear();
        self.list = SelectableList::default();
        self.dismissed = None;
    }

    /// Step the highlight by `delta`, entering the list at its first row and
    /// releasing off the top. Releasing is what hands `Up` back to the entry
    /// list once the list has been stepped out of, so neither direction is a
    /// one-way door.
    pub(crate) fn move_highlight(&mut self, delta: isize) {
        if self.is_empty() {
            return;
        }
        match self.selected_index() {
            None if delta > 0 => self.select_index(0),
            None => {}
            Some(index) => match index as isize + delta {
                next if next < 0 => self.list.select_none(),
                next => self.select_index((next as usize).min(self.len() - 1)),
            },
        }
    }

    /// Put the highlight down, leaving the list open but only advisory again.
    /// Scrolling by hand does this: the highlight is how the list was arrowed
    /// into, and a row scrolled out of sight must not stay armed on `Enter`.
    pub(crate) fn release_highlight(&mut self) {
        self.list.select_none();
    }
}

impl ListNav for SuggestionState {
    fn list(&self) -> &SelectableList {
        &self.list
    }

    fn list_mut(&mut self) -> &mut SelectableList {
        &mut self.list
    }

    fn item_count(&self) -> usize {
        self.len()
    }
}

impl AppModel {
    /// Whether the suggestion list is taking keys — it stays open underneath an
    /// overlay, which is why [`Self::suggestions_visible`] and not this one
    /// gates anything that draws or hit-tests.
    pub(crate) fn suggestions_open(&self) -> bool {
        self.is_search_input_active() && self.search.suggestions.is_open()
    }

    /// Whether the suggestion list is on screen: open, with nothing drawn over
    /// it. The draw and the click map share this so an overlay can't leave
    /// clickable rows behind for a popup nobody can see.
    pub(crate) fn suggestions_visible(&self) -> bool {
        self.suggestions_open() && !self.has_overlay() && self.editor.is_none()
    }

    /// Rebuild the suggestions for wherever the caret now is. Cheap after the
    /// first call per library change: the candidates come from a memo, so this is
    /// a filter over a vec rather than a walk of the corpus.
    pub(crate) fn refresh_search_suggestions(&mut self) {
        let Some((tab, fragment, start)) = self.suggestion_target() else {
            self.search.suggestions.clear();
            return;
        };
        let dismissed = self.search.suggestions.dismissed.filter(|at| *at == start);
        let rows = self.cached_filter_rows(&self.search.scope);
        let matches = matching_rows(&rows[tab.index()], tab, &fragment);
        self.search.suggestions.offer(rows, tab, matches);
        self.search.suggestions.list = SelectableList::default();
        self.search.suggestions.dismissed = dismissed;
    }

    /// The facet, typed fragment and value start the caret is on, or `None` when
    /// there is nothing to complete — full text, a prefix with no vocabulary, or
    /// a value already closed into a quoted pair.
    fn suggestion_target(&self) -> Option<(FilterTab, String, usize)> {
        if !self.is_search_input_active() {
            return None;
        }
        let query = self.search.query.as_str();
        let caret = offsets::caret_context(query, self.search.query.cursor_byte())?;
        if caret.quoted {
            return None;
        }
        let tab = FilterTab::from_prefix(caret.prefix)?;
        Some((
            tab,
            query[caret.value.clone()].to_string(),
            caret.value.start,
        ))
    }

    /// Step the highlight, entering the list on the first `Down`.
    pub(crate) fn move_suggestion_highlight(&mut self, delta: isize) {
        self.search.suggestions.move_highlight(delta);
    }

    /// Take the list down for the value being typed. It comes back on the next
    /// filter, or on `Tab`.
    pub(crate) fn dismiss_suggestions(&mut self) {
        self.search.suggestions.dismissed = self.suggestion_target().map(|(_, _, start)| start);
    }

    /// Write `index`'s value into the query in place of the fragment being typed,
    /// leaving the caret directly after it.
    ///
    /// No trailing space: a space does not separate filters, `;` does, so
    /// `tags:"apple" foo` would be one filter whose value is `"apple" foo`. The
    /// user types the `+`, `|` or `;`, and each of those opens the list again.
    pub(crate) fn commit_suggestion(&mut self, index: usize) {
        let query = self.search.query.as_str();
        let Some(caret) = offsets::caret_context(query, self.search.query.cursor_byte()) else {
            return;
        };
        let Some(tab) = FilterTab::from_prefix(caret.prefix) else {
            return;
        };
        let Some(row) = self.search.suggestions.row(index) else {
            return;
        };

        // An empty value is a caret sitting in whitespace, and writing beside it
        // would leave the space behind: `tags: ` completes to `tags:"apple"`.
        let replaced = if caret.value.is_empty() {
            caret.span
        } else {
            caret.value
        };
        let written = tab.launch_value(&row.search_value);
        let mut updated = String::with_capacity(query.len() + written.len());
        updated.push_str(&query[..replaced.start]);
        updated.push_str(&written);
        let caret_byte = updated.len();
        updated.push_str(&query[replaced.end..]);

        self.search.query.set_text(&updated);
        self.search.query.set_cursor_byte(caret_byte);
        self.mark_search_dirty();
        self.refresh_search_suggestions();
        // A committed value is finished, so the list goes down whether or not the
        // quotes closed it — `location:` commits bare and stays completable.
        self.dismiss_suggestions();
    }

    /// Commit the highlighted row, or the first one when the list is only
    /// advisory — what `Tab` means.
    pub(crate) fn commit_first_suggestion(&mut self) {
        let index = self.search.suggestions.highlighted_index().unwrap_or(0);
        self.commit_suggestion(index);
    }
}

/// The rows offering `fragment`, in the browser's count-first order.
///
/// Matching mirrors the predicate the prefix runs, so a row is offered exactly
/// when typing the fragment would already be narrowing toward it: a substring for
/// the token facets, every word for a place.
fn matching_rows(rows: &[FilterRow], tab: FilterTab, fragment: &str) -> Vec<usize> {
    // Read through an unclosed quote the way the parser does, so a row is
    // offered on the fragment the results below are already narrowing on.
    let fragment = unquote(fragment.trim()).to_lowercase();
    // Tokenized as the predicate does, not on whitespace: a place label reads
    // `Berlin - Germany`, and splitting that on spaces looks for a literal `-`
    // the haystack never contains — so typing back the row just offered would
    // empty the list.
    let words = notema_domain::Location::search_tokens(&fragment);
    rows.iter()
        .enumerate()
        .filter(|(_, row)| match tab {
            // A place row's label is its display name but its value is the query
            // the row searches; match the query, which is what the count is of.
            FilterTab::Locations => {
                let haystack = row.search_value.to_lowercase();
                words.iter().all(|word| haystack.contains(word))
            }
            _ => row.label.to_lowercase().contains(&fragment),
        })
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::{state::ListNav, test_support::app_in_temp};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::fs;

    /// Two entries carrying values that overlap as substrings (`app`/`apple`),
    /// differ only in case, and a place whose name is two words — the shapes a
    /// completion has to get right.
    fn fixture() -> AppModel {
        let mut app = app_in_temp(|root| {
            let dir = root.join("work").join("2026-07-01");
            fs::create_dir_all(&dir).unwrap();
            for (name, tags, city) in [
                ("a", "\"apple\", \"App\"", "Berlin"),
                ("b", "\"apple\", \"pear\"", "Bern"),
            ] {
                fs::write(
                    dir.join(format!("{name}.md")),
                    format!(
                        "+++\nschema_version = 1\n\n[entry]\ntags = [{tags}]\n\n[time]\ncreated_at = \"2026-07-01T10:00:00+02:00\"\n\n[location]\ncity = \"{city}\"\ncountry = \"Germany\"\n+++\n\nbody\n"
                    ),
                )
                .unwrap();
            }
        });
        app.begin_search();
        app
    }

    /// Type `query` into the search field one key at a time, as the event loop
    /// does — so the suggestions refresh the way they do in the app.
    fn type_query(app: &mut AppModel, query: &str) {
        for ch in query.chars() {
            app.search_input_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
    }

    fn labels(app: &AppModel) -> Vec<&str> {
        app.search
            .suggestions
            .rows()
            .map(|row| row.label.as_str())
            .collect()
    }

    #[test]
    fn a_finished_prefix_offers_its_whole_vocabulary_then_narrows() {
        let mut app = fixture();
        type_query(&mut app, "tags:");
        // Ranked count-first, like the browser's rows: `apple` is on both entries.
        assert_eq!(labels(&app), vec!["apple", "App", "pear"]);
        assert!(app.suggestions_open());

        type_query(&mut app, "app");
        // Case-insensitive substring — the same reading `tags:app` already has.
        assert_eq!(labels(&app), vec!["apple", "App"]);

        // Nothing highlighted until the list is arrowed into, so `Enter` is still
        // the entry list's.
        assert_eq!(app.search.suggestions.selected_index(), None);
    }

    /// A space after the prefix is part of typing the value, so the list stays
    /// up — and completing into it replaces the space rather than writing beside
    /// it.
    #[test]
    fn a_space_after_the_prefix_keeps_the_whole_vocabulary_on_offer() {
        let mut app = fixture();
        type_query(&mut app, "tags: ");
        assert_eq!(labels(&app), vec!["apple", "App", "pear"]);
        assert!(app.suggestions_open());

        app.commit_first_suggestion();
        assert_eq!(app.search.query.as_str(), "tags:\"apple\"");
    }

    #[test]
    fn a_committed_value_is_quoted_and_the_caret_lands_after_it() {
        let mut app = fixture();
        type_query(&mut app, "tags:app");
        app.move_suggestion_highlight(1);
        app.commit_first_suggestion();

        assert_eq!(app.search.query.as_str(), "tags:\"apple\"");
        assert_eq!(app.search.query.cursor_byte(), 12);
        // Finished: the list goes down rather than offering the value again.
        assert!(!app.suggestions_open());

        // The next alternative is its own value, and opens on its own.
        type_query(&mut app, "+");
        assert_eq!(labels(&app), vec!["apple", "App", "pear"]);
    }

    /// A place has no exact mode, so its value arrives bare — and a two-word one
    /// is exactly what is hard to type by hand.
    #[test]
    fn a_committed_place_arrives_unquoted() {
        let mut app = fixture();
        type_query(&mut app, "location:berl");
        // The row reads as the place's display label but commits the query the
        // browser's row would launch — the two differ only here.
        assert_eq!(labels(&app), vec!["Berlin - Germany"]);
        app.commit_first_suggestion();
        assert_eq!(app.search.query.as_str(), "location:Berlin, Germany");
    }

    /// Every word must appear, in any order — what `location_predicate` does, so
    /// a row is offered exactly when it would be returned.
    #[test]
    fn a_place_matches_on_every_word_in_any_order() {
        let mut app = fixture();
        type_query(&mut app, "location:germany ber");
        assert_eq!(labels(&app), vec!["Berlin - Germany", "Bern - Germany"]);
        type_query(&mut app, "n");
        assert_eq!(labels(&app), vec!["Bern - Germany"]);
    }

    /// The row reads `Berlin - Germany`, so typing it back has to keep offering
    /// it: splitting the fragment on whitespace would look for a literal `-` the
    /// searched value never contains and empty the list mid-word.
    #[test]
    fn typing_back_a_place_label_keeps_offering_it() {
        let mut app = fixture();
        type_query(&mut app, "location:Berlin - Ger");
        assert_eq!(labels(&app), vec!["Berlin - Germany"]);
    }

    /// `Tab` takes the first row when the list is only advisory — but a dismissed
    /// list is not on screen, so whatever was armed before `Esc` cannot be what
    /// the user meant.
    #[test]
    fn tab_after_a_dismissal_commits_the_first_row_not_the_armed_one() {
        let mut app = fixture();
        type_query(&mut app, "tags:app");
        app.move_suggestion_highlight(1);
        app.move_suggestion_highlight(1);
        assert_eq!(app.search.suggestions.selected_index(), Some(1));

        app.dismiss_suggestions();
        app.commit_first_suggestion();

        assert_eq!(app.search.query.as_str(), "tags:\"apple\"");
    }

    /// An opening quote with no partner is a fragment like any other. Offering
    /// through it is what lets the list close the pair.
    #[test]
    fn a_half_typed_chip_still_offers_its_value() {
        let mut app = fixture();
        type_query(&mut app, "tags:\"app");
        assert_eq!(labels(&app), vec!["apple", "App"]);
        // And the results underneath are narrowing on the same fragment: a list
        // offering `apple` over an empty result list was two readings of one
        // value.
        assert_eq!(app.search_results().len(), 2);
        app.commit_first_suggestion();
        assert_eq!(app.search.query.as_str(), "tags:\"apple\"");
    }

    /// Esc has to survive the next keystroke, or it would undo itself while the
    /// value it was pressed on is still being typed.
    #[test]
    fn dismissing_outlasts_typing_but_not_the_next_filter() {
        let mut app = fixture();
        type_query(&mut app, "tags:app");
        app.dismiss_suggestions();
        assert!(!app.suggestions_open());

        type_query(&mut app, "l");
        assert!(
            !app.suggestions_open(),
            "still the value Esc was pressed on"
        );
        // The rows are still built, so `Tab` has something to complete.
        assert_eq!(labels(&app), vec!["apple"]);

        type_query(&mut app, "; tags:");
        assert!(app.suggestions_open(), "a new filter starts undismissed");
    }
}
