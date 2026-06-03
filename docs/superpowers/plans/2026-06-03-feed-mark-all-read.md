# Feed "mark all as read" + index nav bar + tighter article chrome — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Feed-only "mark all as read" button that marks every article in the deployed PDF as `seen` in Readwise, filter the Feed to unread items only, put a paging nav bar on every index page (Library + Feed), and tighten the article-page chrome.

**Architecture:** Read-back stays asynchronous and best-effort — a highlight over the on-device button is detected on the *next* reader run and applied to Readwise. Detection keys off the doc ids embedded in *that* PDF's manifest, so newer articles are excluded by construction. Render chrome is drawn in-flow by Typst; the button is a recoverable `<region>` like the existing action-band cells.

**Tech Stack:** Rust, Typst 0.14 (in-flow PDF rendering), Readwise Reader API v3 (`bulk_update` with `{"updates":[{"id","seen"}]}`, max 50/request), lopdf manifest embed.

**User Verification:** YES — §2 (index nav bar) and §4 (chrome spacing) require a visual check on a rendered reMarkable-sized page; the spec explicitly defers exact pt values to "dialed in against a rendered page." Task 6 carries the verification block.

---

## File Structure

- `crates/rmreader/src/readwise/mod.rs` — add `Document.seen`; add `mark_seen()` (bulk_update) + `BULK_UPDATE_URL`.
- `crates/rmreader/src/generate.rs` — drop `seen` feed docs (new `drop_seen` helper).
- `crates/rmreader/src/render/typst_doc.rs` — index nav bar (`index-nav`), Feed-only `mark-all-read` button, tighter article header.
- `crates/rmreader/src/render/mod.rs` — recover the `mark-all-read` region into `Rendered`.
- `crates/rmreader/src/manifest.rs` — `MarkAllReadRect` + `EmbeddedManifest.mark_all_read`.
- `crates/rmreader/src/readback/classify.rs` — `Plan.seen_doc_ids`; detect button hits (stroke geometry + text label).
- `crates/rmreader/src/readback/mod.rs` — call `mark_seen` in `execute()`.
- Tests: `tests/readwise.rs`, `tests/classify.rs`, `tests/render.rs`, plus literal-site fixups in `tests/assemble.rs`, `tests/embed.rs`, `src/assemble.rs`, `examples/make_test_pdf.rs`, `examples/readback_overlay.rs`.

**Run all tests with:** `cargo test -p rmreader`

---

### Task 1: Feed shows only unread

**Goal:** Add a `seen` flag to `Document`, deserialize it from the API, and drop `seen=true` documents from the Feed (Library untouched).

**Files:**
- Modify: `crates/rmreader/src/readwise/mod.rs` (Document struct, ~line 166-197)
- Modify: `crates/rmreader/src/generate.rs` (feed thread, ~line 312-329; add `drop_seen`)
- Modify: `crates/rmreader/src/assemble.rs:235` (test `doc()` helper — add field)
- Modify: `crates/rmreader/tests/assemble.rs:6` (test `doc()` helper — add field)
- Modify: `crates/rmreader/examples/make_test_pdf.rs:27,49` (Document literals — add field)
- Test: `crates/rmreader/tests/readwise.rs`, `crates/rmreader/tests/generate.rs`

**Acceptance Criteria:**
- [ ] `Document` has `seen: bool` (defaults `false` when absent from JSON).
- [ ] `drop_seen` removes `seen=true` docs and keeps the rest in order.
- [ ] Feed generation drops seen docs; Library generation does not call `drop_seen`.
- [ ] `cargo test -p rmreader` passes.

**Verify:** `cargo test -p rmreader seen` → new tests pass; `cargo build -p rmreader --examples` compiles.

**Steps:**

- [ ] **Step 1: Add the `seen` field to `Document`**

In `crates/rmreader/src/readwise/mod.rs`, inside `pub struct Document` (after the `location` field, ~line 183), add:

```rust
    /// Readwise "read" flag. Absent on older docs → false (treated as unread).
    #[serde(default)]
    pub seen: bool,
```

- [ ] **Step 2: Write the failing test for `seen` deserialization**

Append to `crates/rmreader/tests/readwise.rs`:

```rust
#[test]
fn seen_field_parses_true_false_and_missing() {
    let body = r#"{"nextPageCursor":null,"results":[
        {"id":"a","title":"A","saved_at":"2026-01-03T00:00:00Z","seen":true,"html_content":"<p>x</p>"},
        {"id":"b","title":"B","saved_at":"2026-01-02T00:00:00Z","seen":false,"html_content":"<p>x</p>"},
        {"id":"c","title":"C","saved_at":"2026-01-01T00:00:00Z","html_content":"<p>x</p>"}
    ]}"#;
    let fake = Fake {
        calls: RefCell::new(vec![]),
        script: vec![(200, None, body.to_string())],
        idx: RefCell::new(0),
    };
    let docs = fetch_documents(&fake, "tok", &["feed".into()], 10, |_| {}).unwrap();
    // newest-first by saved_at: a, b, c
    assert!(docs[0].seen, "a should be seen");
    assert!(!docs[1].seen, "b should be unseen");
    assert!(!docs[2].seen, "c (missing field) defaults to unseen");
}
```

