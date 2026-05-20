# rmbujo — Design Spec

**Date:** 2026-05-20
**Status:** Approved for planning
**Author:** Dan (with Claude)

## Summary

`rmbujo` is a **Rust** CLI that generates a year's worth of dot-grid bullet-journal
PDFs sized for reMarkable devices — primarily the **Paper Pro Move** (the only tested
target), with the larger **Paper Pro** also selectable. It produces one PDF per
"notebook," written into a flat per-year folder, driven entirely by a per-year **TOML**
config. Pages are authored as **askama HTML templates + CSS** and rendered to PDF by
**fulgur** (HTML/CSS → PDF via Blitz + krilla — **no headless browser**). Output is
byte-deterministic so a later phase can refresh page backgrounds on-device without
disturbing the user's handwriting.

The tool is open source: well documented, commented, and easy to extend (pluggable ICS
sources, themes, and deploy backends). Rust was chosen for compile-time checking
(valuable as features like ICS grow) and a single static binary.

### Phasing

- **Phase 1 (this spec):** the PDF generator + config/wizard workflow. Writes PDFs to
  disk. No device integration, no calendar-event rendering.
- **Phase 2 (deferred, architected-for):** ICS subscriptions (multiple) baked into the
  day-list / future-log backgrounds — including holidays, which are simply one ICS feed
  the user can add. Plus a deploy/re-sync step via `rmapi` (reMarkable cloud). Designed
  for now via deterministic page ordering, a reserved event gutter, an `ics` config
  section, and a deploy seam — but **not implemented** in Phase 1.

> Note: the git repo directory is currently `~/git/rppmbujo`. The crate, CLI, and
> project name are `rmbujo`. The repo directory will be renamed separately, later — out
> of scope for this spec.

## Goals

- Generate a complete bullet-journal year for the Paper Pro Move from a single config.
- Look good on a color e-ink screen: dot grid, deep/legible colors, no black fills.
- "Set it once, re-run later": a config file captures all settings; re-running points
  at that file and regenerates with identical settings.
- Be trivially repeatable for future years.
- Be a clean, idiomatic-Rust open-source codebase: small focused modules; compile-time
  checked templates; pluggable extension points. **Not a Python port.**

## Non-goals (Phase 1)

- No calendar/ICS rendering — including no holidays. Holidays arrive in Phase 2 as a
  user-supplied ICS feed, not a built-in.
- No device upload / cloud sync (Phase 2).
- No headless browser (fulgur renders natively).
- No native reMarkable `.rm` file writing (rejected — see Rationale).
- No GUI.

## Rendering engine: fulgur (validated by spike)

A prototype (committed under `docs/superpowers/spikes/` notes) rendered the three
hardest elements at the Move's exact page size and confirmed:

- **Custom page size** via `PageSize { width, height }` (points) and/or
  `@page { size: 260pt 462pt }`. ✅
- **Flexbox layout, text, navy headers, weekday colors, per-row rules, week-start
  divider.** ✅
- **Pills** via `border-radius` + background color. ✅
- **Deterministic metadata** via `Engine::builder().producer(..).creator(..).creation_date(..)`,
  and **byte-deterministic** output across runs. ✅
- **fulgur 0.6 does NOT paint CSS gradients** (its converter handles only
  `background-image: url(...)`, raster or SVG via usvg). ❗ → the **dot grid and cover
  gradient are generated as SVG assets** (registered with `AssetBundle::add_image`,
  referenced via `url(dot.svg)` / `url(cover.svg)`), which krilla rasterizes crisply. ✅
- **Nix-on-macOS** builds once `libiconv` is in the dev shell. ✅

fulgur is `0.x` (unstable API) — pinned via `Cargo.lock`. If it ever breaks or is
abandoned, the HTML/CSS designs and the krilla knowledge transfer.

## Device geometry

PDFs are vector, so the device scales them cleanly. We match the screen aspect ratio
to avoid letterboxing and use physical inches so the 5 mm dot grid is true-to-size.

| Device           | Pixels    | PPI | Page (portrait) | Points (w × h) |
|------------------|-----------|-----|-----------------|----------------|
| `paper-pro-move` | 954×1696  | 264 | 3.61″ × 6.42″   | 260.18 × 462.55|
| `paper-pro`      | 1620×2160 | 229 | 7.07″ × 9.43″   | 509.34 × 679.13|

Default and only tested target: `paper-pro-move`. All page size, margin, and dot-grid
math derive from the selected device plus the theme.

## Notebooks and page layouts

All files are flat inside the year output folder (the folder containing `rmbujo.toml`).

### Filenames

