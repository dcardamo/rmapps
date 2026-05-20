# rmbujo — Design Spec

**Date:** 2026-05-20
**Status:** Approved for planning
**Author:** Dan (with Claude)

## Summary

`rmbujo` is a Python CLI that generates a year's worth of dot-grid bullet-journal
PDFs sized for reMarkable devices — primarily the **Paper Pro Move** (the only
tested target), with the larger **Paper Pro** also selectable. It produces one PDF
per "notebook," written into a flat per-year folder, driven entirely by a per-year
YAML config. Pages are authored as **Jinja2 HTML templates + CSS** and rendered to PDF
via **Playwright (headless Chromium)**. Output is deterministic so a later phase can
refresh page backgrounds on-device without disturbing the user's handwriting.

The tool is open source: well documented, commented, and easy to extend (pluggable
ICS sources, themes, and deploy backends).

### Phasing

- **Phase 1 (this spec):** the PDF generator + config/wizard workflow. Writes PDFs
  to disk. No device integration, no calendar data rendering.
- **Phase 2 (deferred, architected-for):** ICS subscriptions (multiple) baked into the
  day-list / future-log backgrounds — including holidays, which are simply one ICS
  feed the user can add. Plus a deploy/re-sync step via `rmapi` (reMarkable cloud).
  Designed for now via deterministic page ordering, a reserved event gutter, an `ics:`
  config section, and a deploy seam — but **not implemented** in Phase 1.

> Note: the git repo directory is currently `~/git/rppmbujo`. The Python package,
> CLI, and project name are `rmbujo`. The repo directory will be renamed separately,
> later — out of scope for this spec.

## Goals

- Generate a complete bullet-journal year for the Paper Pro Move from a single config.
- Look good on a color e-ink screen: dot grid, deep/legible colors, no black fills.
- "Set it once, re-run later": a config file captures all settings; re-running points
  at that file and regenerates with identical settings.
- Be trivially repeatable for future years.
- Be a clean open-source codebase: small, focused modules; pluggable extension points.

## Non-goals (Phase 1)

- No calendar/ICS rendering — including no holidays. Holidays arrive in Phase 2 as a
  user-supplied ICS feed, not a built-in.
- No device upload / cloud sync (Phase 2).
- No native reMarkable `.rm` file writing (rejected — see Rationale).
- No GUI.

## Device geometry

PDFs are vector, so the device scales them cleanly. We match the screen aspect ratio
to avoid letterboxing and use physical inches so the 5 mm dot grid is true-to-size.

| Device           | Pixels    | PPI | Page (portrait) | Points (w × h) |
|------------------|-----------|-----|-----------------|----------------|
| `paper-pro-move` | 1696×954  | 264 | 3.61″ × 6.42″   | 260 × 462      |
| `paper-pro`      | 2160×1620 | 229 | 7.07″ × 9.43″   | 509 × 679      |

Default and only tested target: `paper-pro-move`. All page size, margin, and dot-grid
math derive from the selected device config plus the theme.

## Notebooks and page layouts

All files are flat inside the year output folder (the folder containing `rmbujo.yaml`).

### Filenames

| Notebook            | Filename pattern              | Example                       |
|---------------------|-------------------------------|-------------------------------|
| Future Log          | `YYYY Future Log.pdf`         | `2026 Future Log.pdf`         |
| Month (×12)         | `YYYY.MM <Month>.pdf`         | `2026.05 May.pdf`             |
| Collection Template | `YYYY Collection Template.pdf`| `2026 Collection Template.pdf`|
| Reference           | `YYYY Reference.pdf`          | `2026 Reference.pdf`          |
| Config              | `rmbujo.yaml`                 | `rmbujo.yaml`                 |

Sort order on device: the `YYYY <name>` files (Collection Template, Future Log,
Reference) sort before the `YYYY.MM` month files.

### Future Log — `YYYY Future Log.pdf`

- Cover page (see Cover spec).
- 4 content pages, **3 months stacked per page** (single-page device — no spreads).
- Each month block: month-name header (navy) + a freeform dot-grid area for "big
  things." Not day-numbered.

### Month — `YYYY.MM <Month>.pdf`

1. **Day list (page 1).** "<Month> YYYY" header, then every day of the month as
   `8 Mon` — day number in black, weekday abbreviation in navy. Row height auto-fits
   so all days fit on one page on the Move (roomy on the Pro). Weekday computed from
   the real calendar for the given year. Days are visually grouped into weeks by a
   subtle extra gap (and a faint rule) before each week's first day; the week boundary
   is determined by `week_start` (Sunday by default, Monday optional) — this is the one
   place `week_start` takes effect. Each row reserves a right-hand gutter for ICS
   events (populated in Phase 2; empty in Phase 1).
