# Multi-page pagination — design

**Status:** approved, ready for plan. **Spec #11.**
**Goal:** make inkapp deliver the pagination promise the docs already state —
*"You author a content flow, not pages. The framework paginates it, differently per
device."* Today the framework is single-page only. After this spec a content flow
renders to **N pages**, **N differs per device profile**, regions are recovered from
**every frame** they touch (including a region **split across a page break**), ink
comes back **per page** and is **stitched** back into content-relative regions, and
components stay **page-blind and device-blind** — proved by a test where the same
content paginates to different page counts on two profiles and decodes to identical
`Msg`s.

This is the "Documents, pages, and devices" section of
[appdx.md](../../appdx.md#documents-pages-and-devices) made true.

## Why

The vision is written down and the loop is built around it (components emit
content-relative regions; the manifest carries region geometry), but the
implementation pins everything to one page:

- `runtime.rs` hardcodes `DOC_PAGE_W`/`DOC_PAGE_H` (and a `16pt` margin) in
  `document_source`, so page geometry can't vary per device — the one input that
  makes pagination differ across devices.
- `render_document` takes `compiled.pages.first()` for `page_h` and never records a
  page count: the rest of the document's pages are invisible to the runtime.
- `attribute` (readback.rs) is **page-blind** — its own doc comment says callers
  must "pass only strokes from the page(s) the manifest covers." But `App::step`
  hands it a flat `Vec<Stroke>` per document with no page tags, and every page shares
  the same coordinate range (`0..w × 0..h`), so two pages' ink would cross-attribute
  to same-rect regions on the wrong page.
- The `#region` prelude anchors a region at a single `here().position()` with
  `measure(body)`'s full height. When a region's body flows across a page break it
  emits **one** rect on the start page (overhanging the page bottom) and **nothing**
  on the continuation page — silently wrong.

## What's already there (don't rebuild it)

Two pieces are further along than the single-page framing suggests, and the design
builds on them rather than replacing them:

- **`recover_regions` already iterates every frame.** It computes a `page_heights`
  vector over `doc.pages` and transforms each region with *its own* page's height.
  Region recovery is multi-frame at the geometry level today; what it lacks is the
  ability to turn one *split* region into several per-frame rects (added below).
- **Per-token (span-level) regions are already page-correct.** `HighlightableText`
  emits `page: here().position().page - 1` per token, so tokens on page 2 already
  recover against page 2's height. A token is a `#box` — atomic, never split — so
  span-level regions only need **page-aware attribution** to "survive" a break;
  their geometry is already right. `HighlightableText` needs **no change**.

## What Typst gives us, and the one thing it doesn't

- **`box` / non-breakable content is free.** The `#region` prelude wraps its body in
  a Typst `box`, which never breaks across pages. So every **affordance** region
  (checkbox, an event-row cancel, a task line) is *already* atomic — the
  `breakable:false` case — and needs nothing new.
- **Point introspection across a break is idiomatic.** `here().position()` /
  `locate(<label>).position()` return a correct `(page, x, y)` even across a
  `pagebreak()` (Typst docs, 0.14). The start/end-marker mechanism below is the
  blessed Typst way to bound content, not a hand-rolled hack.
- **What Typst will *not* do:** introspection is strictly point-based — every element
  has exactly one `location()`. A block that breaks across pages does **not** expose
  its per-page fragments or their bounding boxes to the scripting layer. There is no
  built-in "give me the N rects this paragraph occupied." So the per-frame rect
  arithmetic for a tall region has no Typst primitive and lives in **Rust**, where
  it is small and unit-testable.

## Design

### 1. Page geometry becomes a per-render input

Introduce a `PageGeom` and thread it through the render path, replacing the three
constants.

```rust
// geometry.rs
/// A document's page geometry, in points. Drives Typst `#set page` and lets the
/// content column width be computed (`w - 2·margin`) for full-width regions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGeom {
    pub w: f64,
    pub h: f64,
    pub margin: f64,
}

impl Default for PageGeom {
    /// The standard 3:4-ish e-ink profile (today's DOC_PAGE_W/H + 16pt margin).
    fn default() -> Self { Self { w: 420.0, h: 560.0, margin: 16.0 } }
}