- [ ] **Step 3: Run it — expect a compile/pass cycle**

Run: `cargo test -p rmreader --test readwise seen_field_parses` → expected PASS once Step 1 is in (the field exists). If it fails to compile, fix the field.

- [ ] **Step 4: Add the `drop_seen` helper + failing test**

In `crates/rmreader/src/generate.rs`, add next to `drop_empty` (after line 110):

```rust
/// Drop documents Readwise marks as read (`seen == true`). Feed-only: the Library
/// has no "seen" concept, so its build path must not call this.
fn drop_seen(docs: Vec<crate::readwise::Document>) -> Vec<crate::readwise::Document> {
    docs.into_iter().filter(|d| !d.seen).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::readwise::Document;

    fn d(id: &str, seen: bool) -> Document {
        Document {
            id: id.into(),
            url: String::new(),
            source_url: String::new(),
            title: id.into(),
            author: String::new(),
            site_name: String::new(),
            category: "article".into(),
            location: "feed".into(),
            seen,
            summary: String::new(),
            image_url: String::new(),
            word_count: None,
            reading_time: None,
            published_date: None,
            saved_at: "2026-01-01T00:00:00Z".into(),
            html_content: Some("<p>x</p>".into()),
        }
    }

    #[test]
    fn drop_seen_keeps_only_unread_in_order() {
        let out = drop_seen(vec![d("a", false), d("b", true), d("c", false)]);
        assert_eq!(
            out.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "c"]
        );
    }
}
```

- [ ] **Step 5: Run the helper test — expect compile failure first**

Run: `cargo test -p rmreader --lib drop_seen_keeps_only_unread`
Expected: FAILS to compile until Step 1 added `seen` AND the test `d()` literal matches the struct. Once it compiles: PASS.

- [ ] **Step 6: Wire `drop_seen` into the feed thread**

In `crates/rmreader/src/generate.rs` feed thread, change the `drop_empty` line (~line 322) from:

```rust
                let feed = drop_empty(feed);
```

to:

```rust
                let feed = drop_seen(drop_empty(feed));
```

Leave the library thread's `drop_empty(lib)` (line 307) unchanged — Library must NOT drop seen.

- [ ] **Step 7: Fix the other `Document` literal sites (compile fixups)**

Add `seen: false,` to each literal:
- `crates/rmreader/src/assemble.rs:235` `fn doc(...)` (after `location: "new".into(),`)
- `crates/rmreader/tests/assemble.rs:6` `fn doc(...)`
- `crates/rmreader/examples/make_test_pdf.rs:27` and `:49`

(Place it adjacent to the existing `location` field in each.)

- [ ] **Step 8: Run full crate tests + examples build**

Run: `cargo test -p rmreader && cargo build -p rmreader --examples`
Expected: all pass / compiles.

- [ ] **Step 9: Commit**

```bash
git add crates/rmreader/src/readwise/mod.rs crates/rmreader/src/generate.rs \
        crates/rmreader/src/assemble.rs crates/rmreader/tests/assemble.rs \
        crates/rmreader/tests/readwise.rs crates/rmreader/examples/make_test_pdf.rs
git commit -m "feat(rmreader): filter Feed to unread (seen=false) documents"
```

```json:metadata
{"files": ["crates/rmreader/src/readwise/mod.rs", "crates/rmreader/src/generate.rs", "crates/rmreader/src/assemble.rs", "crates/rmreader/tests/readwise.rs"], "verifyCommand": "cargo test -p rmreader && cargo build -p rmreader --examples", "acceptanceCriteria": ["Document.seen parses with default false", "drop_seen filters seen=true", "feed drops seen, library does not"], "requiresUserVerification": false}
```

---

### Task 2: Index nav bar on every index page (paging)

**Goal:** Draw a `< Prev | Home | Next >` bar on index pages (Library + Feed) that pages through index pages; both ends dimmed on a single-page index.

**Files:**
- Modify: `crates/rmreader/src/render/typst_doc.rs` (`page-header`, ~line 184-193; global `set page` top margin, ~line 224; add `index-nav` helper)
- Test: `crates/rmreader/tests/render.rs`

**Acceptance Criteria:**
- [ ] Index pages render an indigo nav bar with Home (→ `<index-home>`) plus Prev/Next.
- [ ] Prev is dimmed on the first index page; Next is dimmed on the last index page (single-page index → both dimmed).
- [ ] The Typst source compiles and the PDF still has the expected page count.
- [ ] `cargo test -p rmreader --test render` passes.

**Verify:** `cargo test -p rmreader --test render` → PASS; `RMREADER_DUMP_TYPST=1 cargo test -p rmreader --test render renders_internal_links` then inspect `/tmp/rmreader_Feed.typ`.

**Steps:**

- [ ] **Step 1: Add the `index-nav` helper**

In `crates/rmreader/src/render/typst_doc.rs`, in the preamble block (after the `nav-bar()` definition, before `action-band()`, ~line 163), insert. Note the doubled `{{`/`}}` because this is inside a Rust `format!` raw string:

