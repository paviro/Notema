//! Bench-only entry points, compiled behind the `bench` feature and re-exported
//! from the crate root so `benches/` can reach the otherwise-private TUI paths.
//! Not part of the shipped binary.

use std::fs;
use std::path::Path;

use notema_domain::{SearchScope, feelings};
use notema_storage::JournalStore;
use ratatui::{Terminal, backend::TestBackend};

use super::app::{AppModel, Focus};
use super::search::search_loaded_entries;
use super::state::FilterTab;
use crate::config::Config;

/// An opaque, fully-loaded app handle for benchmarks. Wraps the private `AppModel` so
/// the bench API stays public without exposing the TUI's internal types.
pub struct BenchApp(AppModel);

/// Number of filter-browser tabs, so a bench can sweep them without naming the
/// private `FilterTab`.
pub const FILTER_TAB_COUNT: usize = FilterTab::COUNT;

/// Which corpus shape [`app_with_corpus`] writes.
pub enum BenchCorpus {
    /// Fixed vocabulary — 30 tags, 20 people, 12 activities, no feelings, no
    /// locations — regardless of corpus size. Exercises paths that scale with the
    /// *number of entries*.
    Narrow,
    /// Vocabulary that grows with the corpus, so paths that scale with the number
    /// of *distinct values* are actually exercised. See [`wide_entry_text`].
    Wide,
}

/// Build a [`BenchApp`] over a fresh on-disk store holding `count` plaintext
/// entries across four journals, with the first entry selected and the reader
/// focused so a draw exercises the markdown render path. `root` is a caller-owned
/// tempdir that must outlive the returned handle.
pub fn app_with_corpus(root: &Path, count: usize, corpus: BenchCorpus) -> BenchApp {
    let journal_root = root.join("journals");
    let config_path = root.join("config.toml");
    let store = JournalStore::new(&journal_root, root);
    store.ensure().unwrap();

    let vocabulary: Vec<&'static str> = feelings().collect();
    for index in 0..count {
        let journal = index % 4;
        let dir = journal_root
            .join(format!("journal-{journal}"))
            .join(format!(
                "2020-{:02}-{:02}",
                1 + (index % 12),
                1 + (index % 28)
            ));
        fs::create_dir_all(&dir).unwrap();
        let stamp = format!(
            "2020-{:02}-{:02}T{:02}-00-00",
            1 + (index % 12),
            1 + (index % 28),
            index % 24
        );
        let text = match corpus {
            BenchCorpus::Narrow => narrow_entry_text(index),
            BenchCorpus::Wide => wide_entry_text(index, count, &vocabulary),
        };
        fs::write(dir.join(format!("{stamp}-{index:05}.md")), text).unwrap();
    }

    let config = Config::new(root.to_path_buf());
    let mut app = AppModel::new(config_path, config, store).unwrap();
    app.select_journal(0);
    app.select_entry_index(0);
    app.focus_reader_from_click();
    BenchApp(app)
}

/// [`app_with_corpus`] over the fixed-vocabulary corpus. The render and search
/// benches keep using this so their numbers stay comparable across the whole
/// history — changing the corpus under them would silently rebase every
/// previously recorded figure.
pub fn app_with_entries(root: &Path, count: usize) -> BenchApp {
    app_with_corpus(root, count, BenchCorpus::Narrow)
}

/// Render one full frame to an in-memory [`TestBackend`] — the whole TUI draw
/// path (layout, journal/entry columns, markdown reader) with no real terminal.
pub fn draw_frame(app: &mut BenchApp, width: u16, height: u16) {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut view = super::ui::ViewState::default();
    let active_theme = app.0.appearance.theme.clone();
    let mut context = super::ui::RenderContext::new(&active_theme, &mut view);
    terminal
        .draw(|frame| super::render::draw(frame, &mut app.0, &mut context))
        .unwrap();
}

/// Word-search the loaded entries, returning the hit count.
pub fn search(app: &BenchApp, query: &str) -> usize {
    search_loaded_entries(&app.0.library.entries, query, &SearchScope::AllJournals).len()
}

