# rmreader — annotation read-back via a reusable `rmfiles` crate

**Date:** 2026-05-22
**Status:** approved (design)
**Supersedes the "Future phase" stub in** `2026-05-21-rmreader-design.md`.

Turn the Reader PDFs into a round-trip: the user triages and highlights on the
reMarkable, and on the next sync `rmreader` reads those marks back, acts on them
against the Readwise Reader API, then regenerates and re-uploads a fresh PDF. The
on-device reMarkable file parsing is extracted into a new, reusable pure-Rust crate,
`rmfiles`, so other projects (rmbujo, a future Files tool, etc.) can share it.

## Goals

- On each sync, read what the user did on the on-device PDF and apply it:
  - **Per-article actions** — Inbox / Archive / Later / Delete — chosen by
    highlighting an action label word at the top of the article.
  - **Content highlights** — any text the user highlighted in the article body,
    pushed back to Readwise as highlights.
- Keep the round-trip **idempotent**: every sync replaces the on-device document
  with a fresh, un-annotated PDF, so each sync only ever sees *new* marks.
- Extract reMarkable file parsing into **`rmfiles`**, a standalone reusable crate,
  built pure-Rust (no Python sidecar, no aging third-party parser), scoped to
  exactly what we need now and designed to grow organically.

## Non-goals (this phase)

- Rendering Readwise highlights *back into* the regenerated PDF.
- Preserving on-device handwriting or reading position across syncs (explicitly
  discarded — see "Upload model").
- Ink-stroke / freehand-checkbox detection by geometry. We use snap-to-text
  highlights only. (`rmfiles` leaves room to add stroke parsing later.)
- Native interactive PDF checkboxes — **confirmed impossible** on reMarkable (no
  AcroForm support; the 3.8 "checkboxes" are a notebook-only feature). The
  label-highlight mechanism is the chosen alternative, not a fallback.
- Index-level / bulk triage from the index page (per-article only this phase).

## Key decisions (resolved during brainstorming)

| Decision | Choice |
|----------|--------|
| Action mechanism | **Highlight a label word** (snap-to-text), not ink-in-a-box. One read-back channel shared with content highlights. |
| Label placement | **Repeated on every page** of each article, so triage works from anywhere. |
| Upload model | **Full replace** each sync (`rmapi rm` + `put`): writing + reading position reset. Makes read-back idempotent. |
| Action conflicts | **Skip + warn** when ≥2 distinct action labels are highlighted on one article. Content highlights still pushed. |
| CLI shape | **One combined command** (`rmreader <config>`): read-back → act → regenerate → replace. |
| State model | **The PDF is the single source of truth.** The page→doc mapping + per-doc metadata are embedded *inside* the generated PDF, so the downloaded bundle is self-describing. No local manifest state to keep in sync. |
| RM parser | **Pure Rust, our own crate** (`rmfiles`). Spike against a real fixture first. |
| Cloud layout | All tools live under a `/RMDev` root; rmreader uses `/RMDev/Reader`. |

## Confirmed external facts (researched 2026-05-22)

reMarkable / rmapi:

- `rmapi get <path>` downloads a `.rmdoc` (a renamed zip). For an annotated PDF it
  contains the **original `.pdf` unchanged**, `<uuid>.content` (JSON),
  `<uuid>.metadata` (JSON), `<uuid>.pagedata`, and a `<uuid>/` dir of per-page
  `<page-uuid>.rm` files. Annotations live in the `.rm` files; reMarkable does **not**
  rewrite the source PDF. Use `get`, **not** `geta` (which flattens/renders).
  *This preservation is load-bearing: it lets us embed our own metadata in the PDF
  at generation time and read it back from the downloaded bundle (see State model).*
  The round-trip spike confirms our embedded data survives.
- Paper Pro writes the **v6** `.rm` "scene blocks" format. Files start with the
  43-byte ASCII header `reMarkable .lines file, version=6` (space-padded to 43).
- Snap-to-text highlights are stored as a **`GlyphRange`** scene item inside a
  `SceneGlyphItemBlock`, carrying the **verbatim highlighted `text`**, a `length`,
  `color`, and `rectangles: Vec<{x,y,w,h}>` (device space). No OCR needed.
  *Freehand* highlighter strokes that do **not** snap to text produce only ink
  geometry (no text) — out of scope.