impl PageGeom {
    /// The content column width (page width minus both margins).
    pub fn content_w(&self) -> f64 { self.w - 2.0 * self.margin }
}
```

- `document_source` / `compile_document` / `render_document` gain a `PageGeom`
  parameter. `document_source` emits `#set page(width: {w}pt, height: {h}pt,
  margin: {margin}pt)` from the geom.
- Keep the existing zero-geom entry points as thin convenience wrappers that pass
  `PageGeom::default()`, so single-page call sites change by one argument or not at
  all. The `DOC_PAGE_W`/`DOC_PAGE_H` consts are removed; `PageGeom::default()` is the
  single source of the standard profile.
- `App` carries a `geom: PageGeom` (default), set via a builder step
  `.page(PageGeom)`. A *device profile* in this spec **is** a `PageGeom` plus a
  `Device`; no new device crate is introduced (see §6).

> Scope note: the runtime stays **single-profile per app instance** (one `geom`).
> Simultaneous *(logical doc × device)* fan-out — multiple profiles rendered in one
> `DocSet`, ink tracked per device — is deliberately out of scope; it overlaps the
> "multi-device per user" axis the threat model marks future. Device-independence is
> proved in the harness (§6), not by runtime fan-out.

### 2. Two region classes: atomic (default) and breakable

The `#region` prelude grows one optional flag. The body is **boxed (atomic) by
default** and **flows (breakable) when asked**:

```typ
// region.typ
#let region(name, body, breakable: false) = {
  if not breakable {
    // Affordance / atomic region: a box never breaks → one rect, as today.
    box[
      #context [ #metadata((
        name: name, role: "box",
        page: here().position().page - 1,
        x: here().position().x / 1pt, y: here().position().y / 1pt,
        w: measure(body).width / 1pt, h: measure(body).height / 1pt,
      )) <region> ]
      #body
    ]
  } else {
    // Content / Capture region: emit a start marker, let the body flow (it may
    // break across pages), then an end marker. Rust reconstructs per-frame rects.
    context [ #metadata((
      name: name, role: "flow-start",
      page: here().position().page - 1,
      x: here().position().x / 1pt, y: here().position().y / 1pt,
      w: measure(body).width / 1pt,
    )) <region> ]
    body
    context [ #metadata((
      name: name, role: "flow-end",
      page: here().position().page - 1,
      x: here().position().x / 1pt, y: here().position().y / 1pt,
    )) <region> ]
  }
}
```