2. **Tasks (page 2).** "Tasks" header + dot grid.
3. **Daily log (pages 3…N).** Full dot grid, no date printed (the user writes it).
   Default `daily_pages = 60`.

ASCII sketch of the Move day-list page (Phase 1):

```
┌──────────────────────────┐
│ May 2026                  │  ← navy header
│                           │
│  8  Mon                   │  ← day black, weekday navy
│  9  Tue                   │
│ ──────────────────────    │  ← faint rule at week start (week_start)
│ 10  Sun                   │
│ ...        └─ ICS zone ─┘ │  ← right gutter reserved (empty in Phase 1)
│ 31  Sun                   │
└──────────────────────────┘
```

### Collection Template — `YYYY Collection Template.pdf`

A single template the user duplicates on-device for each new collection.

- Decorated cover with a **blank title area** (a labeled space / underline) where the
  user hand-writes the collection name after duplicating.
- `collection_pages` dot-grid pages (default 20).

### Reference — `YYYY Reference.pdf`

- Cover page.
- Concise reference content (2–3 pages):
  - **Bujo key / legend:** `•` task, `×` task complete, `>` migrated, `<` scheduled,
    `○` event, `—` note, `★` priority, `=` feeling / mood.
  - **How to start a month** (set up the day list + tasks, migrate forward).
  - **How to end a month / migration** (review, migrate unfinished tasks).

