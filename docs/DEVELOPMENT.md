# Development

## Setup

Needs the toolchain pinned in `rust-toolchain.toml` (Rust 1.96, with `clippy`
and `rustfmt`). `rustup` installs it automatically on first `cargo` invocation
in the repo.

Enable the versioned Git hooks once per clone:

```bash
git config core.hooksPath .githooks
```

The commit-message hook requires `type(scope): description`. Add `!` before the
colon for a breaking change: `type(scope)!: description`. CI checks the same
format for every commit pushed to `main` or included in a pull request.

## Checks

These mirror the CI jobs; run them before pushing.

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

CI runs `cargo audit --deny warnings` as well; local runs need
`cargo install cargo-audit`. Acknowledged advisories are listed in
[AUDIT.md](AUDIT.md).

The `fuse` feature is off by default. To check the mount wiring compiles:

```bash
cargo check -p notema --features fuse --locked   # needs libfuse3 headers
```

## Cross-compiled builds

Official releases are built by CI on version tags (see
[RELEASING.md](RELEASING.md)). For local cross-builds, `Makefile.toml` provides
per-target tasks through
[`cargo-make`](https://github.com/sagiegurari/cargo-make):

```bash
cargo make build-termux            # Android/Termux ARM64
cargo make build-x86-gnu           # x86_64 Linux (glibc)
cargo make build-macos-universal   # Intel + Apple Silicon
cargo make build-windows-gnu       # Windows x86_64
cargo make run-tests               # full workspace test suite
```

See [BUILDING.md](BUILDING.md) for prerequisites and the FUSE variants.

## Seeding development data

`notema-seed` is a dev-only tool (outside the shipped binary's dependency graph)
that fills a store with generated entries — handy for exercising the TUI or the
benchmarks against a realistic corpus. It writes into a real store, so point it
at a throwaway directory, not your journal:

```bash
cargo run -p notema-seed -- \
  --root /tmp/notema-dev/journals \
  --config-dir /tmp/notema-dev \
  --count 750
```

`--journal` names the journal to fill (default `Sample`), `--days` spreads the
creation dates, and `--seed <n>` makes the data set reproducible. Run
`cargo run -p notema-seed -- --help` for the full list.

## Benchmarks

Benchmarks are deterministic and run as plain timed binaries (`harness = false`)
over 1k / 10k / 25k corpora, printing one line per size. They exist to catch
performance regressions on the paths that scale with journal size. The `editor_*`
lines are the exception: they sweep document length (200 / 2k / 10k lines)
instead, because that is the axis the editor scales on.

| Bench | Crate | Covers |
|---|---|---|
| `analytics` | `notema-analytics` | Cadence/mood/correlation aggregation |
| `scan` | `notema-storage` | Full journal scan: walk + parse + preview + haystack |
| `tui` | root (`--features bench`) | Full-frame render, in-memory search, filter browser, metadata/location pickers, incremental reloads, editor keystrokes |

```bash
cargo bench -p notema-analytics --bench analytics
cargo bench -p notema-storage --bench scan
cargo bench --features bench --bench tui
```

The `tui` bench reaches otherwise-private TUI paths through the `bench` feature,
which exposes a small `notema::bench` module (a `BenchApp` handle plus one entry
point per benched path). The feature is dev-only
and never compiled into the shipped binary. Because the `[[bench]]` entry sets
`required-features`, a plain `cargo clippy --all-targets` *skips* this target —
lint it with `cargo clippy --workspace --all-targets --features bench`.

Two corpus shapes, chosen per bench:

- **Narrow** (`app_with_entries`) holds the vocabulary fixed — 30 tags, 20
  people, 12 activities, no feelings, no locations — however large the corpus
  gets. `render_frame` and `search` use it, and must keep using it: changing the
  corpus under them would silently rebase every number recorded so far.
- **Wide** (`app_with_corpus(.., BenchCorpus::Wide)`) grows the vocabulary with
  the corpus, so paths that scale with the number of *distinct* values are
  actually exercised. The filter-browser lines use it and print their row count
  alongside the timing, since that row count is the vocabulary the timing is
  against. Feelings are the exception: the vocabulary is the fixed set of
  canonical words, so that tab is bounded by construction — as is the picker's
  Activities line, at 12 values in both corpora.

The `filter_*` and `metadata_picker`/`location_picker` lines are two views of the
same facets: the browser lists a facet to search by, the picker lists it to
assign from, and neither is cached. `metadata_filter` is the picker's
per-keystroke refilter with the value list already built. The picker's cost is
dominated by the per-entry walk rather than the vocabulary — the 12-value
Activities line is within a factor of two of the 1280-value Tags one.

`editor_input` is one keystroke reaching the buffer; `editor_highlight` is the
whole-body re-scan the next frame pays for, which every keystroke makes a miss
by construction. Both are linear in document length, so read the 10k-line row
as the slope rather than as a realistic entry — a long journal entry is closer
to the 200-line row.

`search` and `search_query` are two different things. `search` is the text
kernel alone — no parse, no predicate — and stays on the narrow corpus so its
numbers remain comparable with everything recorded before. `search_query` is the
whole rescan the search box's debounce defers, on the wide corpus, one line per
query shape (unquoted substring, quoted exact, `location:`, chained filters,
filters plus scoring text) with its hit count and the query itself alongside.

The `reload_journal_list`, `refresh_path`, `rename_journal` and
`install_snapshot` lines are the incremental routes that replaced whole-library
reloads. Read them against `validate/{size}` in the `scan` bench, which is the
warm-cache full walk each of them exists to avoid — that is the comparison, and
none of them should ever approach it.

Reading the numbers: each line is the mean wall-clock time per iteration, e.g.
`scan/25000: 1.1s`. There is no built-in baseline comparison — record the 25k
numbers before a change and compare after. Treat a **>10% regression on any 25k
figure** as a regression to investigate, matching the workspace's performance
budget. The 25k row is the one that matters; the 1k/10k rows mostly surface
fixed per-run overhead.

Numbers are machine-relative; compare runs on the same hardware, ideally with a
quiet system and on the `release`/`bench` profile (which `cargo bench` uses).

## Startup timing

Benchmarks cover the paths that scale with journal size; they say nothing about
the fixed cost of starting up. `NOTEMA_TIMING=1` prints a phase-by-phase timeline
to stderr — cumulative and delta per step — so a slow launch on a device you
can't attach a profiler to (Termux on Android, iSH on iOS) can be attributed to
something specific.

`NOTEMA_TIMING=2` adds cache-miss causes and filesystem mtime precision. Level 1
keeps the phase timeline and aggregate library summaries without those details.

```bash
NOTEMA_TIMING=1 notema log "probe" 2> timing.log
NOTEMA_TIMING=1 notema 2> timing-tui.log   # lines flush after the TUI exits
```

It's gated on the env var rather than a cargo feature on purpose: the build that
needs measuring is the released one. With the variable unset each hook is one
relaxed atomic load.

Reading it:

- **`pre-main`** (Linux/Android only, from `/proc`, ±10 ms) is exec plus dynamic
  linking. A large number here means loader and page-in cost, not application
  code. `time notema log "x"` minus the reported `total` measures the same thing
  more precisely.
- **`store:ensure-*`** is the first thing to touch the journal root. If these
  dominate, the journal filesystem is the bottleneck — on Android that means
  `/storage/emulated/0`, where every syscall is a FUSE round-trip.
- **`roster:verify (N ops)`** scales with `N`, the append-only roster length, not
  the device count; every op costs a signature check.
- **`theme:bg-query`**, **`term:kbd-enhance-query`** and **`image:picker-query`**
  are blocking terminal round-trips before the first frame, with 1–2 s timeouts.
  A large value is the terminal emulator not answering, not Rust.
- **`cache read:`** is the decode alone, before the journal tree is walked. The
  **`library:`** line comes later, from `LibraryLoadReport`, once background
  validation has reconciled the cache against the source tree — it is the one
  that carries the real hit and miss counts, split across discovery walk, source
  read and cache write.
- **`cache misses by cause:`** (level 2) attributes every miss to one of `len`,
  `mtime`, `ctime`, `journal` (the entry moved between journals), `absent` (no
  cached record) or `rebuild` (the policy forced a reload). A large `ctime`
  bucket means something is touching inodes without changing content.
- **`cache mtime precision:`** (level 2) counts how many stamps carry a
  sub-second mtime. A count of zero means the filesystem resolves mtime no finer
  than a second, which limits what the cache can tell apart.
