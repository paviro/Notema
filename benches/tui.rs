//! TUI hot-path benchmarks: in-memory word search and a full-frame render over
//! deterministic 1k/10k/25k corpora, plus the filter browser over a corpus whose
//! vocabulary grows with its size. Plain `Instant` timing (`harness = false`),
//! matching the analytics and storage scan benches. Needs `--features bench`,
//! which the `[[bench]]` entry requires automatically.

use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use notema::bench::{
    BenchCorpus, FILTER_TAB_COUNT, METADATA_KIND_COUNT, app_with_corpus, app_with_entries,
    bench_terminal, close_picker, draw_frame, editor_highlight, editor_input,
    filter_metadata_picker, filter_tab_rows, first_entry_path, install_snapshot, library_snapshot,
    open_editor_with_body, open_filter, open_location_picker, open_metadata_picker, refresh_path,
    reload_journal_list, rename_journal, search, search_query,
};

fn main() {
    for size in [1_000, 10_000, 25_000] {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_entries(dir.path(), size);
        let iterations = if size < 10_000 { 20 } else { 5 };

        // Full-frame render (layout + journal/entry columns + markdown reader).
        draw_frame(&mut app, 120, 40);
        let started = Instant::now();
        for _ in 0..iterations {
            draw_frame(black_box(&mut app), 120, 40);
        }
        println!("render_frame/{size}: {:?}", started.elapsed() / iterations);

        // In-memory word search across every loaded entry.
        let _ = black_box(search(&app, "representative"));
        let started = Instant::now();
        for _ in 0..iterations {
            black_box(search(black_box(&app), black_box("representative bold")));
        }
        println!("search/{size}: {:?}", started.elapsed() / iterations);

        // The reload spectrum, cheapest first. Read these against
        // `cargo bench -p notema-storage --bench scan`, which times the full walk
        // each of them exists to avoid.
        reload_journal_list(&mut app);
        let started = Instant::now();
        for _ in 0..iterations {
            reload_journal_list(black_box(&mut app));
        }
        println!(
            "reload_journal_list/{size}: {:?}",
            started.elapsed() / iterations
        );

        // One changed entry file, reconciled without walking the corpus.
        let path = first_entry_path(&app);
        refresh_path(&mut app, &path);
        let started = Instant::now();
        for _ in 0..iterations {
            refresh_path(black_box(&mut app), black_box(&path));
        }
        println!("refresh_path/{size}: {:?}", started.elapsed() / iterations);

        // An archive rename, applied in memory. One iteration is both directions,
        // because the rename is one-way and a repeat would find nothing to move.
        rename_journal(&mut app, "journal-0", "journal-0.archived");
        rename_journal(&mut app, "journal-0.archived", "journal-0");
        let started = Instant::now();
        for _ in 0..iterations {
            rename_journal(black_box(&mut app), "journal-0", "journal-0.archived");
            rename_journal(black_box(&mut app), "journal-0.archived", "journal-0");
        }
        println!(
            "rename_journal/{size}: {:?}",
            started.elapsed() / (iterations * 2)
        );

        // Installing a whole-library snapshot — the TUI half of a full reload.
        // The copy is untimed: production moves the worker's snapshot in rather
        // than cloning it, so the elapsed time is accumulated per iteration
        // instead of measured across the loop.
        let reference = library_snapshot(&app);
        install_snapshot(&mut app, reference.clone());
        let mut elapsed = Duration::ZERO;
        for _ in 0..iterations {
            let snapshot = reference.clone();
            let started = Instant::now();
            install_snapshot(black_box(&mut app), black_box(snapshot));
            elapsed += started.elapsed();
        }
        println!("install_snapshot/{size}: {:?}", elapsed / iterations);
    }

    // The filter browser scales with the number of *distinct* values, so it gets
    // the wide corpus; the render and search figures above stay on the narrow one
    // so they remain comparable with previously recorded numbers.
    for size in [1_000, 10_000, 25_000] {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_corpus(dir.path(), size, BenchCorpus::Wide);
        let iterations = if size < 10_000 { 20 } else { 5 };

        // Opening the dialog: every tab's rows and counts, over all journals.
        let _ = black_box(open_filter(&mut app));
        let started = Instant::now();
        for _ in 0..iterations {
            black_box(open_filter(black_box(&mut app)));
        }
        println!("filter_open/{size}: {:?}", started.elapsed() / iterations);

        // Per tab, so a facet that scales badly is not hidden by the others. The
        // row count rides along: it is the vocabulary size the timing is against,
        // and the reason this corpus exists.
        for tab in 0..FILTER_TAB_COUNT {
            let (title, rows) = filter_tab_rows(&app, tab);
            let started = Instant::now();
            for _ in 0..iterations {
                black_box(filter_tab_rows(black_box(&app), black_box(tab)));
            }
            println!(
                "filter_tab/{title}/{size}: {:?} ({rows} rows)",
                started.elapsed() / iterations
            );
        }

        // The pickers walk the whole library uncached, once per dialog open, and
        // are the counterpart to the browser: the browser lists a facet to search
        // by, the picker lists it to assign from.
        for kind in 0..METADATA_KIND_COUNT {
            let (title, values) = open_metadata_picker(&mut app, kind);
            close_picker(&mut app);
            let started = Instant::now();
            for _ in 0..iterations {
                black_box(open_metadata_picker(black_box(&mut app), black_box(kind)));
                close_picker(&mut app);
            }
            println!(
                "metadata_picker/{title}/{size}: {:?} ({values} values)",
                started.elapsed() / iterations
            );

            // Typing into the open dialog: the refilter alone, with the value
            // list already built.
            open_metadata_picker(&mut app, kind);
            let _ = black_box(filter_metadata_picker(&mut app, "topic-1"));
            let started = Instant::now();
            for _ in 0..iterations {
                black_box(filter_metadata_picker(
                    black_box(&mut app),
                    black_box("topic-1"),
                ));
            }
            println!(
                "metadata_filter/{title}/{size}: {:?} ({values} values)",
                started.elapsed() / iterations
            );
            close_picker(&mut app);
        }

        // Presets are capped at 20, so no count rides along — the vocabulary this
        // is timed against is the distinct address labels, `count / 5` of them.
        let _ = black_box(open_location_picker(&mut app));
        let started = Instant::now();
        for _ in 0..iterations {
            black_box(open_location_picker(black_box(&mut app)));
        }
        println!(
            "location_picker/{size}: {:?}",
            started.elapsed() / iterations
        );

        // A whole query, not just the text kernel: the parse, one predicate per
        // filter, and the scoring pass when there is text to score.
        for (label, query) in SEARCH_QUERIES {
            let hits = search_query(&mut app, query);
            let started = Instant::now();
            for _ in 0..iterations {
                black_box(search_query(black_box(&mut app), black_box(query)));
            }
            println!(
                "search_query/{label}/{size}: {:?} ({hits} hits, `{query}`)",
                started.elapsed() / iterations
            );
        }
    }

    editor();
}

