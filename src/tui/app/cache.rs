//! Per-frame render memo caches for [`AppModel`]: the entry-row, rendered-body,
//! analytics, and windowed-correlation memos plus the version counters that
//! invalidate them, and the `&self` accessors that read them.

use super::{AppModel, Mode, RenderedEntryBody};
use crate::tui::entry_rows::{EntryRowCache, build_entry_row_cache};
use crate::tui::features::insights::{InsightsScope, InsightsTimeframe};
use notema_analytics::{Analytics, Correlations, analyze, build_correlations};
use notema_domain::{Entry, entry_group_date};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

/// Identifies the inputs that fully determine the entry-list rows, so a matching
/// key means the cached [`EntryRowCache`] can be reused. Notably excludes the
/// scroll offset and selected index — those are applied when drawing, not baked
/// into the rows.
#[derive(Clone, PartialEq)]
struct EntryRowKey {
    /// [`RenderCaches::rows_version`] — bumped whenever `entries` or
    /// `search.hits` change, since the rows are built from the hits in Search
    /// mode.
    version: u64,
    mode: Mode,
    journal: Option<String>,
    text_width: u16,
    /// The rows bake in the theme's chrome, glyphs, and colors, and the theme
    /// picker live-previews across themes — the key must notice any change.
    theme: crate::tui::theme::Theme,
}

/// Cache key for [`AppModel::cached_entry_body`]: the rendered body is fully
/// determined by which entry is shown (`path` + `version`), the wrap width,
/// whether link URLs are shown, and the theme (markdown colors, glyphs, and
/// syntax highlighting are baked
/// into the lines — the picker's live preview must rebuild them). The
/// `version` is [`RenderCaches::entries_version`] — not the rows version —
/// because the body depends only on entry content, not on which search hits
/// are showing (a hit change that swaps the shown entry already changes
/// `path`).
#[derive(Clone, PartialEq)]
struct EntryBodyKey {
    version: u64,
    path: Option<PathBuf>,
    width: usize,
    theme: crate::tui::theme::Theme,
    show_link_urls: bool,
}

/// The per-frame render memo caches and the version counters that invalidate
/// them. Grouped so `AppModel` carries one field instead of four and the versions
/// have a single home. All three caches are read on the `&self` render/hit-test
/// paths, so each is a `RefCell`.
///
/// Two counters, because the caches have different dependencies:
/// - [`Self::entries_version`] bumps only when the `entries` Vec changes. It
///   keys the body and analytics caches, which depend on entry content alone.
/// - [`Self::rows_version`] bumps when entries **or** search hits change. It
///   keys the row cache, which is built from the hits in Search mode.
///
/// A search recompute therefore bumps only `rows_version`, so the (more
/// expensive) rendered-body and journal-insights memos survive keystroke-driven
/// query edits instead of rebuilding every time.
#[derive(Default)]
pub(crate) struct RenderCaches {
    /// Memoized entry-list rows, keyed by [`EntryRowKey`].
    entry_row_cache: RefCell<Option<(EntryRowKey, Rc<EntryRowCache>)>>,
    /// Memoized rendered body lines for the entry reader, keyed by
    /// [`EntryBodyKey`]. Rebuilt only when the shown entry or wrap width changes,
    /// so scroll and image ticks reuse it.
    entry_body_cache: RefCell<Option<(EntryBodyKey, Rc<RenderedEntryBody>)>>,
    /// Memoized analytics for the `(entries_version, scope key)` they were
    /// computed for. The scope key is the journal name for `Journal` scope or a
    /// sentinel for `All`, so switching tab/scope reuses the build instead of
    /// rescanning every entry each frame.
    analytics_cache: RefCell<Option<(u64, String, Rc<Analytics>)>>,
    /// Memoized correlations for a *windowed* slice of the scope, keyed by
    /// `(entries_version, scope key, timeframe)`. Separate from `analytics_cache`
    /// because it recomputes against the window's own baseline mean (so `mood_delta`
    /// answers "what lifts/drains me *this week*"), and only when the Drivers tab
    /// needs it.
    windowed_cache: RefCell<WindowedCache>,
    entries_version: u64,
    rows_version: u64,
}

/// The windowed-correlations memo: `(entries_version, scope key, timeframe)` and
/// the correlations built for them.
type WindowedCache = Option<(u64, String, InsightsTimeframe, Rc<Correlations>)>;

impl RenderCaches {
    /// The `entries` Vec changed: both the entries-keyed (body, analytics) and
    /// rows-keyed caches are stale.
    pub(super) fn bump_entries(&mut self) {
        self.entries_version = self.entries_version.wrapping_add(1);
        self.rows_version = self.rows_version.wrapping_add(1);
    }

    /// Only the entry-list rows changed (a search recompute); the body and
    /// analytics caches, keyed on [`Self::entries_version`], stay valid.
    pub(crate) fn bump_rows(&mut self) {
        self.rows_version = self.rows_version.wrapping_add(1);
    }

    /// Return the memoized rows for `key`, building them with `build` on a miss.
    fn rows(&self, key: EntryRowKey, build: impl FnOnce() -> EntryRowCache) -> Rc<EntryRowCache> {
        if let Some((cached_key, cache)) = self.entry_row_cache.borrow().as_ref()
            && *cached_key == key
        {
            return cache.clone();
        }
        let cache = Rc::new(build());
        *self.entry_row_cache.borrow_mut() = Some((key, cache.clone()));
        cache
    }

    /// Return the memoized rendered body for `key`, building it with `build` on a
    /// miss (entry or width changed, or the store reloaded).
    fn body(
        &self,
        key: EntryBodyKey,
        build: impl FnOnce() -> RenderedEntryBody,
    ) -> Rc<RenderedEntryBody> {
        if let Some((cached_key, body)) = self.entry_body_cache.borrow().as_ref()
            && *cached_key == key
        {
            return body.clone();
        }
        let body = Rc::new(build());
        *self.entry_body_cache.borrow_mut() = Some((key, body.clone()));
        body
    }