| Notebook            | Filename pattern              | Example                       |
|---------------------|-------------------------------|-------------------------------|
| Future Log          | `YYYY Future Log.pdf`         | `2026 Future Log.pdf`         |
| Month (×12)         | `YYYY.MM <Month>.pdf`         | `2026.05 May.pdf`             |
| Collection Template | `YYYY Collection Template.pdf`| `2026 Collection Template.pdf`|
| Reference           | `YYYY Reference.pdf`          | `2026 Reference.pdf`          |
| Config              | `rmbujo.toml`                 | `rmbujo.toml`                 |

Sort order on device: the `YYYY <name>` files (Collection Template, Future Log,
Reference) sort before the `YYYY.MM` month files.

### Future Log — `YYYY Future Log.pdf`

- Cover page (see Cover spec).
- 4 content pages, **3 months stacked per page** (single-page device — no spreads).
- Each month block: month-name header (navy) + a freeform dot-grid area for "big
  things." Not day-numbered.

### Month — `YYYY.MM <Month>.pdf`

1. **Day list (page 1).** "<Month> YYYY" header, then every day of the month as
   `8 Mon` — day number in black, weekday abbreviation in navy. Days fill the page
   height (flex column). Weekday computed from the real calendar for the given year.
   Days are visually grouped into weeks by a faint rule before each week's first day;
   the week boundary is determined by `week_start` (Sunday by default, Monday optional)
   — this is the one place `week_start` takes effect. Each row reserves a right-hand
   gutter for ICS events (populated in Phase 2; empty in Phase 1).
2. **Tasks (page 2).** "Tasks" header + dot grid.
3. **Daily log (pages 3…N).** Full dot grid, no date printed (the user writes it).
   Default `daily_pages = 60`.

ASCII sketch of the Move day-list page (Phase 1):

```
┌──────────────────────────┐
│ May 2026                  │  ← navy header
│  1  Fri                   │  ← day black, weekday navy
│  2  Sat                   │
│ ──────────────────────    │  ← faint rule at week start (week_start)
│  3  Sun                   │
│ ...        └─ ICS zone ─┘ │  ← right gutter reserved (empty in Phase 1)
│ 31  Sun                   │
└──────────────────────────┘
```

### Collection Template — `YYYY Collection Template.pdf`

A single template the user duplicates on-device for each new collection.

- Decorated cover (SVG gradient) with a **blank title area** (a labeled space /
  underline) where the user hand-writes the collection name after duplicating.
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

Palette **"Library"** lives in `themes/library.toml`:

| Role     | Hex       | Name  |
|----------|-----------|-------|
| Primary  | `#1B365D` | Navy  |
| Event    | `#8B2E1F` | Brick |
| Accent 1 | `#A07E1C` | Ochre |
| Accent 2 | `#556B2F` | Olive |
| Rule     | `#D9D6CC` | Rule  |
| Dot      | `#CFCDC4` | Dot   |

No black fills (poor for color e-ink). Brick is reserved for all-day ICS event pills
(e.g. a holidays feed) in Phase 2. The theme is emitted as CSS custom properties
(`:root { --navy: #1B365D; ... }`) injected into each page's `<style>`, and the **dot
tile + cover SVGs are generated from theme colors**, so re-skinning is editing one TOML
file. A **TTF font is vendored in-repo** and embedded via fulgur's `add_font_bytes`
(referenced by `font-family` in CSS), so output is identical regardless of system
fonts; a theme may name a different bundled font.

## Invocation model

```
rmbujo new                    # interactive wizard → creates year folder + config → builds
rmbujo path/to/rmbujo.toml    # regenerate from an existing config (Phase 2: also re-syncs)
```

- **`rmbujo new`** runs a wizard (dialoguer), creates `<base>/<year>/`, writes
  `<base>/<year>/rmbujo.toml`, then generates the PDFs into that folder.
- **`rmbujo <config.toml>`** loads the config and regenerates with identical settings,
  no prompts. In Phase 2 the same command re-syncs via the configured deploy backend.

The config file lives **inside the year folder**, so a year is self-contained and
movable. The config's own directory **is** the output directory. The Phase 2 deploy
step uploads only `*.pdf`, so the toml never syncs to the device.

### Wizard questions (each prefilled with the default)

Year (default: current year) → base directory (default: cwd) → device → week start →
daily pages → collection pages → theme. The Phase 1 wizard does not prompt for ICS
subscriptions or deploy settings; it writes an empty `[[ics]]` list and
`deploy.backend = "none"`. (Prompting for ICS feeds arrives with Phase 2 rendering;
users may pre-populate the `ics` list by editing the toml.)

### Config schema (`rmbujo.toml`)