/// The editor re-scans the whole buffer on every keystroke, so it scales with
/// document length rather than corpus size — an axis the corpus sweep holds
/// fixed and therefore cannot see.
/// The corpus is small on purpose — it exists only to give the editor a journal
/// to open in.
fn editor() {
    for lines in [200, 2_000, 10_000] {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_entries(dir.path(), 100);
        let mut terminal = bench_terminal(120, 40);
        let iterations = 20;

        open_editor_with_body(&mut terminal, &mut app, lines);

        // One keystroke into the buffer, through the action dispatch.
        editor_input(&mut terminal, &mut app, 'x');
        let started = Instant::now();
        for _ in 0..iterations {
            editor_input(black_box(&mut terminal), black_box(&mut app), 'x');
        }
        println!("editor_input/{lines}: {:?}", started.elapsed() / iterations);

        // The re-highlight the next frame pays for. The keystroke that makes it a
        // miss is untimed — timing it here would count the dispatch twice.
        let mut elapsed = Duration::ZERO;
        for _ in 0..iterations {
            editor_input(&mut terminal, &mut app, 'x');
            let started = Instant::now();
            editor_highlight(black_box(&mut app));
            elapsed += started.elapsed();
        }
        println!("editor_highlight/{lines}: {:?}", elapsed / iterations);
    }
}

/// The query shapes worth separating: an unquoted value matches substrings, a
/// quoted one is exact, `location:` is substring even quoted, and a text part
/// pulls in the scoring pass the filters alone skip.
const SEARCH_QUERIES: [(&str, &str); 5] = [
    ("substring", "tags:topic-5"),
    ("exact", "tags:\"topic-5\""),
    ("location", "location:city-3"),
    // Both values are `-1`, which is the residue the topic and contact cycles
    // share: any other pair leaves the intersection empty at some corpus size.
    ("chained", "tags:topic-1; people:contact-1"),
    ("with_text", "tags:topic-5; representative"),
];
