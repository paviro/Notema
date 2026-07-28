# Usage

```bash
notema                       # launch the TUI
notema log "Had a good day"  # quick entry from the command line
notema log                   # compose an entry in the fullscreen editor
echo "note" | notema log     # entry from stdin
notema use personal          # set the default journal for new entries
```

Run `notema` with no arguments the first time to walk through setup: pick a journal
root (default `~/Journals`) and optionally enable encryption. Config is written to
`~/.config/notema/config.toml` (`~/Library/Application Support/de.paviro.notema/`
on macOS), overridable with `$XDG_CONFIG_HOME` or `--config <DIR>`.

## Logging with metadata

```bash
notema log "Shipped the release" \
  --journal work \
  --tag project,milestone \
  --person Alice --activity coding \
  --feeling proud,focused \
  --mood 3 \
  --location "Berlin"
```

`--feeling` takes values from a fixed vocabulary — see
[STORAGE-FORMAT.md](STORAGE-FORMAT.md#feelings-vocabulary).

`--location` sets where the entry was written. A bare `--location` grabs the
device's current GPS fix; a value is treated as `lat,lon` when it parses as
coordinates, otherwise as an address to geocode. Whenever a location resolves, the
weather, air quality, and celestial data for that place and time are captured with
it.

## Search filters

`/` searches the whole corpus — body text and metadata — ranked by how well each
entry matches. A `prefix:` narrows it to one field:

```
tags:project        people:Alice        activities:coding
feelings:proud      location:Berlin     mood:3            star:true
```

Dates are written year-first, so each component you add narrows the range:

```
date:2026           the whole year
date:2026-07        that month
date:2026-07-25     that day
date:today          date:yesterday
date:7d             7 days ago — also 2w, 3m, 1y
before:2026-07      after:2026-07
```

`*` leaves a component open, which is how you ask for a date across years:

```
date:*-07-25        every 25 July
date:2026-*-25      the 25th of every month in 2026
```

Because an open component recurs, it has no single span for `before:`/`after:` to
compare against — those two take a fixed date.

Chain filters with `;` to require all of them. `tags:`, `people:`, `activities:`,
and `feelings:` take several values: `+` requires all of them and `|` accepts any.
`+` binds looser, so `a+b|c` means a and (b or c):

```
tags:work+urgent; people:alice     both tags, and Alice
tags:berlin|paris                  either tag
after:2025-01; before:2025-06      a bounded range
beach; tags:travel                 full-text "beach" and the travel tag
```

A prefix-less piece is a full-text search AND-ed with the filters, so it also
supplies the ranking. Only `;`, `+`, and `|` are structural, and only where the
prefix takes them — values keep everything else (`location:Berlin, Germany`).

`tags:`, `people:`, `activities:` and `feelings:` match a bare value anywhere in
the field, which is what keeps the list narrowing as you type. Quote it to mean
that value and nothing else, and to carry a `;`, `+`, or `|` literally:

```
tags:app            app, apple, and pineapple
tags:"app"          the tag app
tags:"C++"          the tag C++, not "C" and "+"
tags:"Berlin; Mitte"
tags:"say ""hi"""   a quote inside a quoted value is written twice
```

`location:` is not exact either way: a place contains the places around it, so
`location:Germany` finds every entry in it. It takes `|` but not `+` — an entry
has one location, so `location:Berlin|Paris` is either city and a `+` is just a
character in a place name:

```
location:Berlin|Paris     entries in either city
location:Rock + Roll      the place "Rock + Roll"
```

Searches launched by clicking a tag, person, feeling, or a filter-browser row
(`b`) quote themselves, so the query they drop in the search box means the value
you clicked and only that one. A launched `location:` does not — quoting would
not narrow it — so it arrives as plain text (`location:Berlin, Germany`), quoted
only if the place name itself holds a `;` or a `|`.

### Suggestions

Finishing a prefix lists that facet's values under the query box, most-used
first, with the number of entries each returns. The list narrows as you type the
value, and it writes the value quoted for you, so you never have to spell one out
or remember which needs quoting.

| Key | |
|---|---|
| `Tab` | insert the first value — or the highlighted one |
| `↓` | move into the list; `↑` off the top leaves it again |
| `Enter` | insert the highlighted value |
| `Esc` | put the list away for the value you are typing |

Nothing is highlighted until you press `↓`, so `Enter` keeps opening the selected
result and a query typed to narrow by substring behaves as it always did. After a
value is inserted, type `+`, `|` or `;` to add another.

## Location

Open the location dialog on an entry (`l`) to set where it was written: type a
place name, an address, or `lat, lon`. Addresses and coordinates are geocoded
through OpenStreetMap Nominatim. Once an entry has coordinates, Notema also fetches
the weather and air quality for that place and time from Open-Meteo.

Press `Ctrl+L` to grab the device's **current** location, reverse-geocoded to an
address. The provider is platform-specific and only ever produces
`latitude`/`longitude` — no IP-based fallback:

- **Android / Termux** — `termux-location`. Install the Termux:API app and
  `pkg install termux-api`.
- **iOS / iSH** — the `/dev/location` GPS stream. iSH prompts for Location access
  on first use; if you denied it, re-enable it under iOS Settings › Privacy ›
  Location Services › iSH.
- **Linux** — GeoClue2 over D-Bus. Its default Wi-Fi backend (Mozilla Location
  Service) was retired in 2024, so a machine with no GPS and no reconfigured
  backend (e.g. BeaconDB) reports "no location".
- **macOS** — CoreLocation. Since Ventura a bare CLI can't get location, so Notema
  extracts a small signed helper app to `~/Library/Application
  Support/de.paviro.notema/` on first use and reads the fix from it. Grant
  Location access when prompted, or later in System Settings → Privacy & Security →
  Location Services.

## Backfilling old entries

New entries capture their environment as they're written, but entries logged
before a place was known — or imported with only `lat,lon` — may be missing their
address, weather, air quality, or celestial data. Fill those in on demand:

```bash
notema backfill
```

It walks every located entry and fetches only what's missing, one request per
second (OpenStreetMap Nominatim asks that bulk querying not be automated, so this
is a manual command rather than a background sweep). Data already present is never
overwritten, and entries without coordinates are skipped. It's safe to re-run — a
second pass reports anything already complete and makes no further requests for it.

## Themes

`,` opens settings, then `t` opens the theme picker. Up/Down previews live, `b` cycles
chrome (default → flat → bordered), `m` cycles color mode (auto → dark → light;
hidden on themes without variants), Enter applies and saves, Esc reverts. Broken
themes are listed but can't be applied. `Tab` switches between the global default and
the current journal's own theme; a journal theme saves the previewed color mode and
chrome with it. See [Themes](THEMES.md#per-journal-themes).

Edits to the **active** theme hot-reload (debounced ~400 ms). A broken edit shows
an error toast and keeps the current theme; the next valid save loads. Writing your
own themes is covered in [THEME-REFERENCE.md](THEME-REFERENCE.md); see
[THEMES.md](THEMES.md) for the bundled gallery.

## Images and attachments

Add an image or file to an entry in the editor by putting its path on its own line
— drag a file onto the terminal and most terminals (including macOS Terminal and
iTerm) paste its path — or by writing a Markdown link `[label](/path/to/file.pdf)`.

On save, the file is copied into the entry's own `<entry>.assets/` folder and the
reference is rewritten to point there; the original file is left untouched. In an
encrypted journal the copy is encrypted alongside the entry.

- Images appear in the reader as a numbered `[Image N …]` label — click it or press
  its number to open the fullscreen viewer.
- Other files (PDF, audio, video, …) become a link labelled by the file name. In an
  unencrypted journal, clicking the link opens the file in your OS default app. In an
  encrypted journal the file is stored encrypted and can't be handed to the OS, so the
  link is shown but not clickable for now — in-app viewing of more file types may come
  later.

## Import from Day One

```bash
notema import dayone ./export/personal.json --journal personal
notema import dayone ./export/personal.json --journal personal --download-images   # also fetch remote image links
```

Already-imported entries are skipped on re-run. Photos are imported inline; audio,
video, and PDF attachments are copied into the entry's asset folder and linked (see
[Images and attachments](#images-and-attachments)).