/// Open the filter browser over all journals — what pressing the key costs, every
/// tab built — and return the total row count. Closes the overlay again, so the
/// call is repeatable.
///
/// The focus is forced to the journals column because that is what widens the
/// captured scope to all journals; `app_with_corpus` leaves the reader focused,
/// which would scope to one journal and bench a quarter of the corpus.
pub fn open_filter(app: &mut BenchApp) -> usize {
    app.0.nav.focus = Focus::Journals;
    app.0.begin_filter();
    let rows = app
        .0
        .filter_state()
        .map_or(0, |state| state.rows.iter().map(Vec::len).sum());
    app.0.close_overlay();
    rows
}

/// Build one filter tab's rows over all journals, returning its title and row
/// count. `tab` indexes [`FILTER_TAB_COUNT`].
pub fn filter_tab_rows(app: &BenchApp, tab: usize) -> (&'static str, usize) {
    let tab = FilterTab::ALL[tab];
    let rows = app.0.filter_rows(&SearchScope::AllJournals, tab);
    (tab.title(), rows.len())
}

fn narrow_entry_text(index: usize) -> String {
    format!(
        "+++\n\
         schema_version = 1\n\n\
         [entry]\n\
         tags = [\"tag-{}\", \"tag-{}\"]\n\
         people = [\"person-{}\"]\n\
         activities = [\"activity-{}\"]\n\
         mood = {}\n\n\
         [time]\n\
         created_at = \"2020-01-01T08:00:00+00:00\"\n\
         +++\n\n\
         # Entry {index}\n\n\
         A representative journal body with some **bold** text, a [link](https://example.com),\n\
         and a short list:\n\n\
         - first point\n\
         - second point\n\n\
         Closing line for entry {index}.\n",
        index % 30,
        index % 15,
        index % 20,
        index % 12,
        (index % 11) as i8 - 5,
    )
}

/// The narrow corpus's body and fixed-vocabulary values, plus a long tail whose
/// size is a fraction of `count`: `topic-*` tags, `contact-*` people and
/// `city-*` places. Every long-tail value is a prefix of nine others
/// (`topic-1` inside `topic-10`…), which is the substring-superset shape the
/// facet counts have to get right and the worst case for any index over them.
///
/// `road`/`suburb` grow faster than the settlement so the number of distinct
/// location *haystacks* grows independently of the number of place groups — the
/// two are separate costs. The country is derived from the city, so place groups
/// number exactly as many as cities.
///
/// Feelings cannot grow with the corpus: the vocabulary is the 170 canonical
/// words in `FEELING_GROUPS` and anything else is dropped on read. They are here
/// because matching one used to scan the whole alias table, which is a per-entry
/// cost rather than a per-vocabulary one.
fn wide_entry_text(index: usize, count: usize, vocabulary: &[&'static str]) -> String {
    let topics = (count / 20).max(1);
    let contacts = (count / 50).max(1);
    let cities = (count / 40).max(1);
    let suburbs = (count / 10).max(1);
    let roads = (count / 5).max(1);
    let city = index % cities;
    format!(
        "+++\n\
         schema_version = 1\n\n\
         [entry]\n\
         tags = [\"tag-{}\", \"topic-{}\"]\n\
         people = [\"person-{}\", \"contact-{}\"]\n\
         activities = [\"activity-{}\"]\n\
         feelings = [\"{}\", \"{}\"]\n\
         mood = {}\n\n\
         [location]\n\
         city = \"city-{city}\"\n\
         country = \"country-{}\"\n\
         suburb = \"suburb-{}\"\n\
         road = \"road-{}\"\n\n\
         [time]\n\
         created_at = \"2020-01-01T08:00:00+00:00\"\n\
         +++\n\n\
         # Entry {index}\n\n\
         A representative journal body with some **bold** text, a [link](https://example.com),\n\
         and a short list:\n\n\
         - first point\n\
         - second point\n\n\
         Closing line for entry {index}.\n",
        index % 30,
        index % topics,
        index % 20,
        index % contacts,
        index % 12,
        vocabulary[index % vocabulary.len()],
        vocabulary[(index * 7 + 3) % vocabulary.len()],
        (index % 11) as i8 - 5,
        city % 8,
        index % suburbs,
        index % roads,
    )
}
