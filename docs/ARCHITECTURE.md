# Architecture

Notema is a Cargo workspace. Reusable logic lives in `notema-*` library crates; the
application is the root package.

## Workspace crates

| Package | Owns | Must not own |
|---|---|---|
| `notema-domain` | Entry values, validated coordinates, feelings, search value types, Markdown link parsing | Filesystem access, network access, terminal types |
| `notema-analytics` | Pure aggregation over borrowed domain entries | I/O, clocks, rendering |
| `notema-context` | Geocoding, weather, air quality, celestial calculations, device location | Storage or TUI state |
| `notema-import` | Parsing and normalizing external export formats | Journal creation, dedup policy, writes |
| `notema-encryption` | Keys, recipients, signed roster, ciphertext and identity formats | Journal paths and entry layout |
| `notema-storage` | Journal layout, entry codecs, assets, atomic writes, encryption orchestration | CLI prompts or terminal UI |
| `notema-fuse` | libfuse adapter and mount path policy | Entry parsing and business rules |
| `notema-seed` | Development corpus generation | Production command surface |
| `notema-locate` | macOS CoreLocation helper executable | Application state |
| root `notema` | CLI use cases, config/state, TUI, rendering, workers | Reusable domain or storage primitives |

Dependencies point toward `notema-domain`. `notema-storage` may use
`notema-encryption`; the application composes storage, import, context, and
analytics. Import and context never depend on storage. FUSE reaches storage
through its public facade.

## Application flow

Keyboard and mouse handlers translate input into `Action` values. Only
`dispatch_action` mutates application state; the feature reducers it calls may
mutate, but input translation and rendering may not. `DispatchOutcome` is the
event loop control result.

Panel focus is separate from row selection, and Reader and Insights scroll
independently of it. Render caches are keyed on the data, width, and theme
inputs that can invalidate them.

Dialogs share the action dispatcher. UI elements are clickable, but hover only
highlights that a row is clickable and never commits it.

## Persistence

Every persisted TOML document carries `schema_version = 1`. Unsupported versions
fail rather than being guessed.

Malformed entry front matter does not hide the body. The entry stays readable
with a warning, but editing it is blocked, since every edit touches metadata.
An edit only rewrites the fields Notema owns; any other keys in the front
matter, including ones nested inside known tables, are left as they were.

Config and theme files reject unknown keys: they are hand-edited, so a typo
should fail loudly rather than be silently ignored. Invalid machine-written
state is renamed aside and recreated.

## Platform code

Linux, Android, and macOS location providers are selected at compile time.
`notema-locate` ships only on macOS: a bare CLI binary can't obtain CoreLocation
authorization there, so the location code lives in a separate helper that ships
wrapped in a signed `.app`.

FUSE is an optional feature requiring libfuse3 headers and libraries; the
standard binary has no FUSE dependency. Its unsafe Rust is confined to the C
callback boundary in `notema-fuse`; the rest of the workspace forbids unsafe
code.