```toml
# rmbujo — config for 2026
# regenerate / re-sync with:  rmbujo path/to/this/rmbujo.toml
year = 2026
device = "paper-pro-move"      # paper-pro-move | paper-pro
week_start = "sun"             # sun | mon
daily_pages = 60
collection_pages = 20
theme = "library"              # bundled name, or a path to a theme toml

# Phase 2 — subscriptions rendered onto day-list/future-log. Holidays are just
# another feed. Empty in Phase 1. Example:
# [[ics]]
# name = "Holidays"
# url = "https://example.com/canada-on-holidays.ics"
# color = "brick"             # theme color name; all-day events → pills

[deploy]                       # written now, inert until Phase 2
backend = "none"               # none | rmapi
target_folder = "/2026"        # reMarkable cloud folder
```

## Code architecture

```
flake.nix · flake.lock · .envrc   # Nix dev shell: rust toolchain, libiconv, poppler, fontconfig
Cargo.toml · Cargo.lock           # crate + pinned deps
Makefile                          # test / build / update-goldens
src/
  main.rs            # thin bin entry → rmbujo::cli::main()
  lib.rs             # module declarations (enables integration tests)
  cli.rs             # clap dispatch: `new` → wizard; <path> → load + generate
  wizard.rs          # interactive prompts (dialoguer) → Config + writes toml
  config.rs          # Config/IcsFeed/DeployConfig (serde); load/dump TOML
  device.rs          # Device specs → page geometry (points)
  calendar.rs        # year → months → days/weekdays + week grouping (chrono)
  geometry.rs        # dot-grid math (spacing, margin, counts)
  theme.rs           # theme TOML → palette; CSS custom-property string
  svg.rs             # generate dot-tile SVG + cover SVG from theme + geometry
  templates.rs       # askama Template structs (one per page type)
  render.rs          # assemble HTML + SVG assets → fulgur Engine → PDF bytes
  generate.rs        # orchestrate a year → write all notebook PDFs (NOT build.rs — reserved)
  notebooks/
    mod.rs           # shared: build page fragments + render one notebook PDF
    month.rs · future_log.rs · collection.rs · reference.rs
  deploy/
    mod.rs           # Deployer trait + get_deployer()
    local.rs         # backend "none": no-op (Phase 1)
    # rmapi.rs       ← Phase 2
  # ics.rs           ← Phase 2: fetch + parse ICS feeds → per-day events
templates/           # askama HTML templates (compile-time checked)
  base.html · cover.html · month_index.html · tasks.html · dotgrid.html
  future_log.html · reference.html
themes/
  library.toml
assets/fonts/
  <Bundled>.ttf      # vendored font, embedded via add_font_bytes
tests/
  <integration tests> + tests/goldens/
```

Design notes:

- **askama templates are compile-time checked** and render to HTML strings — testable
  without rendering a PDF by asserting on the string.
- **`render.rs` is the only fulgur-touching module** — HTML + assets in, PDF bytes out
  — isolating the engine behind one seam (eases a future engine swap).
- **`svg.rs`** generates the dot tile and cover SVG deterministically from theme +
  geometry; `render.rs` registers them as fulgur assets.
- **Notebook builders** assemble page fragments (one `<section class="page">` per page,
  separated by CSS `break-after: page`) into one HTML doc per notebook, then render.
- **Extension seams:** theme TOML → CSS + SVG colors, the `Deployer` trait
  (deploy/refresh backends), and the `ics` config list (Phase 2 sources). Forks plug in
  without touching the core.
- The orchestrator is `generate.rs` (with `generate_year`), **not** `build.rs` —
  `build.rs` at the crate root is reserved by Cargo for build scripts.

## Dependencies & development environment

All dependencies are managed with **Nix**. A `flake.nix` provides a reproducible dev
shell, and **direnv** (`.envrc` containing `use flake`) loads it on `cd` into the repo.
The flake also builds the crate (`rustPlatform.buildRustPackage`) from the same nixpkgs
revision, so the dev shell and the built tool share one toolchain.

Provided by the flake (pinned via `flake.lock`):
- **Rust toolchain** (`rustc`, `cargo`, `clippy`, `rustfmt`).
- **`libiconv`** — required to link on macOS (the one gap the spike hit).
- **`poppler_utils`** — `pdftoppm`, used by the visual-regression tests to rasterize.
- **`fontconfig`**, `pkg-config`.

Crate dependencies (pinned via `Cargo.lock`):
- `fulgur` (HTML/CSS → PDF), `askama` (compile-time templates), `serde` + `toml`
  (config/theme), `chrono` (calendar/weekdays), `clap` (CLI), `dialoguer` (wizard),
  `anyhow` (errors).
- Dev: `lopdf` (PDF structural assertions), `image` (golden PNG diffing).

The render font is **vendored in `assets/fonts/`** and embedded into every PDF, so
rasterization is self-contained and reproducible.

Flake outputs:
- `devShells.default` — the dev shell (direnv-loaded, or `nix develop`).
- `packages.default` / `nix run` — the built `rmbujo` CLI.

(Phase 2 adds an ICS crate such as `icalendar`, and shells out to the external `rmapi`
binary for the cloud backend.)