- v6 coordinates: `1404 × 1872`, origin top-left, **X centered on zero**
  (≈ −702..+702). Canonical rm→PDF transform: `SCALE = 72/226`,
  `X_SHIFT = (1404*SCALE)/2`. For *imported PDFs* the page is fit into the viewport
  per a transform recorded in `.content`; exact PDF-point mapping needs that fit
  transform. **We avoid needing it this phase** (see Classification).
- reMarkable has **no PDF AcroForm support** and writes no machine-readable
  checkbox state for PDF content.

Readwise Reader API:

- Change location: `PATCH https://readwise.io/api/v3/update/<id>/` with
  `{"location": "..."}`. Valid values: **`new`, `later`, `archive`, `feed`**
  (`shortlist` is a list filter only, not settable here). Inbox → `new`.
- Delete: `DELETE https://readwise.io/api/v3/delete/<id>/` → 204.
- Create highlight: **no v3 path and no document-id targeting.** Use classic
  `POST https://readwise.io/api/v2/highlights/` with
  `{"highlights":[{text, title?, author?, source_url?, ...}]}`. Required: `text`
  (≤ 8191 chars). Readwise matches to a document by **`source_url`** (+ title /
  author). So content highlights are attached by setting `source_url` to the
  article's URL.

## Architecture: two components

```
/home/dan/git/rmfiles      (new crate)   reMarkable file parsing, reusable
/home/dan/git/rmreader     (this repo)   read-back feature consuming rmfiles
```

The boundary is the `rmfiles` public API. Everything Readwise- and PDF-content–
specific stays in `rmreader`.

### Component A — `rmfiles` (new reusable crate)

Pure-Rust library that turns a reMarkable document bundle into structured data. It
knows nothing about Readwise or treating a PDF as article content; it only parses
reMarkable's own file formats.

**Intended use cases (shape the API now, even though only highlights are built):**

- *rmreader* (this project): read snap-to-text highlights to drive triage + Readwise
  highlights.
- *rmbujo*: read planner annotations (handwriting/checkbox strokes + highlights).
- *Highlight export*: dump every highlight to Markdown per document — rmscene's
  original motivation; needs all highlights with text + color.
- *Handwriting/stroke export* (SVG/PNG): needs `Line`/stroke items + the device→PDF
  geometry transform.
- *Backup / sync tooling*: read metadata (`visible_name`, tags, page count,
  timestamps) without touching strokes.

So the surface is a general bundle + a general scene-item iterator, not a
highlights-only API.

**v0.1 public API (general shape; only `Highlight` parsing implemented now):**

```rust
// Open a .rmdoc / .zip, OR an already-unpacked directory.
let bundle = rmfiles::Bundle::open(path)?;

bundle.metadata();      // -> Metadata { visible_name, last_modified, doc_type, tags, .. }
bundle.source_pdf();    // -> Option<Vec<u8>>  (original PDF if this is an annotated PDF)
bundle.pages();         // -> Vec<Page>, reading order from .content

for page in bundle.pages() {
    page.index;                          // 0-based page order (== source PDF page)
    page.id;                             // page uuid
    if let Some(scene) = page.scene()? { // parse this page's v6 .rm, if present
        scene.version();                 // u32 (6 today)
        for item in scene.items() {      // general iterator over a #[non_exhaustive] enum
            match item {
                SceneItem::Highlight(h) => { h.text; h.rectangles; h.color; }
                SceneItem::Line(_l)     => { /* strokes — added when a consumer needs them */ }
                _ => {}                  // non_exhaustive: forward-compatible
            }
        }
        scene.highlights();              // convenience filter -> Vec<Highlight>
        // scene.lines();                // convenience filter (future)
    }
}
```

`Highlight { text: String, rectangles: Vec<Rect>, color: Color }` — `rectangles` in
device space (1404×1872, centered X). The `#[non_exhaustive]` `SceneItem` enum +
convenience filters mean export tools can take *all* items while rmreader takes only
highlights, and new item types (`Line`, `Text`, `Group`) are added without breaking
callers.

**Module layout:**