- **Atomic** (`role: "box"`, the default and today's behaviour): carries `w`,`h`;
  one rect on one page. Checkbox and any small affordance are unchanged.
- **Breakable** (`role: "flow-start"` + `"flow-end"`): two markers sharing a `name`.
  The start carries the column width `w`; the end carries only its position. The
  body is *not* boxed, so Typst breaks it naturally and `here()` after the body lands
  on whatever page/line the body ended on.
- The per-token `HighlightableText` markup (`role`-less `{name,page,x,y,w,h}`) stays
  valid and is treated as atomic (see §3 back-compat).

### 3. Split-aware region recovery (markers + Rust)

`recover_regions` learns to (a) accept the role-tagged schema and (b) reconstruct a
breakable region's per-frame rects from its start/end markers, using each frame's own
height.

```rust
// manifest.rs — RawRegion gains an optional role + optional w/h (flow-end has none).
#[derive(Deserialize)]
struct RawRegion {
    name: String,
    page: usize,
    x: f64,
    y: f64,
    #[serde(default)] role: Option<String>,   // "box" | "flow-start" | "flow-end" | None
    #[serde(default)] w: Option<f64>,
    #[serde(default)] h: Option<f64>,
}
```

Recovery algorithm:

1. Collect all `<region>` raw metadata and the per-page `page_heights` (as today).
2. **Atomic** rows (`role` is `None` or `"box"`; carry `w`,`h`): emit one `Region`
   with `typst_to_pdf_rect(x, y, w, h, page_heights[page])` — today's path,
   unchanged. This preserves checkbox and per-token recovery byte-for-byte.
3. **Flow** rows: pair `flow-start` with the `flow-end` of the same `name`. A pair
   spanning `start.page..=end.page` produces one `Region` per page `p`, all sharing
   `name`:
   - top in Typst space `= if p == start.page { start.y } else { 0.0 }`
   - bottom in Typst space `= if p == end.page { end.y } else { page_heights[p] }`
   - `x = start.x`, `w = start.w`
   - `rect = typst_to_pdf_rect(x, top, w, bottom - top, page_heights[p])`
   - `start.page == end.page` degenerates to a single rect (`top=start.y`,
     `bottom=end.y`) — the no-break case.

The rect-reconstruction step is a pure function (`split_rects(start, end,
&page_heights) -> Vec<Region>`) so it is unit-tested directly against synthetic
bounds and page heights, independent of Typst.

A breakable region therefore appears in `Manifest.regions` as **several `Region`
entries with the same `name`, one per frame it touches** — exactly the
"collect a region's geometry from every frame it touches" requirement.

### 4. Page-aware attribution + cross-page stitching

Ink enters the framework **per page** (one `.rm` per page → one stroke list per
page). `attribute` becomes page-aware and stitches a logical region's ink across the
pages it spans into a **single** `RegionInk`.

```rust
// readback.rs
/// Attribute per-page strokes to regions, then stitch each logical region's ink
/// across the pages it spans. `pages[p]` holds page p's strokes (that page's PDF
/// space). A stroke on page p is tested ONLY against regions with `region.page == p`,
/// so same-rect regions on different pages never cross-attribute. All ink for one
/// region `name` (across every frame it touches) is concatenated into one RegionInk.
pub fn attribute(pages: &[Vec<Stroke>], manifest: &Manifest) -> Vec<RegionInk> { … }
```

- For each `Region`, hit-test only `pages[region.page]` (empty if that page has no
  ink) by the existing point-in-rect rule.
- Accumulate matches into an order-preserving map `name -> Vec<Stroke>`; the first
  time a `name` is seen fixes its output order. Output one `RegionInk` per `name`,
  with strokes from **every** page that region spanned.
- A split region's two halves (page 4 + page 5) thus arrive at `decode` as one
  `RegionInk` — the component sees "the ink on me," page-blind. Components and their
  `read`/`decode` bodies are **unchanged** (they already group by `ri.region ==
  name` and flat-map strokes; stitching just guarantees one entry per name).
- A single-page convenience `attribute_page(strokes, manifest)` wraps
  `attribute(&[strokes], manifest)` for the many existing one-page call sites
  (readback/simulator tests).

### 5. Device path + runtime: per-page ink

- `RenderedDoc` and the runtime's `DocEntry` gain `page_count: usize`. `page_h` stays
  a single value — every page of a document shares `geom.h`, so the device transform
  (which already takes `page_h`) is unchanged; only the *number* of pages is new.
- Preserved ink in `DocSet` becomes **per page**: `DocEntry.ink: Vec<Vec<Stroke>>`,
  `DocSet::ink(key) -> &[Vec<Stroke>]`.
- `App::step`'s ink input becomes per page:
  `ink_by_key: &HashMap<String, Vec<Vec<Stroke>>>`. Inside `step`, attribution calls
  `attribute(pages, &entry.manifest)`; the fold, reconcile, and flush are unchanged.
  Ink preservation appends each page's new strokes to that page's preserved list.
- **The `Device` trait is unchanged.** The device still reads/writes one page's `.rm`
  at a time (`read_ink(bytes, page_h)`); the *caller* (harness driver / app sync)
  loops over pages and assembles `Vec<Vec<Stroke>>`. reMarkable's `read_ink`
  (including its snap-to-text highlight synthesis) is untouched.

### 6. The two-profile proof (tests)

A **device profile** here is a `PageGeom` + a `Device`. The second profile is
**synthetic page geometry** reusing reMarkable's self-consistent transform (the
transform is already parametric on page height) — no new hardware crate. Pick a
second `PageGeom` whose smaller usable height forces the same content onto **more
pages** (e.g. a shorter page), so a region that fits on page *k* of profile A lands
on page *k+1* of profile B and a tall region that doesn't break on A **splits** on B.

Tests, smallest first:

1. **Unit — split arithmetic** (`manifest.rs`/`geometry.rs`): `split_rects` over
   synthetic start/end bounds + page heights yields the expected rect set
   (no-break → 1 rect; 2-page → 2 rects with correct top/bottom; 3-page → middle
   page full-height).
2. **Recovery — a real break** (core test): a tall breakable `#region` rendered on a
   short page recovers as ≥2 `Region`s sharing one `name`, with PDF rects that tile
   the body across the frames (start page rect reaches the page bottom; end page rect
   starts at the top).
3. **Stitching** (core/harness test): per-page synthetic ink, some on each side of
   the break, attributes+stitches to a **single** `RegionInk` for that name carrying
   strokes from both pages.
4. **Device-blindness (the headline)** (harness test): one `Document` (a
   `HighlightableText` passage + a breakable `Passage` capture region + a
   `Checkbox`), rendered with profile A and profile B, paginates to **different page
   counts**. The same logical gestures (highlight a chosen token; ink the capture
   region; tick the checkbox) are mapped to each profile's per-page device ink,
   round-tripped through the real `.rm` write/read, attributed, stitched, and
   decoded. **Assert the decoded `Msg` sets are identical** across the two profiles.

