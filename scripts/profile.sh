#!/usr/bin/env bash
# Sampling-profiler helpers for notema, built on samply. See docs/DEVELOPMENT.md
# ("Profiling with samply"). Traces land in target/profiling/prof-*.json.gz;
# open one interactively with `samply load target/profiling/prof-bench.json.gz`,
# or summarize it headlessly with `scripts/prof-top.py <trace>`.
#
# Usage: scripts/profile.sh {seed|startup|benches|scan|analytics|tui|all}
#
#   seed       seed a reproducible throwaway corpus (shared by every target)
#   startup    non-TUI launch cost via `notema log` (config/store/roster/encrypt)
#   benches    render/search/filter/pickers/reload/editor (tui bench)
#   scan       library load: walk + parse + preview + haystack (storage scan bench)
#   analytics  cadence / mood / correlation aggregation (analytics bench)
#   tui        launch the real TUI under samply for you to drive by hand, then
#              quit with `q` — samply writes the trace on that clean exit
#   all        every non-interactive target (startup, benches, scan, analytics)
#
# `all` skips `tui` on purpose: samply only writes the trace when its child exits
# cleanly, and the TUI exits on your `q`, so a live trace needs a human at the
# keyboard. The bench targets drive the same render/search/filter/editor code
# through the `bench` feature, which is how the project exercises those paths
# without a terminal.
#
# Env overrides:
#   NOTEMA_PROF_DIR   throwaway corpus dir            (default /tmp/notema-prof)
#   NOTEMA_PROF_COUNT seeded entry count              (default 25000)
#   NOTEMA_PROF_SEED  seed for a reproducible corpus  (default 42)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PROF_DIR="${NOTEMA_PROF_DIR:-/tmp/notema-prof}"
CORPUS_COUNT="${NOTEMA_PROF_COUNT:-25000}"
CORPUS_SEED="${NOTEMA_PROF_SEED:-42}"
OUT_DIR="$ROOT/target/profiling"

need() { command -v "$1" >/dev/null 2>&1 || { echo "missing required tool: $1" >&2; exit 1; }; }

seed() {
    if [ -d "$PROF_DIR/journals" ]; then
        echo "corpus already present at $PROF_DIR (rm -rf it to reseed)" >&2
        return
    fi
    echo "seeding $CORPUS_COUNT entries into $PROF_DIR (seed=$CORPUS_SEED)" >&2
    cargo run -p notema-seed -- \
        --root "$PROF_DIR/journals" --config-dir "$PROF_DIR" \
        --count "$CORPUS_COUNT" --seed "$CORPUS_SEED"
    # The seed tool writes keys and journals but no config.toml; the app needs
    # one pointing at the journal root.
    cat >"$PROF_DIR/config.toml" <<EOF
schema_version = 1

[journal]
path = "$PROF_DIR/journals"
EOF
}

# Newest executable bench binary for a given `[[bench]]` name, minus the .d/.dSYM
# siblings. $1 = bench name, $2 = crate spec (-p flag) or empty for the root.
bench_bin() {
    local name="$1" pkg="${2:-}"
    if [ -n "$pkg" ]; then
        cargo build -p "$pkg" --profile profiling --bench "$name" >&2
    else
        cargo build --features bench --profile profiling --bench "$name" >&2
    fi
    find target/profiling/deps -maxdepth 1 -name "${name}-*" -type f -perm -111 \
        ! -name '*.dSYM' | sort | tail -n1
}

record_bench() { # $1 = out suffix, $2 = bench name, $3 = crate (optional)
    need samply
    local bin
    bin="$(bench_bin "$2" "${3:-}")"
    echo "profiling $2 bench: $bin" >&2
    samply record --save-only -o "$OUT_DIR/prof-$1.json.gz" -- "$bin"
    echo "wrote $OUT_DIR/prof-$1.json.gz" >&2
}

startup() {
    need samply
    cargo build --profile profiling >&2
    seed
    # The `log` subcommand is non-TUI, so no pty is needed. This covers config
    # load, store ensure, roster verify and reconcile-encryption; the library
    # walk is measured by `scan`. NOTEMA_TIMING=2 prints the phase timeline
    # alongside, to line the samples up against. Feed the entry body on stdin (a
    # positional body errors when stdin is not a tty, as under a script).
    printf 'profiling probe\n' | NOTEMA_CONFIG="$PROF_DIR" NOTEMA_TIMING=2 \
        samply record --save-only -o "$OUT_DIR/prof-startup.json.gz" -- \
        target/profiling/notema --config "$PROF_DIR" log --journal Sample
    echo "wrote $OUT_DIR/prof-startup.json.gz" >&2
}

# render/search/filter/pickers/reload/editor over 1k/10k/25k; the 25k
# wide-corpus lines dominate and give ample samples.
benches() { record_bench bench tui; }

# Library load: the full journal walk + markdown parse + preview + haystack.
# This is the CPU behind the TUI's `library:` startup phase, measured cleanly
# without a terminal.
scan() { record_bench scan scan notema-storage; }

# Cadence / mood / emotion / correlation aggregation over the corpus.
analytics() { record_bench analytics analytics notema-analytics; }

tui() {
    need samply
    cargo build --profile profiling >&2
    seed
    # Interactive: samply writes the trace only when its child exits cleanly, and
    # the TUI exits on your `q`. Run this in a real terminal, use the app for a
    # bit (open the filter browser with `b`, scroll, edit), then press `q`.
    echo "Drive the app, then press q to quit; samply writes the trace on exit." >&2
    NOTEMA_CONFIG="$PROF_DIR" \
        samply record --save-only -o "$OUT_DIR/prof-tui.json.gz" -- \
        target/profiling/notema --config "$PROF_DIR"
    echo "wrote $OUT_DIR/prof-tui.json.gz" >&2
}

case "${1:-}" in
    seed) seed ;;
    startup) startup ;;
    benches) benches ;;
    scan) scan ;;
    analytics) analytics ;;
    tui) tui ;;
    all) startup; benches; scan; analytics ;;
    *) echo "usage: $0 {seed|startup|benches|scan|analytics|tui|all}" >&2; exit 2 ;;
esac