```
rmfiles/src/
  lib.rs            // re-exports; crate docs
  error.rs          // Error enum (thiserror): Io, Zip, Json, UnsupportedVersion(u32), Parse
  bundle/
    mod.rs          // Bundle::open (zip OR dir), page enumeration
    content.rs      // .content JSON (page order, per-page metadata) — serde
    metadata.rs     // .metadata JSON — serde
  scene/
    mod.rs          // SceneFile::parse: detect v6 header, walk tagged blocks
    reader.rs       // low-level little-endian / tagged-block reader
    items.rs        // scene items: GlyphRange now; Line/Stroke later (additive)
  geometry.rs       // Rect, Color, device-space constants (1404×1872, SCALE, X_SHIFT)
tests/
  fixtures/         // REAL Paper Pro .rmdoc files (committed)
  highlights.rs     // assert text/rects/page from fixtures
```

**Design-for-reuse choices:**

- The block walker reads every tagged block and **skips unknown block types**, so
  adding `Line`/stroke extraction later does not break existing callers.
- `highlights()` is one method of a scene-item API; future consumers can pull
  strokes, layers, text without an API break.
- Minimal deps: `zip`, `serde` + `serde_json`, `thiserror`. Hand-rolled
  little-endian reads (no `byteorder` needed). **No PDF dependency** — keeps the
  crate generic and light.
- A non-v6 header returns `Error::UnsupportedVersion`; we implement only v6 (all a
  Paper Pro emits). Other versions are added when a project needs them.
- The device→PDF-point transform constants live here (`geometry.rs`), but applying
  the imported-PDF fit transform is **deferred** until a consumer needs exact PDF
  mapping. `rmreader` v1 uses relative position only.

**Spike before committing:** capture a real annotated `.rmdoc` from the Paper Pro
(highlight two known body words **and** one action label), commit it as a fixture,
and assert `rmfiles` returns those exact strings with page index + rectangles. This
validates both the format work and the feature premise in one step. Dan captures the
fixtures; they are committed and expanded as new cases surface.

### Component B — `rmreader` read-back feature

New module `src/readback/` (fetch bundle, read the PDF's embedded manifest, extract
highlights via `rmfiles`, classify, drive Readwise), plus additions to `readwise/`,
`manifest.rs`, `postprocess.rs` (embed the manifest), `deploy/`, `config.rs`,
`generate.rs`/`cli.rs`.

#### Sync loop (one command: `rmreader <config>`)

Read-back runs **before** regeneration. The downloaded PDF is self-describing — it
carries its own page→doc manifest (see State model) — so there is no local state to
consult:

1. **Fetch** the current bundle from the device (`Deployer::fetch`, i.e. `rmapi
   get`). No doc on device (first run) → skip to step 5.
2. **Read the embedded manifest** from the downloaded original PDF (lopdf) — the
   page→doc map + per-doc metadata that *we* embedded when we generated it. No prior
   local manifest needed.
3. **Extract + classify** highlights → a `Plan` of Readwise operations.
4. **Execute** the plan (location changes, deletes, content highlights). Log a
   summary; a single failed op logs and does not abort the rest.
5. **Regenerate** fresh PDFs (existing pipeline) — now reflecting post-action state
   (archived/deleted items have dropped out) — each with a freshly embedded manifest
   for next time.
6. **Full-replace** upload (`Deployer::replace`, i.e. `rmapi rm` + `put`).

Ordering rationale: read-back must run before regeneration because regeneration
changes the document set and page layout. But it reads the *downloaded* PDF's own
embedded manifest, not any local file — so `output_dir` is just scratch space for the
freshly built PDFs, not authoritative state. There is no device/computer state to
keep in sync.

#### Classification (`src/readback/classify.rs`)

For each highlight from `rmfiles`:

- **Page → doc:** the *embedded* manifest (read from the downloaded PDF) records each
  doc's `page_range { first, last }`. The highlight's bundle page index (== source
  PDF page) maps to the owning doc. A highlight on a page absent from the embedded
  manifest → warn + skip.
- **Action vs content:** `Action(doc, kind)` if the normalized highlight text ∈
  `{inbox, archive, later, delete}` **and** the highlight sits in the action band
  (topmost band of the page). Otherwise `ContentHighlight(doc, text)`.
  - The band check is **device-space relative**: the highlight's rectangles fall in
    the top band of the page's annotated content (the stamped labels are the topmost
    text on every page), tested against the page's own coordinate range without the
    imported-PDF fit transform. Text match is primary; the band check guards against
    a bare body highlight of a word like "archive".
- **Per-doc resolution:** 0 actions → leave location unchanged; exactly 1 distinct
  action → apply; ≥2 distinct → skip + warn. Content highlights are always pushed.

