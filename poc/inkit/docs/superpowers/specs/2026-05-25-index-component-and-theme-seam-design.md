# Index component + device-neutral Theme seam

**Status:** design approved, implementation pending
**Date:** 2026-05-25

## Goal

A reusable **Display** component for the reader's landing/contents documents — a
typographically clean listing of articles. The reader has a Library doc and a Feed
doc; both are index pages built from this one component.

Building it surfaces a second, load-bearing need: **device-optimal color**. The
reMarkable Paper Pro / Paper Pro Move have color e-ink; older reMarkables are
grayscale. We render optimally for each. A device-blind component therefore cannot
name literal colors — it needs a *semantic palette* the framework fills from the
device. So this spec delivers two things: the `Index` component **and** the minimal
`Theme` seam it reads from.

## Constraints / invariants honored

- **Apps and components are device-blind.** The component names *roles* (heading,
  byline, muted, …), never literal colors or a device. The framework supplies the
  palette, exactly as it already supplies page geometry (`PageGeom`) — geometry is
  the precedent for a render-time environment value, which is how "page-blind"
  works today.
- **`inkapp-core` stays connector-blind.** Core must never depend on a connector
  crate, so the `Article → IndexEntry` conversion lives in the Readwise crate.
- **Display mode:** `decode` returns `vec![]`. The component emits regions only as
  layout/recovery anchors (and to leave an interactive index possible later).
- **Existing goldens stay byte-identical.** The grayscale default theme emits no
  page fill and changes no existing component output.

## Background: what exists

- `Component` trait (`component.rs`): `render(&self, cx: &mut RenderCx) -> String`
  + `decode(...) -> Vec<Msg>`. `RenderCx { page, next_id }`.
- The render driver (`runtime.rs`) threads `PageGeom` through
  `document_source_in → RenderCx`; `App` carries `geom` (app-set via `.page()`),
  `key`, connectors, fetcher, asset_cache. **The `App`/render path never holds a
  `Device`** — render is fully device-agnostic; the `Device` seam lives only in the
  transport/sync layer.
- `Notice` (the closest Display component) hardcodes `#text(fill: red)`. There is
  **no** theme/palette seam anywhere yet.
- Shared helpers (`components/mod.rs`): `esc_typst_str` (string-literal escaping),
  `token_region`, the `#region(name, body, breakable)` prelude (`typst/region.typ`).
- `Passage` shows the breakable single-region pattern; `CalendarView` shows
  per-row regions.
- `Article` (Readwise) fields used: `title`, `author`, `site_name`,
  `reading_time: Option<String>` (**a String like `"5 min"`, never a number — pass
  through verbatim**), `summary`.
- rmreader's proven Paper Pro palette ("Indigo + Tomato"): blues/reds hold their
  color on Paper Pro e-ink while ambers/greens wash out. Roles:
  `paper #F3F1EA`, `ink #1A1A18`, `heading/indigo #2A2F6B`, `byline/rust #9C3A1B`,
  `muted #5E6166`, `rule #E0DDD2`.
- Harness golden flow: `compile_to_document → recover_regions → assert_golden`
  (`tests/common/mod.rs`). `inspector::inspect` renders **page 0 only** and overlays
  blue region rects + ink — unsuitable for a clean typographic golden.

## Design

### 1. `Theme` — `inkapp-core/src/theme.rs` (new)

A struct of semantic roles, each a **Typst fill-expression string** so components
interpolate them directly into `#text(fill: …)`:

```rust
pub struct Theme {
    pub ink: String,            // body / summary text
    pub heading: String,        // titles
    pub byline: String,         // author · site
    pub muted: String,          // secondary metadata (reading time)
    pub rule: String,           // hairlines
    pub paper: Option<String>,  // page fill; None = leave Typst default
}

impl Theme {
    pub fn grayscale() -> Self;       // the safe default
    pub fn indigo_tomato() -> Self;   // Paper Pro color palette
}
impl Default for Theme { fn default() -> Self { Self::grayscale() } }
```

- `grayscale()`: `ink = "luma(10%)"`, `heading = "black"`, `byline = "luma(35%)"`,
  `muted = "luma(45%)"`, `rule = "luma(80%)"`, `paper = None`.
- `indigo_tomato()`: `ink = rgb("#1A1A18")`, `heading = rgb("#2A2F6B")`,
  `byline = rgb("#9C3A1B")`, `muted = rgb("#5E6166")`, `rule = rgb("#E0DDD2")`,
  `paper = Some(rgb("#F3F1EA"))`.

Roles are limited to what `Index` uses now (plus `paper`); the type grows as more
components adopt it. (Migrating `Notice`'s hardcoded red to a `danger` role is a
natural follow-up, out of scope here.)

### 2. Threading the theme like `geom` (`component.rs` + `runtime.rs`)

- `RenderCx` gains `theme: Theme`. `RenderCx::new(page)` defaults it to grayscale
  (every existing call site compiles unchanged). Add `RenderCx::with_theme(self, Theme) -> Self`.
- Introduce `RenderEnv { geom: PageGeom, theme: Theme }` with `Default` and
  `From<PageGeom>`. The geom-taking render fns take `env: impl Into<RenderEnv>`:
  existing callers passing a `PageGeom` keep compiling via `From` (≈6 test call
  sites unchanged). `document_source_in` reads `env.geom` for `#set page` and injects
  `env.theme` into the `RenderCx`; it emits `fill: <paper>` **only when paper is
  `Some`** (grayscale → unchanged output).
