//! TUI hot-path benchmarks: in-memory word search and a full-frame render over
//! deterministic 1k/10k/25k corpora, plus the filter browser over a corpus whose
//! vocabulary grows with its size. Plain `Instant` timing (`harness = false`),
//! matching the analytics and storage scan benches. Needs `--features bench`,
//! which the `[[bench]]` entry requires automatically.

use std::{hint::black_box, time::Instant};

use notema::bench::{
    BenchCorpus, FILTER_TAB_COUNT, app_with_corpus, app_with_entries, draw_frame, filter_tab_rows,
    open_filter, search,
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
    }

    // The filter browser scales with the number of *distinct* values, so it gets
    // the wide corpus; the render and search figures above stay on the narrow one
    // so they remain comparable with previously recorded numbers.
    for size in [1_000, 10_000, 25_000] {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with_corpus(dir.path(), size, BenchCorpus::Wide);
        let iterations = if size < 10_000 { 3 } else { 1 };

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
    }
}