Output: `Plan { actions: Vec<(doc_id, ActionKind)>, highlights: Vec<HighlightCreate>,
warnings: Vec<String> }`. Pure and table-testable from synthetic highlight inputs.

#### Action labels on the page (`postprocess.rs`)

Stamp `INBOX  ARCHIVE  LATER  DELETE` as **real text** on every article page, in a
reserved band just below the existing nav bar, at coordinates we choose:

```
  < Prev      Home      Next >        ← nav bar  (clickable; existing lopdf stamp)
  INBOX   ARCHIVE   LATER   DELETE     ← action band (highlightable; new lopdf stamp)
  ─────────────────────────────────
  Headline …
```

Stamping it ourselves gives two payoffs: (a) it is real PDF text → snap-to-text can
grab it; (b) the labels render at a known, constant top position on every page, so
the device-space top-band test in Classification is reliable. (Exact PDF-point label
rects are *also* known from the stamp, but comparing them to the device-space
highlight rects would need the deferred PDF fit transform, so v1 relies on relative
top-band position, not exact-rect overlap.) The nav bar
stays lopdf-stamped (it needs clickable `/Link` annotations, which fulgur
running-headers cannot emit — see the 2026-05-21 running-header spike). The action
band needs highlightable text, not links, so stamping is sufficient. The top margin
is enlarged so the band does not collide with content.

**Spike (on-device):** confirm the device's snap-to-text highlighter snaps to our
lopdf-stamped label text (and to fulgur body text). The same parser-spike fixture
covers it (Dan highlights a label). If stamped text is **not** snap-able, fallbacks
in order: fulgur-rendered running-header text (the spike showed running-header text
*renders* on every page; only link annotations were missing), then first-page-only
labels rendered in the HTML flow.

#### Readwise client additions (`src/readwise/`)

The `HttpTransport` seam is extended to carry method + optional body (today it is
GET-only). New high-level calls, all unit-tested against a fake transport:

- `update_location(id, ActionKind)` → `PATCH /api/v3/update/<id>/`
  (Inbox→`new`, Later→`later`, Archive→`archive`).
- `delete_document(id)` → `DELETE /api/v3/delete/<id>/`.
- `create_highlights(Vec<HighlightCreate>)` → `POST /api/v2/highlights/`, batched;
  each item carries `text`, `title`, `author`, `source_url` (from the embedded
  manifest) so Readwise matches it to the right document. A doc with no URL → cannot
  push its highlights → warn + skip.

#### Manifest — embedded in the PDF (`src/manifest.rs`, `src/postprocess.rs`)

The manifest is the read-back contract, and it lives **inside the generated PDF** so
the downloaded bundle is the single source of truth. No authoritative sidecar file.

Shape (compact JSON):

```json
{ "v": 1, "collection": "Library",
  "docs": [ { "id": "<readwise id>", "title": "...", "url": "...",
              "author": "...", "category": "article",
              "first_page": 3, "last_page": 5 } ] }
```

`ManifestItem` gains `page_range { first, last }`, `author`, `source_url`/`category`
(it already has `id`, `title`, `url`, `article_anchor`). `page_range` is filled in
`postprocess::finalize_pdf`, which already computes article start pages by resolving
index-row link destinations — extend it to emit ranges. The action-band rects are
constant across pages (we stamp them), so they are not stored.

**Embedding mechanism:** `postprocess` writes the JSON into the PDF via a
Flate-compressed stream referenced by a custom **Catalog** key (e.g.
`/RMReaderManifest`). It is fully under our control on both ends (write in
`postprocess`, read in `readback` with lopdf), invisible, and — because reMarkable
preserves the source PDF unchanged — present on download. The round-trip spike
confirms it survives; if reMarkable ever normalizes the PDF and drops custom catalog
keys, the fallback is a standard PDF **embedded-file attachment** (`/Names
/EmbeddedFiles`), which any conformant processor keeps.

A human-readable sidecar `*.manifest.json` may still be written to `output_dir` as a
**non-authoritative** debugging convenience; nothing reads it back.

#### Deploy additions (`src/deploy/`)

`Deployer` gains:

- `fetch(folder, name) -> anyhow::Result<Option<PathBuf>>` — `rmapi get` the doc's
  bundle into a temp dir; `Ok(None)` if it does not exist yet (first run).