> The `=` feeling/mood signifier is taken from the official Bullet Journal channel
> video "Write Your Feelings Down" (https://www.youtube.com/watch?v=hrGEFqIE13k).
> It is not in the original printed key; described here as the author uses it.

## Color and type

Palette **"Library"** lives in `themes/library.yaml`:

| Role       | Hex       | Name  |
|------------|-----------|-------|
| Primary    | `#1B365D` | Navy  |
| Event      | `#8B2E1F` | Brick |
| Accent 1   | `#A07E1C` | Ochre |
| Accent 2   | `#556B2F` | Olive |
| Rule       | `#D9D6CC` | Rule  |
| Dot        | `#CFCDC4` | Dot   |

No black fills (poor for color e-ink). Brick is reserved for all-day ICS event pills
(e.g. a holidays feed) in Phase 2. The theme YAML is emitted as CSS custom properties
(`:root { --navy: #1B365D; ... }`), so re-skinning is editing the YAML. A font is
bundled and loaded via `@font-face` (local TTF) for consistent, portable typography; a
theme may point to a different font file.

## Invocation model

```
rmbujo new                    # interactive wizard → creates year folder + config → builds
rmbujo path/to/rmbujo.yaml    # regenerate from an existing config (Phase 2: also re-syncs)
```

- **`rmbujo new`** runs a wizard, creates `<base>/<year>/`, writes
  `<base>/<year>/rmbujo.yaml`, then generates the PDFs into that folder.
- **`rmbujo <config.yaml>`** loads the config and regenerates with identical settings,
  no prompts. In Phase 2 the same command re-syncs via the configured deploy backend.

The config file lives **inside the year folder**, so a year is self-contained and
movable. The config's own directory **is** the output directory. The Phase 2 deploy
step uploads only `*.pdf`, so the yaml never syncs to the device.

### Wizard questions (each prefilled with the default)

Year (default: current year) → base directory (default: cwd) → device → week start →
daily pages → collection pages → theme. The Phase 1 wizard does not prompt for ICS
subscriptions or deploy settings; it writes `ics: []` and `deploy.backend: none`.
(Prompting for ICS feeds arrives with Phase 2 rendering; users may pre-populate the
`ics:` list by editing the yaml.)

### Config schema (`rmbujo.yaml`)

```yaml
# rmbujo — config for 2026
# regenerate / re-sync with:  rmbujo path/to/this/rmbujo.yaml
year: 2026
device: paper-pro-move        # paper-pro-move | paper-pro
week_start: sun               # sun | mon
daily_pages: 60
collection_pages: 20
theme: library                # bundled name, or a path to a theme yaml
ics: []                       # Phase 2 — subscriptions rendered onto day-list/future-log.
                              # Holidays are just another feed. Example:
                              # - name: Holidays
                              #   url: "https://example.com/canada-on-holidays.ics"
                              #   color: brick     # theme color name; all-day events → pills
                              # - name: Work
                              #   url: "https://example.com/work.ics"
                              #   color: navy
deploy:                       # written now, inert until Phase 2
  backend: none               # none | rmapi
  target_folder: "/2026"      # reMarkable cloud folder
```

## Code architecture

```
flake.nix · flake.lock · .envrc   # Nix dev env (direnv: `use flake`); pins Python + Chromium
pyproject.toml                    # package metadata + `rmbujo` console entry point
Makefile                          # common targets (test, build, update-goldens)
rmbujo/
  __main__.py                 # python -m rmbujo entry
  cli.py                      # dispatch: `new` → wizard; a path → load + build
  wizard.py                   # interactive prompts (injectable input helper) → Config + yaml
  config.py                   # Config dataclass; load(path) / dump(path) via pyyaml
  devices.py                  # device specs → page geometry (size, margins, dot spacing)
  geometry.py                 # dot-grid math + layout helpers (feed template context / CSS)
  theme.py                    # palette loader → CSS custom properties (Library default)
  calendar_data.py            # year → months → days/weekdays + week grouping (stdlib calendar)
  render.py                   # HTML → PDF via Playwright (headless Chromium) + metadata normalize
  notebooks/                  # build Jinja context + assemble multi-page HTML, then render → PDF
    future_log.py
    month.py
    collection.py
    reference.py
  build.py                    # orchestrate a year from a Config → write PDFs next to config
  templates/                  # Jinja2 HTML + CSS (the page designs)
    base.html.j2              # page shell: @page size, break-after rules
    styles.css.j2             # theme CSS variables + layout
    cover.html.j2
    future_log.html.j2
    month_index.html.j2       # the day-list page
    tasks.html.j2
    daily.html.j2
    dotgrid.html.j2
    reference.html.j2
    fonts/                    # bundled TTF(s) for @font-face
  deploy/                     # Phase-1 seam
    base.py                   # Deployer protocol (deploy, refresh)
    local.py                  # backend 'none': files on disk only (Phase 1)
    # rmapi.py                ← Phase 2
  # ics.py                    ← Phase 2: fetch + parse ICS feeds → per-day events
themes/
  library.yaml
tests/
```

Design notes:

- **Templates are pure data → HTML** (Jinja2 context in, HTML out) — independently
  testable without a browser by asserting on the rendered HTML string.
- **`render.py` is the only browser-touching module** — HTML in, PDF out (Playwright),
  isolating the Chromium dependency behind one seam.
- **Notebook builders** build the Jinja context, assemble a multi-page HTML doc (one
  `<div class="page">` per page, separated by CSS `break-after: page`), and call
  `render.py` to produce one PDF each.
- **Extension seams behind protocols / data:** theme YAML → CSS (colors/fonts),
  `Deployer` (deploy/refresh backends), and the `ics:` config list (Phase 2 ICS
  sources). Forks plug in without touching the core.

## Dependencies & development environment

All dependencies are managed with **Nix**. A `flake.nix` provides a reproducible dev
shell, and **direnv** (`.envrc` containing `use flake`) loads it automatically on `cd`
into the repo. There is no `pip install` or `playwright install` step. `pyproject.toml`
defines the package metadata and the `rmbujo` console entry point; the flake builds it
(`buildPythonApplication`) from the same nixpkgs package set, so the dev shell and the
built tool share one source of truth.

Provided by the flake (from nixpkgs, pinned via `flake.lock`):

Runtime:
- `python3` with `jinja2` (templating), `pyyaml` (config/theme), and the `playwright`
  Python package.
- `playwright-driver.browsers` — a pinned Chromium. The dev shell exports
  `PLAYWRIGHT_BROWSERS_PATH=${playwright-driver.browsers}` and
  `PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS=1` so the Python `playwright` package uses
  the Nix-provided browser. The `playwright` Python version is kept in lockstep with the
  driver (both from the same nixpkgs revision).

Dev / tests:
- `pypdf` — structural assertions + PDF metadata normalization.
- `pillow` + a pixel diff (`pixelmatch`) — visual-regression golden diffing.
- `pytest`.

Flake outputs:
- `devShells.default` — the dev shell (entered automatically via direnv, or `nix develop`).
- `packages.default` / `nix run` — the built `rmbujo` CLI.

(Phase 2 adds `icalendar` to the flake, and `rmapi` as a tool in the shell/package for
the cloud backend.)

## Error handling and determinism

- Validate year range, device name, and week-start; emit clear messages on bad input.
- Config: unknown keys and missing keys handled gracefully (clear errors or documented
  defaults).
- **Deterministic output:** Chromium stamps `/CreationDate` and `/Producer` into the
  PDF, so `render.py` normalizes PDF metadata to fixed values via pypdf after rendering.
  What Phase 2's `rmapi put --content-only` actually needs is **stable page geometry**
  (page order, count, and box size), which fixed templates + a fixed `@page` size
  guarantee regardless of timestamps. Building twice yields structurally identical PDFs.
- Regeneration overwrites existing output by default (regeneration is the point).
- Dynamic content (ICS, Phase 2) is confined to the day-list and future-log pages;
  daily/dot-grid pages never change — minimizing the surface that must stay
  index-stable.

## Testing

All automated; no manual testing required. Three layers, fast → slow.

**Layer 1 — template/data unit tests (no browser, fast).**
- **calendar_data:** known weekdays and leap-year handling (e.g. 2026-05-18 is a
  Monday; February day counts).
- **week grouping:** given `week_start`, day-list week breaks fall correctly (Sunday
  start in May 2026 → weeks begin on the 3rd, 10th, 17th, 24th, 31st).
- **geometry:** dot counts and exact page size per device.
- **config round-trip:** `dump` then `load` yields an equal `Config`; a hand-written
  yaml parses to expected values; missing/unknown keys handled.
- **wizard:** scripted answers through the injected input function produce the expected
  `Config` and write `rmbujo.yaml` to `<base>/<year>/`.
- **HTML rendering:** the Jinja templates render to HTML strings containing the expected
  elements (correct day count, week-break markers, legend symbols, blank-title cover
  area) — verified without launching a browser.

**Layer 2 — structural tests (Playwright → pypdf).**
- Each notebook produces a valid PDF with the expected page count and exact page size
  per device; text extraction confirms key content (day numbers + weekday abbreviations;
  reference legend symbols).
- **determinism:** building twice yields structurally identical PDFs (page count/size +
  normalized metadata).

**Layer 3 — visual regression (Playwright screenshots).**
- Screenshot each distinct page type and compare to committed golden PNGs with a small
  pixel-diff tolerance (Pillow + pixelmatch). This is the test that actually verifies
  the dot grid, cover art, pills, and day-list layout render correctly.
- Goldens depend on the Chromium version, which the flake pins via `flake.lock` — so
  they stay stable until nixpkgs is bumped. A `make update-goldens` target regenerates
  them on an intentional design change or a deliberate flake bump.

## Phase 2 (deferred — design intent only)

Captured so Phase 1 leaves the right seams; **not built now.**

- **ICS subscriptions:** the `ics:` config list holds multiple feeds, each with a
  `name`, `url`, and theme `color`. `ics.py` fetches and parses them into per-day
  events. Events bake into the day-list (and future-log) page backgrounds at generation
  time: all-day events (e.g. a holidays feed) render as colored pills on the day row;
  timed events render in the reserved right-hand gutter. **Holidays are simply one feed
  the user adds** — there is no built-in holiday logic.
- **Deploy / re-sync:** the `rmapi` `Deployer` backend (reMarkable cloud). Initial
  upload via `rmapi mkdir` + `put`/`mput`; non-destructive refresh via
  `rmapi put --content-only`, which replaces the PDF background while preserving the
  device's annotation `.rm` files and `.content` mapping. rmapi handles sync15
  content-hashing.
- **Why non-destructive refresh works:** on reMarkable, handwriting is stored in
  per-page `.rm` files keyed by a stable page-UUID, mapped to a **PDF page index** via
  `.content`. Inserting/reordering pages on-device does not renumber existing pages'
  PDF-index mapping, so a regenerated background lands on the correct page and the ink
  is preserved. This **requires** our generator to keep page count and the
  index→meaning mapping stable across runs for a given config — which the deterministic
  ordering guarantees. The exact `.content` schema will be verified empirically against
  the Move before Phase 2 relies on it.

## Rationale: PDF over native `.rm`

Native `.rm` (e.g. via `rmscene`) would allow true on-device toggleable layers, but its
writer is experimental and the reverse-engineered format breaks on firmware updates —
too fragile a foundation for a tool meant to run yearly and be shared. The PDF route
achieves the same "update the layer below without touching the user's edits" outcome
because the device already separates user ink (`.rm`) from the PDF background. We get
non-destructive refresh (Phase 2) without betting the core on an experimental binary
writer.
