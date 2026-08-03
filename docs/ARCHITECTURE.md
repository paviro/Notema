# Architecture

Notema is a Cargo workspace. Reusable logic lives in `notema-*` library crates; the
application is the root package.

## Root application

The root crate keeps platform and delivery concerns separate from the reusable
workspace crates:

| Module | Owns |
|---|---|
| `cli` | Clap parsing, routing, log creation, imports, encryption commands, prompts |
| `config` | Persisted config and UI-state models, load/save behavior |
| `startup` | First-run setup, store preparation, cache progress |
| `platform` | Device naming and iSH adapters |
| `licenses` | `notema licenses`: data-source attributions plus the dependency license report embedded at build time |
| `tui` | Terminal runtime, model, events, rendering, theme, and feature state |

`notema::run` is the root library entry point. Command syntax, persisted
formats, and workspace-crate APIs are independent of the module layout.

The TUI lives inside the root crate:

| TUI module | Owns |
|---|---|
| `app` | `AppModel`, `Services`, appearance, library and render-cache aggregates |
| `features` | Browser, reader, editor, search, metadata, location/environment, insights, settings, overlay, and image behavior |
| `events` | Semantic actions, input translation, dispatch, handlers, effects |
| `runtime` | Terminal lifecycle, scheduling, redraw, watchers, workers, effect execution |
| `ui` | Frame-local view state, ordered interaction regions, shared UI context |
| `render` | Feature views and shared terminal widgets |
| `theme` | Owned theme values, schema, loading, resolution, semantic accessors |
| shared leaves | `state`, `surface`, `scroll`, `hit_test`, `text_input`, `editor_state`, `editor_highlight`, `syntax_highlight`, `entry_rows`, `env_strip`, `environment`, `geocode`, `search`, `errors`, `image`, `clipboard`, `paste` (plus cfg-gated `bench_support`/`test_support`) |

## Workspace crates

| Package | Owns | Must not own |
|---|---|---|
| `notema-domain` | Entry values, validated coordinates, feelings, search value types, Markdown link parsing | Filesystem access, network access, terminal types |
| `notema-analytics` | Pure aggregation over borrowed domain entries | I/O, clocks, rendering |
| `notema-http` | The shared blocking HTTP agent, its timeouts, and iSH detection | URLs, payload shapes, retry policy |
| `notema-timing` | Opt-in process timing behind `NOTEMA_TIMING` (`1` phase timings, `2` cache diagnostics): the level/origin/main-thread globals, stderr emission, the `defer` buffer holding output while the alternate screen is up, and Linux/Android `/proc` process-age probing | Anything the app branches on — it is write-only diagnostics |
| `notema-context` | Geocoding, weather, air quality, celestial calculations, device location | Storage or TUI state |
| `notema-import` | Parsing and normalizing external export formats | Journal creation, dedup policy, writes |
| `notema-encryption` | Keys, recipients, signed roster, ciphertext and identity formats, the atomic-write primitives | Journal paths and entry layout |
| `notema-storage` | Journal layout, entry codecs, assets, encryption orchestration | CLI prompts or terminal UI |
| `notema-fuse` | libfuse adapter and mount path policy | Entry parsing and business rules |
| `notema-seed` | Development corpus generation | Production command surface |
| `notema-locate` | macOS CoreLocation helper executable | Application state |
| root `notema` | CLI use cases, config/state, TUI, rendering, workers | Reusable domain or storage primitives |

Dependencies point toward `notema-domain`. `notema-storage` may use
`notema-encryption`; the application composes storage, import, context, and
analytics. Import and context never depend on storage in their production
graph (test-only `dev-dependencies` on storage, for round-trip fixtures, are
exempt). FUSE reaches storage through its public facade.

Network access is a capability, not a layer: `notema-context` (geocoding,
weather, air quality) and `notema-storage` (remote asset download) are the only
crates that reach the network, and every request goes through `notema-http`,
which owns the agent but decides nothing about what is fetched. Storage also
opens a plain `TcpStream` to check whether a host is up before downloading from
it, so an import full of links to a dead server fails fast. Every other crate —
domain, analytics, encryption, import, fuse — must not open sockets.