- `replace(pdf, folder)` — `rmapi rm <folder>/<name>` (ignore "not found") then
  `rmapi put`. Replaces the existing default `refresh` behavior for this tool
  (full replace, not `--content-only`).

The `none` backend treats `output_dir` as the "device": `fetch` looks for a local
bundle (or returns `None`), `replace` overwrites the local PDF. The existing
token-clobber guard wraps the new rmapi calls too.

#### Config / cloud layout (`src/config.rs`, `src/wizard.rs`)

- Default deploy folders move under the `/RMDev` root: `library_folder` and
  `feed_folder` default to **`/RMDev/Reader`**. The wizard prompts with that default.
- Migration: existing configs pointing at `/Reader` keep working; Dan updates
  `danout/rmreader.toml` to `/RMDev/Reader` and the next sync creates the docs there
  (old `/Reader` copies removed manually once).
- `/RMDev` is a personal cloud convention shared across tools (Bujo, Reader, Files);
  documented here so sibling projects adopt the same root.

## Error handling

- No bundle on device (first run / deleted) → skip read-back, generate + upload only.
- Downloaded PDF has no embedded manifest, or it is unreadable → warn + skip read-back
  for that collection (still regenerate + upload). This is the only way the round-trip
  can break, and it is self-healing: the freshly uploaded PDF re-embeds the manifest.
- Non-v6 `.rm` page → warn + skip that page (`rmfiles` returns
  `UnsupportedVersion`).
- Highlight on a page absent from the embedded manifest → warn + skip.
- A Readwise op failing → log and continue the rest; never abort the whole sync.
- Doc with no URL → cannot push its highlights → warn + skip.
- Ambiguous action (≥2 labels) → skip + warn.
- rmapi failures → existing token-clobber guard.

## Testing (no manual testing except the one physical spike)

- **`rmfiles`:** unit tests against committed real Paper Pro fixtures — highlight
  text + rectangles + page index, multi-page documents, non-v6 header error,
  bundle-as-dir vs bundle-as-zip. Fixtures expanded as new cases surface.
- **Classification:** table tests over synthetic highlight inputs — action vs
  content, top-band guard, conflict skip+warn, page→doc mapping, missing-page skip.
- **Readwise client:** fake transport asserts method, URL, body, and auth header for
  `update_location` / `delete_document` / `create_highlights`.
- **Manifest:** assert `postprocess` fills correct `page_range`s for multi-page
  articles, and an in-process round trip — embed the manifest into a PDF, then read it
  back with the `readback` reader and assert equality (covers everything except
  reMarkable's preservation, which the round-trip spike covers).
- **Deploy:** fake `RmapiRunner` asserts the `get` and `rm`+`put` command sequences
  (and the `none` backend's local behavior).
- **On-device spike:** one-time physical confirmation that snap-to-text snaps to the
  stamped labels.

## Spikes (do first)

1. **`rmfiles` parser** — real `.rmdoc` fixture → `rmfiles` returns the exact
   highlighted strings + rects + page. Validates the format work and the premise.
2. **Snap-to-text on stamped labels** — same fixture; confirms the device snaps to
   our stamped label text. Drives the label-rendering choice (stamp vs running-header
   vs first-page-only).
3. **Embedded-manifest round trip** — generate a PDF with the embedded manifest,
   upload, annotate, `rmapi get`, and confirm the embedded manifest reads back intact
   from the downloaded original PDF. Validates the single-source-of-truth model and
   the embedding mechanism (catalog key vs embedded-file fallback).

All three spikes share a single captured fixture: a generated PDF (with embedded
manifest) that Dan annotates on the Paper Pro with a couple of known body highlights
and one action-label highlight, then captures via `rmapi get` and commits.

## Dependencies

- New crate `rmfiles`: `zip`, `serde` + `serde_json`, `thiserror`. Dev: `tempfile`.
- `rmreader`: add `rmfiles` (path dependency to `../rmfiles`). The embedded manifest
  uses `lopdf` (already a dep) to write/read the compressed stream — no new dep
  needed. Everything else reused.

## Future (designed-for, not built)

- Ink-stroke extraction in `rmfiles` (freehand checkboxes, drawings) — additive to
  the scene-item API.
- Exact device→PDF-point mapping using the `.content` fit transform, when a consumer
  needs geometry beyond relative position.
- Index-level / bulk triage; rendering Readwise highlights back into the PDF.
- `rmfiles` graduating to its own spec/lifecycle once a second project depends on it.