```rust
// Index nav bar: < Prev | Home | Next >, paging through index pages. Index pages
// are pages 1..=(firstArticlePage-1) (1-based), or all pages when there are no
// articles. Prev/Next link to absolute page positions; inert cells are dimmed.
#let index-nav() = context {{
  let p = here().position().page
  let first-art = if order.len() == 0 {{ none }} else {{
    let m = query(label("art-" + order.at(0)))
    if m.len() == 0 {{ none }} else {{ m.first().location().page() }}
  }}
  let last-index = if first-art == none {{ counter(page).final().first() }} else {{ first-art - 1 }}
  let prev = if p > 1 {{ p - 1 }} else {{ none }}
  let next = if p < last-index {{ p + 1 }} else {{ none }}
  let cell(txt, target) = align(center + horizon, if target == none {{
    text(font: "Hanken Grotesk", size: 8pt, weight: "semibold", tracking: 0.04em,
      fill: navfg.transparentize(55%), txt)
  }} else {{
    link((page: target, x: 0pt, y: 0pt),
      text(font: "Hanken Grotesk", size: 8pt, weight: "semibold", tracking: 0.04em,
        fill: navfg, txt))
  }})
  let home = align(center + horizon, link(<index-home>,
    text(font: "Hanken Grotesk", size: 8pt, weight: "semibold", tracking: 0.04em,
      fill: navfg, "Home")))
  block(width: 100%, height: 24pt, fill: navbg, inset: (x: 10pt),
    align(horizon, grid(columns: (1fr, 1fr, 1fr),
      cell([< Prev], prev), home, cell([Next >], next))))
}}
```

- [ ] **Step 2: Render the index bar in `page-header`**

Replace the `page-header` helper (~line 184-193) with:

```rust
// Per-page chrome. Article pages get nav bar + action band (tall reserved
// header). Index pages get just the paging nav bar, below the toolbar-clearance
// gap, above the masthead.
#let page-header() = context {{
  if section-state.at(here()) == "" {{
    block(width: 100%)[
      #v(34pt, weak: false)
      #index-nav()
    ]
  }} else {{
    block(width: 100%, height: 96pt)[
      #v(34pt, weak: false)
      #nav-bar()
      #v(4pt, weak: false)
      #action-band()
    ]
  }}
}}
```

