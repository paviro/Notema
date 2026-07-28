//! The search box's suggestion list: which values it offers for the filter value
//! the caret is in, and what committing one writes.
//!
//! The candidates are the filter browser's rows for that facet, so a suggestion
//! and a browser row are the same value ranked the same way, and neither pays for
//! a walk the other already did.
//!
//! Nothing is highlighted until the list is arrowed into. Typing `tags:app` and
//! pressing `Enter` still opens the selected result — the substring narrowing 07
//! preserves is the default reading of a fragment, and a list that guesses
//! otherwise would take the key away from someone who meant it.

use crate::tui::{
    app::AppModel,
    features::filter::FilterRow,
    render::tab_strip::StripTab,
    state::{FilterTab, ListNav, SelectableList, SuggestionRow},
};

use super::offsets;

impl AppModel {
    /// Whether the suggestion list is on screen.
    pub(crate) fn suggestions_open(&self) -> bool {
        self.is_search_input_active() && self.search.suggestions.is_open()
    }

    /// Rebuild the suggestions for wherever the caret now is. Cheap after the
    /// first call per library change: the candidates come from a memo, so this is
    /// a filter over a vec rather than a walk of the corpus.
    pub(crate) fn refresh_search_suggestions(&mut self) {
        let Some((tab, fragment, start)) = self.suggestion_target() else {
            self.search.suggestions.clear();
            return;
        };
        // Dismissal is keyed to the value it was made on, so typing more of that
        // value keeps the list down and moving to another filter brings it back.
        let dismissed = self.search.suggestions.dismissed.filter(|at| *at == start);
        let rows = self.cached_filter_rows(&self.search.scope);
        self.search.suggestions.rows = matching_rows(&rows[tab.index()], tab, &fragment);
        // Nothing highlighted: a recompute follows a keystroke, and a keystroke
        // means the typed fragment is still what was meant.
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
        let Some(row) = self.search.suggestions.rows.get(index) else {
            return;
        };

        let written = tab.launch_value(&row.value);
        let mut updated = String::with_capacity(query.len() + written.len());
        updated.push_str(&query[..caret.value.start]);
        updated.push_str(&written);
        let caret_byte = updated.len();
        updated.push_str(&query[caret.value.end..]);

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
        let index = self.search.suggestions.selected_index().unwrap_or(0);
        self.commit_suggestion(index);
    }
}

/// The rows offering `fragment`, in the browser's count-first order.
///
/// Matching mirrors the predicate the prefix runs, so a row is offered exactly
/// when typing the fragment would already be narrowing toward it: a substring for
/// the token facets, every word for a place.
fn matching_rows(rows: &[FilterRow], tab: FilterTab, fragment: &str) -> Vec<SuggestionRow> {
    // A half-typed chip is still a fragment: `tags:"app` has no closing quote, so
    // the parser leaves the `"` in the needle and the results blank. Offering
    // `apple` there is what closes the pair.
    let fragment = fragment.trim().trim_start_matches('"').to_lowercase();
    let words: Vec<&str> = fragment.split_whitespace().collect();
    rows.iter()
        .filter(|row| match tab {
            // A place row's label is its display name but its value is the query
            // the row searches; match the query, which is what the count is of.
            FilterTab::Locations => {
                let haystack = row.search_value.to_lowercase();
                words.iter().all(|word| haystack.contains(word))
            }
            _ => row.label.to_lowercase().contains(&fragment),
        })
        .map(|row| SuggestionRow {
            label: row.label.clone(),
            value: row.search_value.clone(),
            count: row.count,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::test_support::app_in_temp;
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
            .rows
            .iter()
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

    /// An opening quote with no partner is a fragment like any other. Offering
    /// through it is what lets the list close the pair.
    #[test]
    fn a_half_typed_chip_still_offers_its_value() {
        let mut app = fixture();
        type_query(&mut app, "tags:\"app");
        assert_eq!(labels(&app), vec!["apple", "App"]);
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
