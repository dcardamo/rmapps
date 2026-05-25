# inkapp — Spec #12: Multi-page device pull (assemble per-page ink from an `.rmdoc`)

**Date:** 2026-05-25
**Status:** Approved (design); plan pending.
**Goal:** close the last single-page assumption on the reader app's **real device
path**. Pagination (Spec #11) renders an article to N pages and `App::step` consumes
per-page ink (`HashMap<key, Vec<Vec<Stroke>>>`), but the reading-queue app's device
pull (`serve.rs`) still reads only the **first** `.rm` in a pulled `.rmdoc` and wraps
it as page 0 — so a user's annotations on pages 2..N are silently dropped. After this
spec, the device pull assembles **per-page** ink from a multi-page `.rmdoc`, indexed
by the bundle's `.content` page order, so ink on page *p* attributes to page *p* and no
ink is lost end-to-end against a real reMarkable.

This is the device-transport half of the "Documents, pages, and devices" promise that
[appdx.md](../../appdx.md#documents-pages-and-devices) and Spec #11 already make true
*inside* the framework.

## Why

The framework is fully per-page already; the gap is entirely in the app's transport:

- `serve.rs::strokes_from_rmdoc` does
  `names.into_iter().find(|n| n.ends_with(".rm"))` — it takes only the **first** `.rm`,
  and "first" is **zip-iteration order**, not the document's reading order.
- `serve.rs::pull_ink` then wraps that single page as `vec![strokes]` (page 0), with
  the explicit caveat *"multi-page rmdoc support is a future step."*

Everything downstream is already correct and must not change:

- `App::step` (runtime.rs) takes `ink_by_key: &HashMap<String, Vec<Vec<Stroke>>>` —
  per page — and preserves/accumulates it per page across cycles.
- `attribute` (readback.rs) is **page-aware**: a stroke on page *p* is tested **only**
  against regions with `region.page == p`, and it range-checks (`pages.get(region.page)`,
  skipping out-of-range pages). A *split* region (one breakable region spanning a page
  break) is stitched from every frame it touches into one `RegionInk` before decode.
- `render_document_in` records a single `page_h` per document — *"`#set page` fixes
  every page of a document to the same height"* — and that one height transforms every
  page. `pull_ink` already threads it via `page_h_by_key`.

So the per-page **indices** the pull assembles must be exactly right, or ink attributes
to the wrong page. That is the real work.

## What's already there (don't rebuild it)

- **The `.content` page-order reader exists.** `rm_files::Bundle::open(path)` opens an
  `.rmdoc` zip *or* an unpacked dir; `bundle.pages()` returns `Vec<Page>` in **`.content`
  reading order** (prefers the newer `cPages` list, falls back to legacy `pages`, and is
  null-tolerant for a freshly-deployed, never-opened doc). `Page::scene()` returns
  `Ok(None)` for an un-inked page. This is the authoritative page-order source — page
  order comes from `.content`, **not** filename/zip sort.
- **The device transform is proven.** `Remarkable::read_ink(bytes, page_h)` parses a
  `.rm` scene to PDF-space `Stroke`s **and** synthesizes the horizontal swipes for
  `GlyphRange` snap-to-text highlights (how Readwise highlights arrive). It is reused
  unchanged.

## Design

### 1. A pure assembly core, factored out of the rmapi shell-out

Replace `strokes_from_rmdoc` (first-`.rm`, single page) with a pure function that
assembles **per-page** ink in `.content` order:

```rust
/// Assemble per-page PDF-space strokes from an `.rmdoc` bundle, indexed by the
/// bundle's `.content` page order: slot `p` aligns with the manifest's
/// `region.page == p`. An un-inked page occupies its slot as an empty `Vec` (so it
/// never shifts later pages). Empty on a bundle that won't open.
pub fn strokes_by_page(device: &Remarkable, path: &Path, page_h: f64) -> Vec<Vec<Stroke>> {
    let Ok(bundle) = Bundle::open(path) else { return Vec::new(); };
    bundle
        .pages()
        .iter()
        .map(|pg| match pg.scene_bytes() {
            Some(bytes) => device.read_ink(bytes, page_h).unwrap_or_default(),
            None => Vec::new(),
        })
        .collect()
}
```

The crux is **`.map`, not `.filter_map`**: iterating `bundle.pages()` (every visible
page, in order) and emitting an empty `Vec` for an un-inked page keeps every later
page's index correct. The old code looked only at files that *had* a `.rm`, which both
dropped pages and lost order.

This has no `rmapi`, no network, and no filesystem dependency beyond reading the bundle
the caller points it at — so it is directly unit-testable against a synthesized fixture.

### 2. `pull_ink` calls the pure core

`pull_ink` keeps its signature (`page_h_by_key: &HashMap<String, f64>`) and its rmapi
`mget` shell-out. Per pulled `<key>.rmdoc` it calls
`strokes_by_page(device, &path, page_h)` and inserts the result into the output map
**only if some page carries ink** (preserving today's "skip empty docs" behaviour and
keeping the map tidy). The "future step" caveat comment is removed.

### 3. One tiny additive accessor in `rm-files`

`strokes_by_page` needs the **raw per-page `.rm` bytes** in `.content` order to feed
the proven `read_ink(bytes, page_h)`. `Bundle` already holds those bytes but exposes
only a parsed `Scene`. Add a one-line accessor (the only change outside
`apps/reading-queue`; `inkapp-core` and `inkapp-remarkable` are untouched):

```rust
impl Page<'_> {
    /// Raw `.rm` bytes for this page, if present (None = un-inked page).
    pub fn scene_bytes(&self) -> Option<&[u8]> {
        let key = format!("{}/{}.rm", self.bundle.uuid, self.id);
        self.bundle.files.get(&key).map(|v| v.as_slice())
    }
}
```

(`Page` lives in `bundle/mod.rs`, so it can read `Bundle`'s private `uuid`/`files`.)

### 4. Page-count: size to the bundle's page list

The outer `Vec` length is `bundle.pages().len()` — the device-authoritative list of
visible pages in `.content` order. Reconciliation with the rendered `page_count` is
**not** needed and not threaded in, because:

- `.content` already enumerates *all* visible pages (inked or not), so a doc that
  paginated to N pages and was opened on-device lists N entries; un-inked pages get an
  empty slot via the `None` arm. The list is already the right length.
- `attribute` range-checks `region.page` against `pages.len()`, so a list that is
  *shorter* than `page_count` (e.g. a freshly-pushed, never-opened doc whose `.content`
  is still `"pages":null` → zero pages, hence zero ink) is safe: regions on absent
  pages simply receive no ink, which is correct.

**Accepted limitation (documented in code):** a page the user *inserts in the middle*
on the tablet (mechanics §5 — its own `.content` entry, no PDF backing) shifts the
indices of pages after it relative to the rendered manifest. `content-only` push cannot
move it (§3–§4), and reflowing ink across such a structural divergence is out of scope
here; it is the same class of problem as Spec #11's deferred "ink reflow across
re-pagination." Append/grow/shrink at the **trailing** edge — the invariant the app
actually follows — stays correct.

### 5. Uniform page height

Confirmed already true and reused: one `page_h` per document (Typst `#set page`), so
`strokes_by_page` applies the caller's single `page_h` to every page. No change.

## Testing

A deterministic test with **no device and no `rmapi`** (the harness can't reach a real
tablet), in `apps/reading-queue/tests/multipage.rs`. It mirrors the
`pagination_device_blind` harness test's approach in reverse — that test *builds*
`Vec<Vec<Stroke>>` via `write_ink`+`read_ink`; this test routes the same bytes through
a synthesized `.rmdoc` and the new pull path.

Setup:

1. Build a logical document that paginates to **≥3 pages** with a region split across a
   break: `Body` (a `HighlightableText` of many tokens) + `Passage("notes", …)`
   (breakable, long enough to span a page break) + `Checkbox("done")`. Use a small
   `PageGeom` so it splits. Assert up front that `notes` recovers as **≥2 frames** and
   the doc is ≥3 pages (so there is a genuine un-inked middle page to test).
2. `render_document_in` → `manifest`, `page_h`, `page_count`.
3. Synthesize per-page `.rm` blobs with `Remarkable::write_ink`, placing ink in the
   `tok-N`, `notes`, and `done` rects on the pages those regions occupy — but
   **deliberately leave one middle page un-inked** (omit its `.rm` entirely).
4. Assemble a `.rmdoc` **zip** by hand: `<uuid>.content` (a hand-written JSON `cPages`
   list naming the page UUIDs in order — no `serde_json` dep needed) and
   `<uuid>/<page-uuid>.rm` for each inked page (the un-inked page contributes no `.rm`).
   Uses the existing `zip` dependency.

Assertions, via `serve::strokes_by_page` → `attribute` → component `decode`:

- **Per-page alignment:** ink written on page *k* attributes to a region whose
  `region.page == k`; the highlighted token decodes to the expected `Msg::Hi(word)`.
- **Split-region stitching:** the `notes` passage, inked on *both* frames it spans,
  attributes to one stitched `RegionInk` and decodes to a single `Msg::Note`.
- **Empty slot, no shift:** the un-inked middle page yields an empty `Vec` in its slot,
  and every later page's ink still attributes to the correct region (i.e. dropping the
  page's `.rm` does **not** shift subsequent pages — the bug this spec fixes).
- A complementary assertion that `strokes_by_page` returns a vec whose length equals
  `bundle.pages().len()`, with the expected empty/non-empty pattern per slot.

This proves the assembly end-to-end through the real `.rm` byte path and the real
`.content` order reader, without a device.

## Scope / non-goals

- **In scope:** `apps/reading-queue/src/serve.rs` (the pure core + `pull_ink`), one
  additive `Page::scene_bytes()` in `rm-files`, the new test, and removing the
  single-page caveat comment + correcting any single-page-pull implication in the docs.
- **Out of scope (unchanged):** `inkapp-core` (runtime/attribute/manifest per-page
  contract is done), `inkapp-remarkable` (`read_ink` reused as-is), and the
  connector/cache crates (`inkapp-readwise-reader`, `inkapp-core::cache`).
- **Deferred (named, not solved):** mid-document user-inserted pages shifting indices;
  ink reflow across re-pagination (shared with Spec #11); simultaneous
  *(doc × device)* fan-out.

## Definition of done

- `serve.rs` assembles per-page ink in `.content` order; the "future step" caveat is
  gone and `strokes_from_rmdoc`'s "first `.rm`" doc comment is corrected.
- `rm_files::Page::scene_bytes()` exists and is covered by the new test.
- `apps/reading-queue/tests/multipage.rs` proves per-page alignment, split-region
  stitching, and the empty-middle-page no-shift case.
- Any single-page-pull implication in `docs/` is corrected (grep `appdx.md`,
  `how-it-works.md`, `remarkable-pdf-mechanics.md`).
- `cargo test --workspace` is green. `Cargo.lock` staged if any dependency changes
  (none expected).