(The article branch's reduced spacing here is finalized in Task 6 — leaving it as `96pt`/`v(4pt)` now is fine; Task 6 only tunes values.)

- [ ] **Step 3: Grow the index top margin to reserve room for the bar**

In the global `#set page(...)` (~line 222-229), change `margin: (top: 44pt, ...)` to:

```rust
  margin: (top: 76pt, right: 16pt, bottom: 30pt, left: 16pt),
```

- [ ] **Step 4: Add a render test asserting an index-page link**

Append to `crates/rmreader/tests/render.rs` a helper that counts Link annots on a specific page, then a test. Add at the bottom of the file:

```rust
/// Count Link annotations on a single 0-based page index.
fn links_on_page(pdf: &[u8], page_index: usize) -> usize {
    let doc = lopdf::Document::load_mem(pdf).unwrap();
    let pages: Vec<_> = doc.get_pages().into_values().collect();
    let pid = pages[page_index];
    let mut n = 0;
    if let Ok(annots) = doc
        .get_dictionary(pid)
        .and_then(|p| p.get(b"Annots"))
        .and_then(|a| a.as_array())
    {
        for a in annots {
            if let Ok(ad) = a.as_reference().and_then(|id| doc.get_dictionary(id)) {
                if ad.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) == Some(b"Link") {
                    n += 1;
                }
            }
        }
    }
    n
}

#[test]
fn index_page_has_nav_bar_links() {
    let device = get_device("paper-pro-move").unwrap();
    let theme = load_theme("reader").unwrap();
    let (rows, articles) = sample();
    let r = render_collection(&device, &theme, "Feed", &rows, &articles, &[]).unwrap();
    // Index is page 0. Home always links; with articles present, Next links too.
    // The two index rows also link. So the index page has multiple Link annots.
    assert!(
        links_on_page(&r.pdf, 0) >= 3,
        "index page should carry Home + Next + row links, got {}",
        links_on_page(&r.pdf, 0)
    );
}
```

- [ ] **Step 5: Run render tests**

Run: `cargo test -p rmreader --test render`
Expected: all PASS (existing 3 tests + the new one). If the Typst fails to compile (e.g. position-dict link unsupported), dump and inspect: `RMREADER_DUMP_TYPST=1 cargo test -p rmreader --test render renders_internal_links` → read `/tmp/rmreader_Feed.typ` and the compile error.

- [ ] **Step 6: Commit**

```bash
git add crates/rmreader/src/render/typst_doc.rs crates/rmreader/tests/render.rs
git commit -m "feat(rmreader): paging nav bar on index pages"
```

```json:metadata
{"files": ["crates/rmreader/src/render/typst_doc.rs", "crates/rmreader/tests/render.rs"], "verifyCommand": "cargo test -p rmreader --test render", "acceptanceCriteria": ["index pages render nav bar", "Prev/Next page through index pages", "index page carries Home/Next links"], "requiresUserVerification": false}
```

---

### Task 3: "MARK ALL AS READ" button on the Feed index + manifest recovery

**Goal:** Render a bordered, recoverable button under the Feed masthead (Feed only), recover its rect into the embedded manifest.

**Files:**
- Modify: `crates/rmreader/src/render/typst_doc.rs` (`MARK_ALL_READ_LABEL` const; `build` passes a flag to `build_index`; emit `region("mark-all-read", …)`)
- Modify: `crates/rmreader/src/manifest.rs` (`MarkAllReadRect`, `EmbeddedManifest.mark_all_read`, fixup `to_embedded`)
- Modify: `crates/rmreader/src/render/mod.rs` (`Rendered.mark_all_read`, recover region)
- Modify: `crates/rmreader/src/generate.rs` (set `embedded.mark_all_read`)
- Modify: `crates/rmreader/tests/classify.rs:11,310`, `crates/rmreader/tests/embed.rs:6`, `crates/rmreader/examples/readback_overlay.rs:224` (add `mark_all_read: None`)
- Test: `crates/rmreader/tests/render.rs`

**Acceptance Criteria:**
- [ ] `EmbeddedManifest` has `#[serde(default)] mark_all_read: Option<MarkAllReadRect>` (page + PDF-coords rect).
- [ ] The Feed index renders a `mark-all-read` region; `render_collection` recovers its rect/page into `Rendered`; `generate` stores it on the embedded manifest.
- [ ] The Library index renders NO such region (`mark_all_read == None`).
- [ ] `cargo test -p rmreader` passes.

**Verify:** `cargo test -p rmreader --test render mark_all_read` → PASS.

**Steps:**

- [ ] **Step 1: Add `MarkAllReadRect` + manifest field**

In `crates/rmreader/src/manifest.rs`, after the `LabelRect` struct (~line 96), add:

```rust
/// The "mark all as read" button's tap region on the Feed index page.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MarkAllReadRect {
    pub page: usize, // 0-based
    pub rect: ManifestRect,
}
```

In `pub struct EmbeddedManifest` (~line 113), after the `label_rects` field, add:

```rust
    /// Feed-only "mark all as read" button region; None for Library / when absent.
    #[serde(default)]
    pub mark_all_read: Option<MarkAllReadRect>,
```

In `to_embedded` (~line 44-66), add `mark_all_read: None,` after `label_rects: Vec::new(),` (postprocess/generate fills it).

- [ ] **Step 2: Fix the other `EmbeddedManifest` literal sites**

Add `mark_all_read: None,` to each literal (after their `label_rects` field):
- `crates/rmreader/tests/classify.rs:11` (`manifest()`)
- `crates/rmreader/tests/classify.rs:310` (`manifest_with_category`)
- `crates/rmreader/tests/embed.rs:6` (`sample()`)
- `crates/rmreader/examples/readback_overlay.rs:224` (`empty_manifest`)

- [ ] **Step 3: Add the button label const + render it (Feed only)**

In `crates/rmreader/src/render/typst_doc.rs`, near `ACTION_LABELS` (~line 31), add:

```rust
/// Text + recoverable-region name for the Feed-only "mark all as read" button.
pub const MARK_ALL_READ_LABEL: &str = "MARK ALL AS READ";
```

Change `build_index` to know whether it's the Feed. Update its signature (~line 252) from `fn build_index(collection: &str, rows: &[Row])` to:

```rust
fn build_index(collection: &str, rows: &[Row], feed: bool) -> String {
```

And its call site in `build` (~line 236) from `s.push_str(&build_index(collection, rows));` to:

```rust
    s.push_str(&build_index(collection, rows, collection == "Feed"));
```

In `build_index`, immediately after the masthead `push_str` (the block that ends with `count = rows.len(),` ~line 265) and before the `for r in rows` loop, insert:

```rust
    // Feed-only "mark all as read" button: a bordered, tappable, recoverable
    // region under the masthead. region(...) records its page+rect as <region>
    // metadata, recovered by render::render_collection into the manifest.
    if feed {
        s.push_str(&format!(
            "#block(above: 4pt, below: 10pt, region(\"mark-all-read\", \
             box(stroke: 0.8pt + heading-col, inset: (x: 10pt, y: 6pt), radius: 2pt, \
             text(font: \"Hanken Grotesk\", size: 9pt, weight: \"semibold\", \
             tracking: 0.12em, fill: heading-col, [{label}]))))\n",
            label = esc_markup(MARK_ALL_READ_LABEL),
        ));
    }
```

- [ ] **Step 4: Recover the region into `Rendered`**

In `crates/rmreader/src/render/mod.rs`, add a field to `pub struct Rendered` (~line 79-85):

```rust
    /// Feed-only "mark all as read" button region (page + PDF-coords rect), if present.
    pub mark_all_read: Option<crate::manifest::MarkAllReadRect>,
```

After the `label_rects` loop (~line 150), before `Ok(Rendered { ... })`, add:

```rust
    // Mark-all-read button: first occurrence of the mark-all-read region,
    // converted Typst top-left → PDF bottom-left like the action rects.
    let mark_all_read = regions
        .iter()
        .find(|r| r.name == "mark-all-read")
        .map(|r| crate::manifest::MarkAllReadRect {
            page: r.page,
            rect: crate::manifest::ManifestRect {
                x0: r.x,
                y0: page_h - (r.y + r.h),
                x1: r.x + r.w,
                y1: page_h - r.y,
            },
        });
```

And add `mark_all_read,` to the returned `Rendered { ... }`.

- [ ] **Step 5: Store it on the embedded manifest in `generate`**

In `crates/rmreader/src/generate.rs`, after the `embedded.label_rects = rendered.label_rects;` line (~line 251), add:

```rust
    embedded.mark_all_read = rendered.mark_all_read;
```

(Place it before the `let mut pdf_doc = ...` line. Note `rendered.label_rects` is moved on the prior line; `mark_all_read` is `Copy` via `Option<MarkAllReadRect>` so order doesn't matter, but keep this line immediately after for clarity.)

- [ ] **Step 6: Add render tests (Feed has it, Library doesn't)**

Append to `crates/rmreader/tests/render.rs`:

```rust
#[test]
fn feed_index_emits_mark_all_read_region() {
    let device = get_device("paper-pro-move").unwrap();
    let theme = load_theme("reader").unwrap();
    let (rows, articles) = sample();
    let r = render_collection(&device, &theme, "Feed", &rows, &articles, &[]).unwrap();
    let m = r.mark_all_read.expect("Feed must emit a mark-all-read region");
    assert_eq!(m.page, 0, "button is on the index page");
    assert!(m.rect.x1 > m.rect.x0 && m.rect.y1 > m.rect.y0, "rect must be non-empty: {m:?}");
}

#[test]
fn library_index_has_no_mark_all_read_region() {
    let device = get_device("paper-pro-move").unwrap();
    let theme = load_theme("reader").unwrap();
    let (rows, articles) = sample();
    let r = render_collection(&device, &theme, "Library", &rows, &articles, &[]).unwrap();
    assert!(r.mark_all_read.is_none(), "Library must not render the button");
}
```

- [ ] **Step 7: Run full crate tests + examples build**

Run: `cargo test -p rmreader && cargo build -p rmreader --examples`
Expected: all PASS / compiles.

- [ ] **Step 8: Commit**

```bash
git add crates/rmreader/src/manifest.rs crates/rmreader/src/render/typst_doc.rs \
        crates/rmreader/src/render/mod.rs crates/rmreader/src/generate.rs \
        crates/rmreader/tests/render.rs crates/rmreader/tests/classify.rs \
        crates/rmreader/tests/embed.rs crates/rmreader/examples/readback_overlay.rs
git commit -m "feat(rmreader): Feed-only mark-all-read button + manifest region"
```

```json:metadata
{"files": ["crates/rmreader/src/manifest.rs", "crates/rmreader/src/render/typst_doc.rs", "crates/rmreader/src/render/mod.rs", "crates/rmreader/src/generate.rs", "crates/rmreader/tests/render.rs"], "verifyCommand": "cargo test -p rmreader && cargo build -p rmreader --examples", "acceptanceCriteria": ["EmbeddedManifest.mark_all_read added", "Feed index emits region recovered into manifest", "Library index emits none"], "requiresUserVerification": false}
```

---

### Task 4: Classify button highlights → `seen_doc_ids`

**Goal:** A highlight over the button (stroke geometry on its page, or a snap-to-text hit of its label) sets `Plan.seen_doc_ids` to every manifest doc id.

**Files:**
- Modify: `crates/rmreader/src/readback/classify.rs` (`Plan.seen_doc_ids`; detection in `classify`)
- Test: `crates/rmreader/tests/classify.rs`

**Acceptance Criteria:**
- [ ] `Plan` has `seen_doc_ids: Vec<String>` (default empty).
- [ ] A stroke whose center is inside the button rect on the button's page → `seen_doc_ids` = all manifest ids; the stroke is NOT also a content highlight.
- [ ] A text hit whose text parses as `MARK ALL AS READ` (case-insensitive) → same, regardless of page (not warned as off-manifest).
- [ ] A non-button stroke on the index page behaves exactly as today (warns / no false trigger).
- [ ] `cargo test -p rmreader --test classify` passes.

**Verify:** `cargo test -p rmreader --test classify` → PASS.

**Steps:**

- [ ] **Step 1: Add the `seen_doc_ids` field to `Plan`**

In `crates/rmreader/src/readback/classify.rs`, in `pub struct Plan` (~line 48-53), add:

```rust
    /// Doc ids to mark seen=true (Feed "mark all as read"). Empty = no-op.
    pub seen_doc_ids: Vec<String>,
```

- [ ] **Step 2: Add a label-parse + rect-hit helper near the top of `classify.rs`**

After the `x_overlap` fn (~line 58), add:

```rust
/// True when the text is the "mark all as read" button label (case-insensitive,
/// whitespace-trimmed/collapsed).
fn is_mark_all_read_label(s: &str) -> bool {
    let norm = s.split_whitespace().collect::<Vec<_>>().join(" ");
    norm.eq_ignore_ascii_case(crate::render::typst_doc::MARK_ALL_READ_LABEL)
}

/// True when a stroke bbox center falls inside the button rect on its page.
fn hits_mark_all_read(m: &EmbeddedManifest, page: usize, bbox: &PdfRect) -> bool {
    match &m.mark_all_read {
        Some(b) if b.page == page => {
            let cx = (bbox.x0 + bbox.x1) / 2.0;
            let cy = (bbox.y0 + bbox.y1) / 2.0;
            let r = &b.rect;
            cx >= r.x0 && cx <= r.x1 && cy >= r.y0 && cy <= r.y1
        }
        _ => false,
    }
}
```

(Add `use crate::manifest::EmbeddedManifest;` already present at top — confirm; `PdfRect` is imported.)

- [ ] **Step 3: Detect in the text path**

In `classify`, at the very start of the `for hit in text_hits` loop (before the `doc_for_page` lookup, ~line 94), add:

```rust
        if is_mark_all_read_label(&hit.text) {
            plan.seen_doc_ids = m.docs.iter().map(|d| d.id.clone()).collect();
            continue;
        }
```

- [ ] **Step 4: Detect in the stroke path**

In `classify`, at the very start of the `for hit in hits` loop (before the `doc_for_page` lookup, ~line 126), add:

```rust
        if hits_mark_all_read(m, hit.page, &hit.bbox) {
            plan.seen_doc_ids = m.docs.iter().map(|d| d.id.clone()).collect();
            continue;
        }
```

- [ ] **Step 5: Write the tests**

Append to `crates/rmreader/tests/classify.rs`. First, a manifest variant with a button rect on the index page (page reuse: put button on a page NOT in any doc range to mirror production — use page 5, rect x=[20,160] y=[420,440]):

```rust
use rmreader::manifest::MarkAllReadRect;

/// Manifest with a mark-all-read button at page 5, rect x=[20,160], y=[420,440].
fn manifest_with_button() -> EmbeddedManifest {
    let mut m = manifest();
    m.collection = "Feed".into();
    m.mark_all_read = Some(MarkAllReadRect {
        page: 5,
        rect: ManifestRect { x0: 20.0, y0: 420.0, x1: 160.0, y1: 440.0 },
    });
    m
}

#[test]
fn stroke_on_button_marks_all_docs_seen() {
    // center (90,430) inside rect x[20,160] y[420,440] on page 5.
    let p = classify(&manifest_with_button(), &[], &[hit(5, 70.0, 425.0, 110.0, 435.0)], |_, _| String::new());
    assert_eq!(p.seen_doc_ids, vec!["d1".to_string(), "d2".to_string()]);
    assert!(p.highlights.is_empty(), "button stroke must not become a highlight");
    assert!(p.warnings.is_empty(), "button stroke must not warn off-manifest");
}

#[test]
fn text_label_marks_all_docs_seen() {
    let p = classify(&manifest_with_button(), &[thit(5, "mark all as read")], &[], |_, _| String::new());
    assert_eq!(p.seen_doc_ids, vec!["d1".to_string(), "d2".to_string()]);
    assert!(p.highlights.is_empty());
    assert!(p.warnings.is_empty());
}

#[test]
fn non_button_stroke_on_button_page_does_not_trigger() {
    // A stroke elsewhere on page 5 (center below the button band) → no seen ids.
    // page 5 is not in any doc range → it warns (today's behavior), seen stays empty.
    let p = classify(&manifest_with_button(), &[], &[hit(5, 70.0, 100.0, 110.0, 120.0)], |_, _| String::new());
    assert!(p.seen_doc_ids.is_empty());
    assert_eq!(p.warnings.len(), 1, "off-manifest stroke still warns as before");
}
```

- [ ] **Step 6: Run classify tests**

Run: `cargo test -p rmreader --test classify`
Expected: all PASS (existing + 3 new).

- [ ] **Step 7: Commit**

```bash
git add crates/rmreader/src/readback/classify.rs crates/rmreader/tests/classify.rs
git commit -m "feat(rmreader): classify mark-all-read highlights into seen_doc_ids"
```

```json:metadata
{"files": ["crates/rmreader/src/readback/classify.rs", "crates/rmreader/tests/classify.rs"], "verifyCommand": "cargo test -p rmreader --test classify", "acceptanceCriteria": ["Plan.seen_doc_ids added", "stroke on button → all ids", "text label → all ids", "non-button stroke does not trigger"], "requiresUserVerification": false}
```

---

### Task 5: Readwise `mark_seen` (bulk_update) + execute wiring

**Goal:** Send `seen=true` for the doc ids via `PATCH /api/v3/bulk_update/` in chunks of ≤50, and call it from `execute()`.

**Files:**
- Modify: `crates/rmreader/src/readwise/mod.rs` (`BULK_UPDATE_URL`, `mark_seen`)
- Modify: `crates/rmreader/src/readback/mod.rs` (`execute` calls `mark_seen`)
- Test: `crates/rmreader/tests/readwise.rs`

**Acceptance Criteria:**
- [ ] `mark_seen(t, token, ids)` issues `PATCH https://readwise.io/api/v3/bulk_update/` with body `{"updates":[{"id":"<id>","seen":true}, …]}`.
- [ ] Empty ids → zero HTTP calls.
- [ ] >50 ids → multiple requests, each ≤50 items.
- [ ] `execute()` calls `mark_seen` when `plan.seen_doc_ids` is non-empty (and not when empty).
- [ ] `cargo test -p rmreader` passes.

**Verify:** `cargo test -p rmreader --test readwise mark_seen` → PASS.

**Steps:**

- [ ] **Step 1: Add the endpoint const + `mark_seen`**

In `crates/rmreader/src/readwise/mod.rs`, add the const near the other URLs (~line 9):

```rust
const BULK_UPDATE_URL: &str = "https://readwise.io/api/v3/bulk_update/";
```

After `delete_document` (~line 129), add:

```rust
/// Mark documents read (`seen=true`) via the bulk-update endpoint. Chunked at 50
/// ids/request (the API max). Empty input is a no-op (zero HTTP calls).
pub fn mark_seen(t: &dyn HttpTransport, token: &str, ids: &[String]) -> anyhow::Result<()> {
    for chunk in ids.chunks(50) {
        let updates: Vec<_> = chunk
            .iter()
            .map(|id| serde_json::json!({ "id": id, "seen": true }))
            .collect();
        let body = serde_json::json!({ "updates": updates }).to_string();
        let r = t.request(HttpMethod::Patch, BULK_UPDATE_URL, token, Some(&body))?;
        anyhow::ensure!(
            (200..300).contains(&r.status),
            "bulk_update (seen) failed: HTTP {}",
            r.status
        );
    }
    Ok(())
}
```

- [ ] **Step 2: Wire `mark_seen` into `execute`**

In `crates/rmreader/src/readback/mod.rs`, in `pub fn execute` (~line 106-126), after the `create_highlights` call and before the warnings loop, add:

```rust
    if !plan.seen_doc_ids.is_empty() {
        if let Err(e) = readwise::mark_seen(t, token, &plan.seen_doc_ids) {
            eprintln!("[rmreader] mark_seen failed: {e:#}");
        }
    }
```

- [ ] **Step 3: Write tests**

Append to `crates/rmreader/tests/readwise.rs`. The `Recording` fake captures only the last call; add a multi-recording fake for the chunk test:

```rust
#[test]
fn mark_seen_patches_bulk_update_with_seen_true() {
    let r = Recording { last: RefCell::new(None), status: 200 };
    mark_seen(&r, "TKN", &["a".into(), "b".into()]).unwrap();
    let last = r.last.borrow().clone().unwrap();
    assert_eq!(last.0, HttpMethod::Patch);
    assert_eq!(last.1, "https://readwise.io/api/v3/bulk_update/");
    assert_eq!(last.2, "TKN");
    let body = last.3.unwrap();
    assert!(body.contains("\"updates\""), "body: {body}");
    assert!(body.contains("\"id\":\"a\""), "body: {body}");
    assert!(body.contains("\"seen\":true"), "body: {body}");
}

#[test]
fn mark_seen_empty_is_noop() {
    let fake = Counting { n: std::cell::RefCell::new(0) };
    mark_seen(&fake, "TKN", &[]).unwrap();
    assert_eq!(*fake.n.borrow(), 0, "empty ids → zero HTTP calls");
}

#[test]
fn mark_seen_chunks_at_50() {
    // 120 ids → 3 requests (50 + 50 + 20).
    let fake = Counting { n: std::cell::RefCell::new(0) };
    let ids: Vec<String> = (0..120).map(|i| format!("id{i}")).collect();
    mark_seen(&fake, "TKN", &ids).unwrap();
    assert_eq!(*fake.n.borrow(), 3, "120 ids must chunk into 3 requests");
}
```

Add `mark_seen` to the `use rmreader::readwise::{…}` import at the top of the file.

- [ ] **Step 4: Run tests**

Run: `cargo test -p rmreader --test readwise`
Expected: all PASS.

- [ ] **Step 5: Full crate test**

Run: `cargo test -p rmreader`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/rmreader/src/readwise/mod.rs crates/rmreader/src/readback/mod.rs \
        crates/rmreader/tests/readwise.rs
git commit -m "feat(rmreader): mark_seen via bulk_update + execute wiring"
```

```json:metadata
{"files": ["crates/rmreader/src/readwise/mod.rs", "crates/rmreader/src/readback/mod.rs", "crates/rmreader/tests/readwise.rs"], "verifyCommand": "cargo test -p rmreader --test readwise", "acceptanceCriteria": ["mark_seen PATCHes bulk_update with seen:true", "empty is no-op", "chunks at 50", "execute calls mark_seen when ids present"], "requiresUserVerification": false}
```

---

### Task 6: Tighten article chrome + visual verification

**Goal:** Finalize the article-page chrome so the action band sits just under the nav bar (no big gap) and the body rises accordingly, then verify the rendered layout with the user.

**Files:**
- Modify: `crates/rmreader/src/render/typst_doc.rs` (article header spacing ~line 184-193; article top margin in `#article` ~line 204)
- Test: existing `crates/rmreader/tests/render.rs` (guardrail: page count, links, determinism must still hold)

**Acceptance Criteria:**
- [ ] The gap between the nav bar and the action band on article pages is small (~4pt), not the old ~20pt.
- [ ] The reserved article header height and `#article` top margin are reduced so the body starts just below the action band with no large whitespace gap.
- [ ] Existing render tests still pass (3-page sample, links present, byte-deterministic).
- [ ] User confirms the rendered article page looks right (no clipping under the toolbar; bars adjacent; body not crowded).

**Verify:** `cargo test -p rmreader --test render` → PASS, then render a real sample PDF and inspect (command in Step 3).

**User Verification Required:**
Before marking this task complete, you MUST call AskUserQuestion:
```yaml
AskUserQuestion:
  question: "Rendered the article page with the tightened chrome (nav bar + action band adjacent, body raised). Does the layout look right on a reMarkable-sized page — bars adjacent with no big gap, nothing clipped under the toolbar, body not crowded?"
  header: "Verification"
  options:
    - label: "Looks right"
      description: "Spacing is correct — accept the chrome layout."
    - label: "Needs tuning"
      description: "Adjust the pt values (gap / header height / top margin) and re-render."
```
**If the user selects "Needs tuning":** the task is NOT complete — adjust the values, re-render, and re-verify with AskUserQuestion.

**Steps:**

- [ ] **Step 1: Confirm/adjust the article header spacing**

In `crates/rmreader/src/render/typst_doc.rs` `page-header` (article branch, set in Task 2), the values are `block(... height: 96pt)`, `v(34pt)` (toolbar clearance), `v(4pt)` (inter-bar gap). The math: 34 (clearance) + 24 (nav bar) + 4 (gap) + 28 (action band) = 90pt, within the 96pt block. Keep these unless Step 3 shows clipping.

- [ ] **Step 2: Reduce the article top margin to match**

In the `#article` helper (~line 204), change `set page(margin: (top: 120pt, …))` to:

```rust
  set page(margin: (top: 104pt, right: 16pt, bottom: 30pt, left: 16pt))
```

(104pt reserves the 96pt header block + 8pt `header-ascent`, so the body starts just under the action band instead of 120pt down.)

- [ ] **Step 3: Render a real sample PDF for inspection**

Run:

```bash
RMREADER_DUMP_TYPST=1 cargo test -p rmreader --test render renders_internal_links_and_pages
cargo run -p rmreader --example typst_preview 2>/dev/null || true
```

If `typst_preview` isn't suitable, generate from the dumped source with the project's typst path, or use `make_test_pdf`:

```bash
cargo run -p rmreader --example make_test_pdf
```

Then open the produced PDF (copy to a Tailscale-reachable path / serve it) and inspect an article page. Expected: nav bar and action band adjacent (~4pt apart), body starting just below, masthead/toolbar not clipped.

- [ ] **Step 4: Run render tests (guardrail)**

Run: `cargo test -p rmreader --test render`
Expected: PASS (page count, links, determinism unaffected by spacing).

- [ ] **Step 5: User verification**

Call AskUserQuestion exactly as in the "User Verification Required" block above. If "Needs tuning", adjust pt values, re-render (Step 3), re-verify. Only proceed when "Looks right".

- [ ] **Step 6: Commit**

```bash
git add crates/rmreader/src/render/typst_doc.rs
git commit -m "feat(rmreader): tighten article chrome (action band under nav bar)"
```

```json:metadata
{"files": ["crates/rmreader/src/render/typst_doc.rs"], "verifyCommand": "cargo test -p rmreader --test render", "acceptanceCriteria": ["inter-bar gap small", "article body raised", "render tests pass", "user confirms layout"], "requiresUserVerification": true, "userVerificationPrompt": "Does the tightened article chrome look right on a reMarkable-sized page — bars adjacent, nothing clipped, body not crowded?"}
```

---

## Self-Review

**1. Spec coverage:**
- §1 Feed unread → Task 1. ✓
- §2 Index nav bar (both collections, paging) → Task 2. ✓
- §3 Mark-all-read button (render + manifest + classify + readwise) → Tasks 3, 4, 5. ✓
- §4 Tighter article chrome → Task 6. ✓
- Testing section → tests in every task. ✓

**2. Placeholder scan:** No TBD/TODO; every code step shows full code; literal-fixup sites enumerated with exact files/lines. ✓

**3. Type consistency:** `seen_doc_ids` (Task 4) consumed by `execute` (Task 5). `MARK_ALL_READ_LABEL` defined in Task 3, used by classify in Task 4. `MarkAllReadRect`/`mark_all_read` defined in Task 3, used in Task 4. `mark_seen` signature consistent across Task 5 def/use. `Rendered.mark_all_read` (Task 3) read by `generate` (Task 3). ✓

**4. Verification requirement scan:** YES — the spec defers exact chrome/margin pt values to a rendered-page check, which is human visual sign-off. Task 6 carries `requiresUserVerification: true` + the standard AskUserQuestion block. ✓

## Dependencies

- Task 2 → blockedBy Task 1 (both edit `typst_doc.rs`; sequential avoids churn and Task 2 sets the article header block Task 6 tunes).
- Task 3 → blockedBy Task 2 (edits same `typst_doc.rs`/`render` region; needs the manifest field before Task 4).
- Task 4 → blockedBy Task 3 (`mark_all_read` field + `MARK_ALL_READ_LABEL`).
- Task 5 → blockedBy Task 4 (`seen_doc_ids`).
- Task 6 → blockedBy Task 5 (final `typst_doc.rs` tuning after all render edits land).
