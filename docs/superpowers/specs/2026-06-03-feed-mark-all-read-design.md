# Feed "mark all as read" + index nav bar + tighter article chrome

**Date:** 2026-06-03
**Status:** Approved (design)

## Problem

The Readwise Feed PDF deployed to the reMarkable currently:

- Shows every fetched feed item, read or not.
- Offers no way to clear items you've finished with — the per-article
  INBOX/ARCHIVE/LATER/DELETE band moves an item's *location*, but the Feed has
  no "I'm done with this batch" gesture.
- Has no navigation chrome on the index ("first") page — the `< Prev | Home |
  Next >` bar only appears on article pages.
- Renders article pages with a large whitespace gap between the nav bar and the
  action band.

This work adds a Feed-only "mark all as read" button, filters the Feed to unread
items only, puts a paging nav bar on every index page (Library and Feed), and
tightens the article chrome.

## Background: how the system works today

- `crates/rmreader/src/generate.rs` fetches Library and Feed collections from the
  Readwise Reader API, builds one PDF per collection, embeds a manifest, and
  hands deploy targets back to `apps/rmapps/src/reader.rs`.
- `crates/rmreader/src/render/typst_doc.rs` emits the full Typst source: an
  **index page** (masthead = collection name + numbered rows) followed by full
  articles. A per-page `page-header()` draws the chrome — but only on article
  pages (`section-state == ""` on the index → no chrome).
- Read-back is **asynchronous and best-effort**: the user highlights a region
  on-device; on the *next* reader run, `reader::run` calls
  `readback::sync_collection` *before* regenerating, which downloads the deployed
  bundle, runs `classify()` over the highlights against the PDF's embedded
  manifest, and applies the resulting `Plan` to Readwise.
- Readwise represents read/unread with a **`seen` boolean** on a document
  (settable via the update endpoint). The Feed is fetched with `location=feed`.
- "Only mark those in the PDF" falls out for free: read-back keys off the doc ids
  embedded in *that* PDF's manifest, so articles that arrived after the PDF was
  generated are never in the manifest and can't be touched.

## Design

### 1. Feed shows only unread

Files: `crates/rmreader/src/readwise/mod.rs`, `crates/rmreader/src/generate.rs`

- Add `#[serde(default)] pub seen: bool` to `readwise::Document`. If the API omits
  the field, it defaults to `false` (treated as unread → safe no-op; no items are
  hidden).
- In `generate()`'s feed thread, after `drop_empty(...)`, filter
  `.filter(|d| !d.seen)`. Library is unaffected.
- Semantics: "unread within the newest N fetched." `fetch_documents` already
  returns the newest `feed.max_items` documents (newest-first, then truncated);
  we drop the seen ones from that page, so the Feed shows ≤ N unread items. No
  extra pagination.

### 2. Index nav bar on every index page (Library + Feed)

Files: `crates/rmreader/src/render/typst_doc.rs`

- Change `page-header()` so index pages (no active article section) render an
  **index nav bar** with the same indigo styling as the article bar:
  `< Prev | Home | Next >`.
- Prev/Next page through *index pages*. In Typst `context`:
  - current page = `here().position().page` (1-based).
  - first article page = `query(label("art-" + order.at(0))).first().location().page()`
    when `order` is non-empty; otherwise there are no articles and the whole
    document is index pages.
  - index pages span `1 ..= (firstArticlePage - 1)` (or `1 ..= finalPage` when
    there are no articles, via `counter(page).final()`).
  - Prev = `page - 1` if `page > 1`, else dimmed (inert).
  - Next = `page + 1` if `page < lastIndexPage`, else dimmed.
  - Prev/Next link via `link((page: n, x: 0pt, y: 0pt))[…]`; Home links to
    `<index-home>`.
  - A single-page index → both Prev and Next dimmed, Home active.
- The global (index) `set page` top margin grows from ~44pt to ~76pt to reserve
  room for the bar above the masthead while still clearing the reMarkable
  toolbar. The masthead and rows flow below the bar. Article pages keep their own
  120pt top margin set inside `#article` (adjusted in §4).

### 3. "MARK ALL AS READ" button under the Feed title

Files: `crates/rmreader/src/render/typst_doc.rs`,
`crates/rmreader/src/render/mod.rs`, `crates/rmreader/src/manifest.rs`,
`crates/rmreader/src/readback/classify.rs`,
`crates/rmreader/src/readback/mod.rs`, `crates/rmreader/src/readwise/mod.rs`