## Error handling and determinism

- Validate device name and week-start at parse time (typed enums); clear messages on
  bad input. Missing required config keys (e.g. `year`) error clearly; optional keys
  use documented defaults via `serde(default)`.
- **Deterministic output:** fulgur produces byte-deterministic PDFs; we additionally
  pin metadata via the builder (`producer`, `creator`, fixed `creation_date`). Phase
  2's `rmapi put --content-only` needs stable page geometry (order, count, box size),
  guaranteed by fixed templates + fixed page size. Building twice yields **byte-identical**
  PDFs.
- Regeneration overwrites existing output by default (regeneration is the point).
- Dynamic content (ICS, Phase 2) is confined to the day-list and future-log pages;
  daily/dot-grid pages never change — minimizing the surface that must stay
  index-stable.

## Testing

All automated via `cargo test`; no manual testing required. Four layers, fast → slow.

**Layer 1 — logic units (no rendering).**
- **calendar:** known weekdays and leap-year handling (2026-05-18 is a Monday; Feb
  2024 = 29 days, Feb 2026 = 28).
- **week grouping:** given `week_start`, week breaks fall correctly (Sunday start in
  May 2026 → weeks begin on the 3rd, 10th, 17th, 24th, 31st).
- **geometry:** dot counts and exact page size per device.
- **config round-trip:** serialize then parse yields an equal `Config`; a hand-written
  toml parses to expected values; missing `year` errors; defaults applied.
- **svg:** dot-tile and cover SVG strings contain the expected shapes/colors.

**Layer 2 — HTML-string tests (askama, no PDF).**
- Templates render to HTML containing the expected structure: 31 day rows with the
  `weekstart` class on the right days, navy header, the reference legend incl. `=`,
  blank-title cover area.

**Layer 3 — PDF structural tests (`lopdf`).**
- Each notebook renders to a valid PDF with the expected page count and page-box size
  per device (month = `2 + daily_pages`, future-log = 5, collection = `1 + N`,
  reference = 3).
- **determinism:** rendering the same notebook twice yields **byte-identical** PDFs.

**Layer 4 — layout / overlap inspection (`fulgur::inspect`).**
- Render a notebook, call `fulgur::inspect::inspect(pdf)` to get every laid-out
  `TextItem { page, x, y, width, height, text, .. }`, and assert two invariants per page:
  (1) **no two text boxes intersect** (rectangle-overlap check with a small tolerance),
  and (2) **every text box lies within the page bounds** (no overflow/clipping). This
  catches text-overlap and run-off-the-page bugs geometrically, on the first render —
  not merely as a regression from a golden.
- Caveat: `inspect()` estimates text width from font size (not exact glyph metrics), so
  the overlap check uses approximate widths with a tolerance — reliable for gross
  overlaps, not sub-pixel exact. Layer 5 (visual) covers finer rendering drift.

**Layer 5 — visual regression.**
- Rasterize each distinct page type with `pdftoppm` → PNG, diff against committed golden
  PNGs via the `image` crate with a small pixel tolerance. This verifies the dot grid,
  cover gradient, pills, and day-list layout actually render.
- `RMBUJO_UPDATE_GOLDENS=1 cargo test` regenerates goldens. Goldens are reproducible:
  fulgur/krilla/Blitz pinned via `Cargo.lock`, the font vendored, poppler pinned via
  the flake. A `make update-goldens` target wraps the refresh.

## Phase 2 (deferred — design intent only)

Captured so Phase 1 leaves the right seams; **not built now.**

- **ICS subscriptions:** the `ics` config list holds multiple feeds, each with a `name`,
  `url`, and theme `color`. An `ics.rs` module fetches and parses them into per-day
  events baked into the day-list (and future-log) backgrounds at generation time:
  all-day events (e.g. a holidays feed) render as colored pills on the day row; timed
  events render in the reserved right-hand gutter. **Holidays are simply one feed the
  user adds** — there is no built-in holiday logic.
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

## Rationale

**fulgur over headless Chromium.** A native HTML/CSS → PDF engine (Blitz + krilla)
removes the Chromium/Playwright dependency entirely, yields byte-deterministic output,
and builds to a single binary. The spike proved it renders our design (the only gap —
CSS gradients — is handled by generated SVG assets). Chromium would have been heavier,
non-deterministic without metadata scrubbing, and harder to package.

**PDF over native `.rm`.** Native `.rm` (e.g. via `rmscene`) would allow true on-device
toggleable layers, but its writer is experimental and the reverse-engineered format
breaks on firmware updates — too fragile for a tool meant to run yearly and be shared.
The PDF route achieves the same "update the layer below without touching the user's
edits" outcome because the device already separates user ink (`.rm`) from the PDF
background. We get non-destructive refresh (Phase 2) without betting the core on an
experimental binary writer.