- `App` gains a `theme: Theme` field (default grayscale) + a `.theme(Theme)` builder.
  It is set by the **binary's bootstrap**, never by app `update`/`view` — so app code
  stays device-agnostic. The `App` render path passes `RenderEnv { geom, theme }`
  down. Re-export `Theme` and `RenderEnv` from `lib.rs`.

Deferred (separate follow-up spec): auto-selecting the palette from `deploy.toml`'s
backend (Paper Pro → `indigo_tomato`, grayscale rM → `grayscale`) in the
facade/deploy resolver, so binaries need no manual `.theme(...)`.

### 3. `Index<M = ()>` — `inkapp-core/src/components/index.rs` (new)

```rust
pub struct IndexEntry {
    pub title: String,
    pub byline: Option<String>,        // author or site_name; None if neither
    pub reading_time: Option<String>,  // verbatim passthrough (e.g. "5 min")
    pub summary: Option<String>,
}
impl IndexEntry { pub fn new(title: impl Into<String>) -> Self; } // others None

pub struct Index<M = ()> { entries: Vec<IndexEntry>, _msg: PhantomData<fn() -> M> }
impl<M> Index<M> { pub fn new(entries: Vec<IndexEntry>) -> Self; }
```

`render(&self, cx)`:
- Each entry is a **non-breakable `#region("idx-{i}", […])` box** — an entry never
  splits across a page break; the *list* flows and Typst paginates between entries.
- Per entry: title in `cx.theme.heading` (strong); a meta line `byline · reading_time`
  using `cx.theme.byline` / `cx.theme.muted` (omit each part when `None`); optional
  summary in `cx.theme.ink`, **truncated to `DEFAULT_SUMMARY_CHARS` (~200) + "…"**
  (keeps a non-breakable box page-safe; contents pages want short summaries); a
  `cx.theme.rule` hairline (`#line`) between entries.
- All user text injected as Typst **string expressions** (`#"…"` via `esc_typst_str`)
  — the Notice/Passage safety recipe so `[`, `]`, `#` stay literal.

`decode(...) -> vec![]` (Display). Regions are layout/recovery anchors only.

One line in `components/mod.rs`: `pub mod index;`.

**Design choice — per-entry boxes vs. breakable single region.** The prompt
suggested reusing Passage's breakable single region. I chose per-entry non-breakable
boxes instead: cleaner whole-entry typography (no title-on-p1/summary-on-p2 splits)
and per-entry pagination recovery for the test (and a future interactive index).
Trade-off: requires summary truncation to keep a box within one page — acceptable
and desirable for a contents page. Switching to one breakable `index` region later
is a localized change if ever wanted.

### 4. `Article → IndexEntry` — `inkapp-readwise-reader/src/index_entry.rs` (new)

`impl From<&Article> for inkapp_core::components::index::IndexEntry` (orphan-rule
legal: `Article` is local to this crate, which already depends on core):
- `title = article.title.clone()`
- `byline = (!author.is_empty()).then(author) else (!site_name.is_empty()).then(site_name) else None`
- `reading_time = article.reading_time.clone()` — **verbatim, never parsed**
- `summary = (!summary.is_empty()).then(summary)`

One line `mod index_entry;` in the Readwise `lib.rs`.

### 5. Harness clean rasteriser (`inkapp-harness/src/inspector.rs`)

Extract `pub fn render_page(doc: &PagedDocument, page_index: usize) -> Result<Vec<u8>>`
— the typst-render → pixmap → PNG path with **no overlays**, for any page index.
`inspect` is refactored to call it then draw region/ink overlays on page 0.

## Testing (TDD)

1. **Readwise unit tests** (in `index_entry.rs`): byline fallback (author present →
   author; author empty → site; both empty → None); `reading_time` passthrough
   (`Some("5 min")` stays `"5 min"`; `None` stays `None`); `summary` `""` → `None`.
2. **Core unit tests** (in `index.rs`): rendering with a `RenderCx` themed
   `indigo_tomato` contains the themed fill exprs (`rgb("#2A2F6B")` for the title,
   etc.); titles/bylines are escaped string-expressions; missing `reading_time`/
   `summary`/`byline` are omitted cleanly; `decode` is always empty. (No PNG — proves
   the theme threads into markup deterministically.)
3. **Harness golden** (new `tests/index_render.rs`): build a ~14-entry `Index` doc;
   `compile_document_in` at default geom; `recover_regions`; assert `page_count == 2`
   and that `idx-*` regions land on **both** page 0 and page 1 (pagination proof);
   `assert_golden` page 0 and page 1 (grayscale, via `render_page`).

All green under `nix develop -c cargo test --workspace`.

## Definition of done

Update `docs/appdx.md` to mark the `Index` component **and** the `Theme` /
`RenderCx` palette seam as built (per project convention: appdx is the definition of
done).

## Files touched

- **core:** `theme.rs` (new), `component.rs`, `components/index.rs` (new),
  `components/mod.rs` (+1 line), `runtime.rs`, `lib.rs`.
- **readwise:** `index_entry.rs` (new), `lib.rs` (+1 line).
- **harness:** `inspector.rs`, `tests/index_render.rs` (new), goldens under
  `tests/golden/`.
- **docs:** `docs/appdx.md`.

## Conventions

- Clear native tasks before committing (the commit hook blocks on open tasks).
- Do **not** stage `Cargo.lock`.
- Verify with `nix develop -c cargo test --workspace` (not `-p`), and keep
  `make clippy` (warnings = errors) clean.