**Render (Feed index only).** In `build_index`, when the collection is the Feed,
emit a bordered button immediately after the masthead and before the rows,
wrapped in the existing `region("mark-all-read", …)` helper so it records its
0-based page + rect as `<region>` metadata. Library's index does not render it.
`build_index` therefore needs to know whether it is building the Feed (pass the
collection name / a flag, which it already receives).

**Manifest.** Add an optional region to `EmbeddedManifest`:

```rust
pub struct MarkAllReadRect {
    pub page: usize,            // 0-based
    pub rect: ManifestRect,     // PDF bottom-left origin
}
// EmbeddedManifest:
#[serde(default)]
pub mark_all_read: Option<MarkAllReadRect>,
```

`render::render_collection` extracts the `mark-all-read` region (first
occurrence), converting Typst top-left → PDF bottom-left exactly like the action
label rects, and stores it on the embedded manifest. Absent region → `None`
(Library, or a Feed with no button).

**Read-back / classify.** Add to `Plan`:

```rust
pub seen_doc_ids: Vec<String>,  // docs to PATCH seen=true
```

Detection (Feed only, gated on `manifest.mark_all_read.is_some()`):

- **Stroke path:** a highlighter stroke on the button's page whose bbox overlaps
  the button rect (x-overlap > 0 and center-y within the rect's y-band, mirroring
  the action-band geometry test) → set `seen_doc_ids = all manifest doc ids`.
  Such a stroke is consumed (not also emitted as a content highlight).
- **Text path:** a snap-to-text highlight whose text parses as the button label
  (`MARK ALL AS READ`, case-insensitive) → same result. This is checked before
  the per-doc page lookup so a hit on the index page (owned by no doc) is not
  warned away.

Because `seen_doc_ids` is exactly the manifest's doc ids, only the articles in
that PDF are marked — newer articles are excluded by construction.

**Readwise.** Add `mark_seen(t, token, ids: &[String])` sending `{"seen": true}`.
`execute()` calls it when `seen_doc_ids` is non-empty.

- **Endpoint choice:** prefer the **bulk update** endpoint (one PATCH for all
  ids) over looping the per-doc `update` endpoint, because the per-doc path is
  ~20 req/min rate-limited and a full feed page is up to 100 docs (≈5 min). The
  exact bulk request shape will be verified against the live Reader API during
  implementation; if a bulk endpoint is not available in the public API, fall
  back to per-id `PATCH /update/{id}/ {"seen": true}` (correct, just slower).

**Timing.** Read-back runs before regeneration in the same reader run
(`reader::run`), so items marked via the button are `seen=true` by the time the
feed is re-fetched and are filtered out of the freshly generated Feed PDF
(modulo Readwise's usual eventual consistency).

### 4. Tighter article chrome

Files: `crates/rmreader/src/render/typst_doc.rs`

- In `page-header()` for article pages, shrink the `v(20pt)` gap between the nav
  bar and the action band to ~`v(4pt)`.
- Reduce the reserved header block height and the article `set page` top margin
  (currently 112pt / 120pt) so the article body rises to sit just under the
  action band rather than leaving a large gap. Exact pt values are dialed in
  against a rendered page during implementation.

## Testing

- **classify** (`readback/classify.rs` unit tests):
  - Stroke overlapping the button rect on the button's page → `seen_doc_ids` =
    all manifest doc ids.
  - Snap-to-text hit with text `MARK ALL AS READ` → same.
  - Stroke elsewhere on the index page → `seen_doc_ids` empty (no false trigger);
    behaves as today.
  - Newer-doc exclusion is implicit (only manifest ids are ever added).
- **Feed filter** (`generate`/`readwise`): docs with `seen=true` dropped, those
  with `seen=false`/missing kept; Library list unaffected.
- **Render** (`render` tests / region recovery):
  - Feed index emits a `mark-all-read` region with a non-empty rect; Library
    index does not.
  - Index pages carry a nav bar; article inter-bar gap reduced.
- **mark_seen** request shape: assert method/URL/body against the `HttpTransport`
  seam, mirroring the existing `update_location` / `delete_document` tests.

## Out of scope

- No "mark all as read" on Library (no `seen` concept there).
- No new config keys.
- No change to the per-article INBOX/ARCHIVE/LATER/DELETE action band behavior.
