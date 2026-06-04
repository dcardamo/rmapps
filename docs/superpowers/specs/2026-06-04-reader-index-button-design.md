# Reader "Index" button — design

**Date:** 2026-06-04
**Scope:** `crates/rmreader/src/render/typst_doc.rs` (+ `tests/render.rs`)

## Problem

Each Reader collection (Feed, Library) renders as one Typst document: a
multi-page **index** (Fraunces masthead + numbered article rows) followed by the
full **articles**. Every article page carries a chrome header whose nav bar is
`< Prev | Home | Next >`. **Home** links to `<index-home>` — the very top of the
index (page 1).

When the index spans several pages, Home always returns you to page 1, so to get
back to where the current article was listed you must page forward through the
index. There is no direct way to jump to the index page that lists the article
you're reading.

## Goal

Add an **Index** button to the article nav bar that jumps to the specific index
page containing the current article's row — landing the reader exactly where
this article sits in the list. Applies to both Feed and Library (one shared
builder, so the change is universal).

## Design

All changes are confined to `typst_doc.rs`. No changes to deploy, region/
`page_range` recovery, the manifest, or `mark-all-read`.

### 1. Label each index row

In `build_index`, each row is currently `#link(label("art-{anchor}"))[…]` — a
link *to* the article. Attach a distinct label `idx-{anchor}` to the row element
so the row's page becomes queryable. The article headline continues to own
`art-{anchor}` (line ~246); the index row owns the new `idx-{anchor}`. The two
labels never collide.

### 2. Add the Index cell to `nav-bar()`

The article nav bar becomes a 4-column grid:

```
< Prev | Index | Home | Next >
```

`nav-bar()` runs in the article page context and already knows `sid` (the
current article id). The Index cell resolves the row page:

```typst
let idx = query(label("idx-" + sid))
let idx-page = if idx.len() == 0 { none } else { idx.first().location().page() }
```

and links to `(page: idx-page, x: 0pt, y: 0pt)`. Every article has exactly one
index row, so Index is always active; it is dimmed only defensively if the query
returns empty (which cannot happen for a rendered article).

### Out of scope / unchanged

- **`index-nav()`** (the bar shown *on* index pages) stays `< Prev | Home | Next >`.
  An "Index" button is meaningless when you are already in the index.
- **Home** stays — it remains the jump-to-top-of-list shortcut, distinct from
  Index when the index spans multiple pages.
- Action band, `mark-all-read` region, and `page_range`/region recovery are
  untouched.

### Edge cases

- **Single-page index:** Index and Home resolve to the same page — harmless.
- **Empty/missing row:** cannot occur for an article that exists; handled
  defensively by dimming the cell.

## Testing

In `tests/render.rs`, mirroring the existing link-annotation counting helpers:

- New test: an article page (page index 1) carries an Index Link annotation whose
  destination is the index page (page 0). Assert the article page has a link
  targeting page 0.
- Existing assertions must still pass unchanged: per-article `page_range`
  recovery, four action-band rects, `mark_all_read` present for Feed / absent for
  Library, clean text layer, and byte-determinism.