To exercise a true single-region split with a real component, add a minimal
**`Passage`** Capture component to `inkapp-core::components`:

```rust
/// A breakable block of read-only text that captures any ink on it as one region
/// (Capture mode). Carries the value-message to emit when inked (no stored closure).
pub struct Passage<M = ()> { name: String, lines: Vec<String>, on_capture: M }
```

`render` emits `#region(name, breakable: true)[ … lines … ]`; `decode` emits
`on_capture` once if any ink landed in the (stitched) region. It is a legitimate,
reusable Capture component (carries a value message, per appdx's composition rule)
and is the vehicle that demonstrates split-stitch end to end.

## Scope — deliberately not in this spec

- **No simultaneous (doc × device) runtime fan-out.** One `geom` per app instance;
  device-independence proved in the harness. (Future: per-device ink streams in one
  `DocSet`.)
- **No new hardware device crate.** The second profile is synthetic page geometry.
- **No multi-page inspector.** The `simulate`/`inspect` harness helpers remain
  single-page debugging tools; the pagination proof runs through the
  `render → recover → attribute → decode` path (§6.4), not `simulate`. (Making the
  inspector stack N pages is a later nicety.)
- **No connector/cache work**, no edits to `inkapp-readwise*`, no
  `inkapp-core::cache` module (sibling-worktree territory). Edits to
  `inkapp-core/src/lib.rs` (only if `PageGeom`'s module export needs it) and
  `docs/appdx.md` stay minimal and localized.

## Acceptance criteria

- [ ] `PageGeom` exists; `DOC_PAGE_W`/`DOC_PAGE_H` are gone; `document_source` /
      `compile_document` / `render_document` are geometry-parametric with
      default-geom convenience wrappers; `App` carries a settable `geom`.
- [ ] The `#region` prelude supports `breakable: true`; the default (atomic) path is
      byte-identical to today (checkbox + per-token goldens unaffected).
- [ ] `recover_regions` reconstructs a breakable region into one `Region` per frame
      it touches (same `name`), each transformed with its own page height; atomic and
      per-token recovery are unchanged. `split_rects` is unit-tested in isolation.
- [ ] `attribute` is page-aware (`&[Vec<Stroke>]`), never cross-attributes between
      pages, and stitches a region's ink across pages into one `RegionInk`;
      `attribute_page` covers single-page callers.
- [ ] `RenderedDoc`/`DocEntry` carry `page_count`; `DocSet` ink is per page;
      `App::step` takes per-page ink. The `Device` trait is unchanged.
- [ ] A `Passage` Capture component exists and decodes stitched cross-page ink.
- [ ] The two-profile harness test renders identical content to **different page
      counts** and decodes **identical `Msg`s**; the split-recovery and stitching
      tests pass.
- [ ] `docs/appdx.md` marks pagination built (see below); the single-page caveats in
      `render.rs` ("Single-page only this spec") and `attribute`'s "one page per
      cycle" note are removed/updated.

## Verification

- `cargo test --workspace` is green. (Not `-p inkapp-core`: the page-geometry and
  `attribute`/`App::step` signature changes touch call sites in **both** `crates/`
  and `apps/` — enumerate them with a worktree-root `rg` and let the workspace build
  catch any missed app call site.)
- `cargo build --workspace` is clean; `Cargo.lock` is staged with any dep change.
- `rg "DOC_PAGE_W|DOC_PAGE_H"` returns nothing.
- The device-blindness test asserts the two profiles produced different
  `page_count`s **and** equal decoded `Msg` sets (so it can't pass by accidentally
  paginating the same).

## appdx update (definition of done)

`docs/appdx.md` is the project's definition of done. On completion:

- In **"Documents, pages, and devices"**, change the framing from promise to built:
  the framework paginates a content flow to N pages per device, recovers regions from
  every frame, and stitches per-page ink back into content-relative regions before
  decode. Keep the author-facing rules ("you never think in pages," regions are
  content-relative) — they're now backed by code.
- In the **Status** banner, note pagination is built (one logical doc → N-page
  render, device-parametric; *(doc × device)* fan-out remains the only future item
  alongside event-sourcing/CRDT and multi-user/cloud).
- Update the parking lot / FUTURE cross-reference so simultaneous per-device fan-out
  is the named remaining near-future refinement, not "single-page only."
</content>
</invoke>