    /// Return the memoized analytics for `scope_key` at `version`, building them
    /// with `build` on a miss (scope/journal changed, or the store reloaded).
    fn analytics(
        &self,
        version: u64,
        scope_key: &str,
        build: impl FnOnce() -> Analytics,
    ) -> Rc<Analytics> {
        if let Some((cached_version, key, analytics)) = self.analytics_cache.borrow().as_ref()
            && *cached_version == version
            && key == scope_key
        {
            return analytics.clone();
        }
        let analytics = Rc::new(build());
        *self.analytics_cache.borrow_mut() =
            Some((version, scope_key.to_string(), analytics.clone()));
        analytics
    }

    /// Return the memoized windowed correlations for `(version, scope_key,
    /// timeframe)`, building them with `build` on a miss (window, scope, or
    /// entries changed).
    fn windowed(
        &self,
        version: u64,
        scope_key: &str,
        timeframe: InsightsTimeframe,
        build: impl FnOnce() -> Correlations,
    ) -> Rc<Correlations> {
        if let Some((cached_version, key, cached_tf, correlations)) =
            self.windowed_cache.borrow().as_ref()
            && *cached_version == version
            && key == scope_key
            && *cached_tf == timeframe
        {
            return correlations.clone();
        }
        let correlations = Rc::new(build());
        *self.windowed_cache.borrow_mut() = Some((
            version,
            scope_key.to_string(),
            timeframe,
            correlations.clone(),
        ));
        correlations
    }
}

impl AppModel {
    /// The memoized entry-list rows for the current state, rebuilt only when the
    /// row-determining inputs (rows version, mode, journal, width) change. Returns
    /// an `Rc` so callers can read it while holding a `&mut AppModel` borrow elsewhere.
    pub(crate) fn entry_rows(&self, text_width: u16) -> Rc<EntryRowCache> {
        let key = EntryRowKey {
            version: self.caches.rows_version,
            mode: self.nav.mode.clone(),
            journal: self.selected_journal().map(|journal| journal.name.clone()),
            text_width,
            theme: self.appearance.theme.clone(),
        };
        self.caches
            .rows(key, || build_entry_row_cache(self, text_width))
    }

    /// Return the memoized rendered body for the entry at `path`/`width`, building
    /// it with `build` only on a cache miss (entry or width changed, or the store
    /// reloaded). The markdown parse+render `build` runs is the reader pane's
    /// dominant per-frame cost, so this keeps scroll and image-tick redraws cheap.
    pub(crate) fn cached_entry_body(
        &self,
        path: Option<&Path>,
        width: usize,
        build: impl FnOnce() -> RenderedEntryBody,
    ) -> Rc<RenderedEntryBody> {
        let key = EntryBodyKey {
            version: self.caches.entries_version,
            path: path.map(Path::to_path_buf),
            width,
            theme: self.appearance.theme.clone(),
            show_link_urls: self.services.config.ui.layout.reader.show_link_urls,
        };
        self.caches.body(key, build)
    }

    /// The memoized analytics for the current scope, or `None` in `Journal`
    /// scope when no journal is selected. `All` scope always yields a value
    /// (aggregating every loaded entry).
    ///
    /// `today` (for the current-streak calculation) is read from the wall clock
    /// but deliberately kept out of the cache key: only `current_streak` depends
    /// on it, so a streak that goes stale across a midnight boundary with no
    /// reload is acceptable and self-heals on the next entry change. Keying on
    /// the date instead would rebuild the whole aggregate every frame after
    /// midnight for no real benefit.
    pub(crate) fn cached_analytics(&self) -> Option<Rc<Analytics>> {
        let today = chrono::Local::now().date_naive();
        match self.nav.insights_scope {
            InsightsScope::Journal => {
                let name = self.selected_journal()?.name.clone();
                Some(
                    self.caches
                        .analytics(self.caches.entries_version, &name, || {
                            analyze(&self.selected_entries(), today)
                        }),
                )
            }
            InsightsScope::All => {
                // A NUL-prefixed key can't collide with a journal name.
                Some(
                    self.caches
                        .analytics(self.caches.entries_version, "\u{0}all", || {
                            let entries: Vec<&Entry> = self.library.entries.iter().collect();
                            analyze(&entries, today)
                        }),
                )
            }
        }
    }

    /// The memoized lift/drain correlations for the current scope, windowed to
    /// `nav.insights_timeframe`. `None` in `Journal` scope with no journal selected.
    /// Powers the Drivers ranking.
    pub(crate) fn cached_windowed_correlations(&self) -> Option<Rc<Correlations>> {
        let today = chrono::Local::now().date_naive();
        let timeframe = self.nav.insights_timeframe;
        let (scope_key, entries): (String, Vec<&Entry>) = match self.nav.insights_scope {
            InsightsScope::Journal => (
                self.selected_journal()?.name.clone(),
                self.selected_entries(),
            ),
            InsightsScope::All => (
                "\u{0}all".to_string(),
                self.library.entries.iter().collect(),
            ),
        };
        Some(
            self.caches
                .windowed(self.caches.entries_version, &scope_key, timeframe, || {
                    let windowed: Vec<&Entry> = match timeframe.window(today) {
                        None => entries.clone(),
                        Some((start, end)) => entries
                            .iter()
                            .copied()
                            .filter(|entry| {
                                entry_group_date(entry)
                                    .is_some_and(|date| start <= date && date <= end)
                            })
                            .collect(),
                    };
                    build_correlations(&windowed)
                }),
        )
    }
}
