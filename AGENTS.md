# Agent Notes

This is a terminal-first markdown journal app. Treat terminal readability as a core product requirement, not a polish pass.

## Product map

Where the major areas live. The rest of this file is TUI-focused; these exist too.

- CLI — `src/cli/mod.rs` (`log`, `use`, `import`, `backfill`, `encryption`, `licenses`, `mount`).
- Encryption (age + device keystore) — `crates/notema-encryption`, `src/cli/encryption.rs`.
- FUSE mount — `crates/notema-fuse`, `notema mount` (feature-gated `fuse`).
- In-TUI markdown editor — `src/tui/features/editor.rs`.
- Insights / analytics — `crates/notema-analytics`, `src/tui/features/insights.rs`.
- Entry metadata & location/weather/celestial — `src/tui/features/metadata.rs`, `src/tui/features/location.rs`, `crates/notema-context`, `crates/notema-locate`; `notema backfill` fills these in.
- Day One import — `crates/notema-import`, `src/cli/import.rs`.
- Themes & hot reload — `src/tui/theme/`, watched and dispatched from `src/tui/runtime/watcher.rs` and `src/tui/runtime/mod.rs`.
- Image viewer / terminal graphics — `src/tui/image/`.

## UI Intent

- The app should work well on both light and dark terminal themes.
- E-ink and monochrome screens are a supported use case.
- Do not rely on subtle color differences to communicate focus, selection, hierarchy, or state.
- Prefer structural cues that survive monochrome rendering: border shape, clear text markers, reversed selection, spacing, labels, and stable layout.
- When using color, keep terminal-default foregrounds where practical so the user's terminal theme controls contrast.
- Avoid hardcoded white, black, bright, or dim-only text for essential content.

## TUI Rendering

- Focused panels should be obvious without color. Keep the thick focused border and visible title marker behavior unless replacing it with an equally clear monochrome cue.
- Journals, entries/search, and the Reader should have consistent focused panel treatment. The right-hand column is two-mode: the Reader and the tabbed Insights panel share it (four focus targets total — `Journals`, `Entries`, `Reader`, `Insights`).
- Markdown content should remain readable on white and dark backgrounds.
- Selection inside the focused list should remain distinct from panel focus.

## Mouse Navigation

- Mouse clicks on a panel should make that panel active even when the click is on empty space or a non-item row such as an entry date header.
- Clicking a journal or entry row should select that row. In Browse mode, clicking empty space in the entries list deselects the current entry and reveals the Insights panel (tabbed: Overview / Writing / Feelings / Drivers); in Search mode the selection stays unchanged. Clicking empty space in the journals panel keeps the current journal selection.
- Keep selection visible as a left-to-right trail: Journals stays selected while focus moves to Entries or the Reader, and Entries stays selected while focus is in the Reader.
- When focus moves back to Journals, do not keep the entry row visually selected, even though the internal selected entry index may remain unchanged.
- Mouse wheel events should scroll only the panel under the cursor and must not change row selection.
- Wheel scrolling over the Reader or Insights should also make that panel active. Wheel scrolling over Journals or Entries should not change the active panel.
- Dialogs are mouse-driven like the rest of the UI, through the same interaction map and dispatcher (`src/tui/events/mouse/overlay.rs`): rows, focus, scrollbar drags, wheel, and text selection in dialog inputs. Hover only highlights; the click commits.

## Event Dispatch

- All state mutations must go through `dispatch_action` in `src/tui/events/mod.rs`, which applies each `Action` via the `apply_action` match in the same file; heavier per-action logic lives in helper functions in `src/tui/events/actions.rs`. When adding a new action, add an `Action` variant in `src/tui/events/action.rs` and handle it in the `apply_action` match — never implement the logic inline in a specific input handler.
- The keyboard path (`handle_key` in `src/tui/events/keyboard.rs`) is the reference: every key maps to an `Action`, then calls `dispatch_action`. The exception is the in-TUI editor — while it is open, `handle_key` short-circuits to `handle_editor_key` and bypasses the `Action` enum. Mouse handlers (`handle_mouse`, `handle_scroll` in `src/tui/events/mouse.rs`) follow the reference pattern — translate the gesture to an `Action` and return or dispatch it; do not reimplement toggle, navigation, or selection logic locally.
- The pure mapping helpers (`mouse_to_action`, `apply_mouse_action`, and the `mouse/overlay.rs` helpers) intentionally take no `terminal` and return an `Action`/`MouseAction` instead of mutating — the dispatch happens later at the `dispatch_action` boundary. Keep that split; don't fall back to a direct state mutation in a helper.

## Writing Style (README, docs, comments)

- Write plainly, like a person — not like a tool narrating what it did. Cut filler lead-in sentences (e.g. "journal credits the external data it fetches:"); a bare list or the fact itself is usually enough.
- Don't over-explain or justify. State what's true and stop. No "this is why", no restating the obvious, no tutorial framing for things the reader can see.
- Comments: only when they add information the code can't. Never narrate refactor history or what code replaces.
- Be precise, not padded. "Weather data from Open-Meteo (CC BY 4.0)" beats a paragraph about attribution obligations.

## Validation

- Run `cargo fmt` after Rust changes.
- Run `cargo clippy --workspace --all-targets --features bench` after Rust changes.
  Without `--features bench` the `tui` bench target is skipped and its support
  module goes unlinted.
- Run `cargo clippy -p notema --features fuse -- -D warnings` after Rust changes.
  The FUSE code path is behind the `fuse` feature and is otherwise unlinted.
- Run `cargo test --workspace` after behavioral or rendering-related changes.
- Run `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` after changing doc comments. CI denies rustdoc warnings, and a link from public documentation to a `pub(crate)` item is an error there while `cargo build` stays silent.
- CI runs clippy/test/doc with `--locked`, so it fails if a run would change `Cargo.lock`. Add `--locked` to match CI exactly before pushing.
- The age CLI interoperability test uses the `age` and `age-keygen` CLIs when available; if those tools are missing, the test skips itself.
- When changing colors or focus styling, scan for hardcoded foreground colors in `src/tui` and justify any that remain.

## Git

- Always run `cargo fmt` before committing.
- Before choosing a commit message, inspect the recent history and match its
  convention.
- Always write commit messages in the form `type(scope): description`, or
  `type(scope)!: description` for a breaking change. Examples:
  `refactor(storage): …`, `feat(api)!: …`. The scope is required — name the crate
  or area touched.