Errors follow the anyhow + thiserror split: the domain crates expose typed
`thiserror` enums, the application uses `anyhow` at its edges (`bail!`,
`.context()`, clean cause chains from `main`), and cross-crate handlers that
need to branch downcast the typed error back out of an `anyhow::Error` (e.g.
storage degrading an entry to a locked placeholder on an `EncryptionError`).
That downcast is a real contract: a crate changing its error type can break a
caller silently.

## Application flow

`AppModel` aggregates feature state. `Services` owns the config path, config,
and journal store used by synchronous application operations. Appearance owns
the resolved theme and warning-deduplication state.

Keyboard and mouse handlers translate input into `Action` values. Only
`dispatch_action` mutates application model state. Browser, search, editor,
metadata, location, settings, images, overlays, reader, and insights each have
sub-actions and a handler. Worker, watcher, timer, and effect
completions return as background actions. `DispatchOutcome` carries loop
control, redraw intent, and typed effects. Geocoding, environment fetches, image
preparation, and OS launches start only when the runtime executes those effects.

`RenderContext` borrows the active owned theme and frame-local `ViewState`.
Rendering records effective scroll offsets in that view state; the runtime then
dispatches `ViewRendered` to apply them. Rendering never changes lasting
navigation intent or starts background work. The ordered interaction map records
panels, rows, text fields, hints, links, images, scrollbars (with their track
metrics), dialog lists, buttons, and overlays. Mouse translation reads this map
instead of reconstructing layout; only sub-region probes inside a hit panel —
insights tabs, reader metadata chips — and wheel routing stay geometric. Later
regions win, and every frame begins by clearing stale regions.

Panel focus is separate from row selection, and Reader and Insights scroll
independently of it. Render caches are keyed on the data, width, and theme
inputs that can invalidate them.

Dialogs share the action dispatcher. UI elements are clickable, but hover only
highlights that a row is clickable and never commits it.

## Persistence

Every persisted TOML document carries `schema_version = 1`, and an unsupported
version fails rather than being guessed. The `.journal.toml` sidecar is the
exception: it keeps the journal id, reports no theme, and leaves the file as it
found it.

Device-local documents live in the config directory: config and state, themes,
the iSH store binding (`ish-store.toml`, holding the store id this device
confirmed), and this device's encryption identity and roster trust pins. The
rest sit in the journal folder and sync: entry front matter, the per-journal
`.journal.toml` sidecar, the `.notema-store.toml` store marker (a random 32-hex
id for one journal root), and the signed roster plus pending join requests under
`.age/`, whose sub-layout `notema-encryption` owns. Only the front matter is
encrypted with its entry; the synced TOML stays plaintext even under encryption.
This is a v1: pre-release data predating the version field is unsupported, with
no migrations.

Entries and persisted documents are fsynced before a write reports success, in
one of two ways. Edits go through the shared write-fsync-rename-fsync
primitive (`notema_encryption::atomic_write`, `atomic_write_private`,
`atomic_write_with`), so a crash can't truncate a good file. New entries and
assets take their final name straight away with `create_new`, which doubles as
the id reservation, then fsync the file and its parent directory. Duplicating an
entry is the exception: it copies the stored assets with a plain `fs::copy`.

Malformed entry front matter does not hide the body. The entry stays readable
with a warning, and the TUI blocks editing it (every metadata edit would touch
the unparseable front matter); storage still permits a byte-preserving
body-only write. An edit only rewrites the fields Notema owns; any other keys in
the front matter, including ones nested inside known tables, are left as they
were.

Config and theme files reject unknown keys: they are hand-edited, so a typo
should fail loudly rather than be silently ignored. Invalid machine-written
state is renamed aside and recreated.

## Platform code

Linux, Android, and macOS location providers are selected at compile time.
`notema-locate` ships only on macOS: a bare CLI binary can't obtain CoreLocation
authorization there, so the location code lives in a separate helper that ships
wrapped in a signed `.app`.

FUSE is an optional feature requiring libfuse3 headers and libraries; the
standard binary has no FUSE dependency. Unsafe Rust lives in exactly two places:
the C callback boundary in `notema-fuse` and the Objective-C bindings in
`notema-locate`. Every other crate carries `#![forbid(unsafe_code)]`, and the
workspace lints deny `unsafe_op_in_unsafe_fn` and undocumented unsafe blocks
everywhere.
