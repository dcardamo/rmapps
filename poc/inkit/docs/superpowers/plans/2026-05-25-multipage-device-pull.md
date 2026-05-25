# Multi-page device pull Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Assemble per-page ink from a multi-page `.rmdoc` on the reading-queue app's real device path, so a user's annotations on pages 2..N are no longer dropped.

**Architecture:** Replace `serve.rs`'s first-`.rm`-only pull with a pure `strokes_by_page` that maps over `rm_files::Bundle::pages()` (the authoritative `.content` order) and emits one `Vec<Stroke>` per page — empty for un-inked pages so indices never shift. A one-line additive `Page::scene_bytes()` in `rm-files` bridges the bundle's raw `.rm` bytes to the proven `Remarkable::read_ink`. Everything downstream (`App::step`, `attribute`, manifest) is already per-page and unchanged.

**Tech Stack:** Rust, `rm-files` (`.rmdoc`/`.content`/`.rm` parsing), `inkapp-core` (runtime/readback/components), `inkapp-remarkable` (device transform), `zip` 2.4.

Spec: [docs/superpowers/specs/2026-05-25-multipage-device-pull-design.md](../specs/2026-05-25-multipage-device-pull-design.md) (Spec #12).

---

### Task 1: `rm_files::Page::scene_bytes()` — raw per-page `.rm` bytes accessor

**Goal:** Expose the raw `.rm` bytes for a bundle page (None when un-inked), so the device-pull can feed them to `read_ink` in `.content` order.

**Files:**
- Modify: `crates/rm-files/src/bundle/mod.rs` (add `scene_bytes` to `impl Page<'_>`, next to `scene` at lines 139-151)
- Test: `crates/rm-files/tests/bundle.rs` (add one test)

**Acceptance Criteria:**
- [ ] `Page::scene_bytes()` returns `Some(&[u8])` for a page that has an `.rm` entry, matching the raw file bytes.
- [ ] `Page::scene_bytes()` returns `None` for a page with no `.rm` entry.
- [ ] Pages are addressed in `.content` order (slot 0 = first listed page).

**Verify:** `cargo test -p rm-files --test bundle scene_bytes` → PASS

**Steps:**

- [ ] **Step 1: Write the failing test**

Add to `crates/rm-files/tests/bundle.rs`:

```rust
/// `scene_bytes()` returns the raw `.rm` bytes for an inked page (in `.content`
/// order) and `None` for an un-inked page — the accessor the device-pull needs to
/// assemble per-page ink without re-parsing `.content`.
#[test]
fn scene_bytes_returns_raw_rm_or_none_in_content_order() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // An unpacked bundle: `<uuid>.content` listing two pages in order; only the
    // first page has an `.rm` file (the second is un-inked).
    let uuid = "doc-uuid";
    std::fs::write(
        root.join(format!("{uuid}.content")),
        br#"{"cPages":{"pages":[{"id":"page-a"},{"id":"page-b"}]}}"#,
    )
    .unwrap();
    std::fs::create_dir(root.join(uuid)).unwrap();
    let rm_a = b"\x00raw-rm-bytes-for-a";
    std::fs::write(root.join(uuid).join("page-a.rm"), rm_a).unwrap();

    let bundle = rm_files::Bundle::open(root).unwrap();
    let pages = bundle.pages();
    assert_eq!(pages.len(), 2, "both pages listed in .content order");
    assert_eq!(
        pages[0].scene_bytes(),
        Some(&rm_a[..]),
        "page A (inked) returns its raw .rm bytes"
    );
    assert_eq!(
        pages[1].scene_bytes(),
        None,
        "page B (no .rm) returns None"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rm-files --test bundle scene_bytes -- --nocapture`
Expected: FAIL — `no method named scene_bytes found for struct Page`.

- [ ] **Step 3: Add the accessor**

In `crates/rm-files/src/bundle/mod.rs`, inside `impl Page<'_>` (after the `scene` method, before the closing `}` at line 151), add:

```rust
    /// Raw bytes of this page's `.rm` scene file, if present.
    ///
    /// Returns `None` when the page has never been annotated (no `.rm` entry),
    /// the same condition under which [`scene`][Page::scene] returns `Ok(None)`.
    /// Unlike `scene`, this hands back the unparsed bytes so a caller can run its
    /// own device transform (e.g. `Remarkable::read_ink`).
    pub fn scene_bytes(&self) -> Option<&[u8]> {
        let key = format!("{}/{}.rm", self.bundle.uuid, self.id);
        self.bundle.files.get(&key).map(|v| v.as_slice())
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rm-files --test bundle scene_bytes -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rm-files/src/bundle/mod.rs crates/rm-files/tests/bundle.rs
git -c core.hooksPath=.githooks commit -m "rm-files: add Page::scene_bytes() for raw per-page .rm bytes"
```

---

### Task 2: `serve::strokes_by_page` + multi-page `pull_ink` (with end-to-end test)

**Goal:** Assemble per-page ink in `.content` order on the device-pull path and prove it end-to-end (per-page alignment, split-region stitching, empty-middle-page no-shift) without a device.

**Files:**
- Modify: `apps/reading-queue/Cargo.toml` (add `rm-files` dependency)
- Modify: `apps/reading-queue/src/serve.rs` (replace `strokes_from_rmdoc` with `strokes_by_page`; rewrite `pull_ink`; drop the "future step" caveat; drop now-unused imports)
- Create: `apps/reading-queue/tests/multipage.rs`
- Stage: `Cargo.lock`

**Acceptance Criteria:**
- [ ] `serve::strokes_by_page(device, path, page_h)` returns a `Vec<Vec<Stroke>>` of length `bundle.pages().len()`, in `.content` order, empty `Vec` per un-inked page.
- [ ] `pull_ink` builds per-page ink via `strokes_by_page` and inserts a key only when some page has ink.
- [ ] The "multi-page rmdoc support is a future step" comment and the first-`.rm` doc comment are gone.
- [ ] New test: ink written on page *k* attributes to a region with `region.page == k`.
- [ ] New test: a `Passage` region inked on two non-adjacent frames stitches to one `Msg::Note`.
- [ ] New test: an un-inked middle page is an empty slot, and a later page (`done`) still attributes correctly (no shift).

**Verify:** `cargo test -p reading-queue --test multipage` → PASS; `cargo test -p reading-queue` → PASS

**Steps:**

- [ ] **Step 1: Add the `rm-files` dependency**

`strokes_by_page` uses `rm_files::Bundle`, which the app doesn't yet depend on. In `apps/reading-queue/Cargo.toml`, under `[dependencies]`, add:

```toml
rm-files = { path = "../../crates/rm-files" }
```

- [ ] **Step 2: Write the failing end-to-end test**

Create `apps/reading-queue/tests/multipage.rs`. This mirrors the `pagination_device_blind` harness test in reverse: it renders a tall document, writes per-page ink through the real `.rm` byte path into a synthesized `.rmdoc` zip (omitting one middle page's `.rm`), then runs the new pull path and asserts attribution.

```rust
//! The reading-queue device pull assembles per-page ink from a multi-page
//! `.rmdoc` in `.content` order: ink on page k attributes to page k, a passage
//! split across page breaks stitches to one message, and an un-inked middle page
//! stays an empty slot (no index shift). No device / no rmapi.

use std::io::Write;

use inkapp::{Document, Remarkable};
use inkapp_core::component::{Component, RenderCx};
use inkapp_core::components::checkbox::Checkbox;
use inkapp_core::components::highlight_text::HighlightableText;
use inkapp_core::components::passage::Passage;
use inkapp_core::crypto::Key;
use inkapp_core::device::Device;
use inkapp_core::geometry::{PageGeom, PdfPoint};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::Manifest;
use inkapp_core::readback::attribute;
use inkapp_core::runtime::render_document_in;

use reading_queue::serve::strokes_by_page;

/// Local test messages (this test exercises the pull path, not the app's view).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Msg {
    Hi(String),
    Note,
    Done,
}

/// A per-token highlightable body that emits `Hi(token)`.
struct Body {
    text: HighlightableText,
}
impl Component for Body {
    type Msg = Msg;
    fn render(&self, cx: &mut RenderCx) -> String {
        self.text.render(cx)
    }
    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<Msg> {
        self.text.read(ink, manifest).into_iter().map(Msg::Hi).collect()
    }
}

/// A tall document: tokens + a long breakable passage + an archive checkbox.
/// The passage forces several pages and splits across breaks.
fn doc() -> Document<Msg> {
    let tokens: Vec<String> = (0..30).map(|i| format!("word{i:02}")).collect();
    let tok_refs: Vec<&str> = tokens.iter().map(|s| s.as_str()).collect();
    let body = Body { text: HighlightableText::new(&tok_refs) };

    let lines: Vec<String> = (0..30).map(|i| format!("passage line number {i}")).collect();
    let line_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    let passage = Passage::with_msg("notes", &line_refs, Msg::Note);

    let check = Checkbox::with_msg("done", Msg::Done).label("Archive");

    let mut flow: Vec<Box<dyn Component<Msg = Msg>>> = Vec::new();
    flow.push(Box::new(body));
    flow.push(Box::new(passage));
    flow.push(Box::new(check));
    Document::keyed("d", flow)
}

/// A swipe across a region's rect, sampled at interior points so at least one
/// survives the f32 round-trip in the device transform (`attribute` needs ANY
/// point inside the rect).
fn swipe(r: &inkapp_core::manifest::Region, highlighter: bool) -> Stroke {
    const SAMPLES: usize = 8;
    let cy = (r.rect.y0 + r.rect.y1) / 2.0;
    let points: Vec<PdfPoint> = (0..=SAMPLES)
        .map(|k| {
            let t = k as f64 / SAMPLES as f64;
            PdfPoint { x: r.rect.x0 + t * (r.rect.x1 - r.rect.x0), y: cy }
        })
        .collect();
    Stroke { points, highlighter }
}

/// Write a multi-page `.rmdoc` zip: `<uuid>.content` listing `page_count` pages in
/// order, and `<uuid>/<page-uuid>.rm` for every page whose `per_page` strokes are
/// non-empty (un-inked pages get no `.rm`). Returns the file path.
fn write_rmdoc(device: &Remarkable, page_h: f64, per_page: &[Vec<Stroke>]) -> std::path::PathBuf {
    let uuid = "doc-uuid";
    let page_ids: Vec<String> = (0..per_page.len()).map(|p| format!("page-{p}")).collect();

    let unique = format!("{}-{}", std::process::id(), std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    let path = std::env::temp_dir().join(format!("rq-multipage-{unique}.rmdoc"));
    let file = std::fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::SimpleFileOptions =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    // .content (cPages page list, in order).
    let pages_json: Vec<String> = page_ids.iter().map(|id| format!(r#"{{"id":"{id}"}}"#)).collect();
    let content = format!(r#"{{"cPages":{{"pages":[{}]}}}}"#, pages_json.join(","));
    zip.start_file(format!("{uuid}.content"), opts).unwrap();
    zip.write_all(content.as_bytes()).unwrap();

    // One `.rm` per inked page.
    for (p, strokes) in per_page.iter().enumerate() {
        if strokes.is_empty() {
            continue;
        }
        let bytes = device.write_ink(strokes, page_h).unwrap();
        zip.start_file(format!("{uuid}/{}.rm", page_ids[p]), opts).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap();
    path
}

#[test]
fn pull_assembles_per_page_ink_with_empty_middle_page() {
    let key = Key::from_bytes([7u8; 32]);
    let device = Remarkable::new();
    let d = doc();

    // A short page forces several pages and splits the passage across breaks.
    let geom = PageGeom { w: 420.0, h: 240.0, margin: 16.0 };
    let rd = render_document_in(&d, 1, &key, geom).unwrap();

    // Pages the `notes` passage occupies (one frame per page it flows through).
    let mut notes_pages: Vec<usize> = rd
        .manifest
        .regions
        .iter()
        .filter(|r| r.name == "notes")
        .map(|r| r.page)
        .collect();
    notes_pages.sort_unstable();
    notes_pages.dedup();
    assert!(
        rd.page_count >= 3 && notes_pages.len() >= 3,
        "need >=3 pages and a passage spanning >=3 frames to test a gap; \
         got page_count={}, notes frames on pages {:?}",
        rd.page_count,
        notes_pages
    );

    // Ink targets: a token, the `done` checkbox, and notes on two NON-ADJACENT
    // frames (notes_pages[0] and notes_pages[2]); leave notes_pages[1] un-inked as
    // the empty middle slot.
    let inked_notes = [notes_pages[0], notes_pages[2]];
    let empty_page = notes_pages[1];

    let mut per_page: Vec<Vec<Stroke>> = vec![Vec::new(); rd.page_count];
    for r in &rd.manifest.regions {
        let target = match r.name.as_str() {
            "tok-7" => Some(true),
            "done" => Some(false),
            "notes" if inked_notes.contains(&r.page) => Some(true),
            _ => None,
        };
        if let Some(hl) = target {
            per_page[r.page].push(swipe(r, hl));
        }
    }
    assert!(
        per_page[empty_page].is_empty(),
        "the chosen middle page must be un-inked (it carries only a skipped notes frame)"
    );

    // Synthesize the `.rmdoc` and run the real pull path.
    let path = write_rmdoc(&device, rd.page_h, &per_page);
    let pages = strokes_by_page(&device, &path, rd.page_h);
    std::fs::remove_file(&path).ok();

    // Length matches the `.content` page list; the middle page is an empty slot.
    assert_eq!(pages.len(), rd.page_count, "one slot per .content page");
    assert!(
        pages[empty_page].is_empty(),
        "un-inked middle page {empty_page} is an empty slot, not dropped"
    );

    // Attribute + decode: per-page alignment, split-region stitching, no shift.
    let region_ink = attribute(&pages, &rd.manifest);
    let mut msgs = std::collections::BTreeSet::new();
    for c in &d.flow {
        for m in c.decode(&region_ink, &rd.manifest) {
            msgs.insert(m);
        }
    }
    let expected: std::collections::BTreeSet<Msg> =
        [Msg::Hi("word07".into()), Msg::Note, Msg::Done].into_iter().collect();
    assert_eq!(
        msgs, expected,
        "ink attributes to the right page (tok-7=>word07, notes stitched from two \
         frames=>one Note, done on the last page=>Done) despite an empty middle page"
    );
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p reading-queue --test multipage -- --nocapture`
Expected: FAIL — `strokes_by_page` not found in `reading_queue::serve` (function doesn't exist yet).

- [ ] **Step 4: Implement `strokes_by_page` and rewrite `pull_ink`**

In `apps/reading-queue/src/serve.rs`:

(a) Update imports at the top — drop `Path` only if unused (it is still used by `find_rmdocs` and the new fn, so keep `use std::path::{Path, PathBuf};`), and add the bundle import near the other `use` lines:

```rust
use rm_files::Bundle;
```

(b) Replace the whole `strokes_from_rmdoc` function (lines 85-106) with:

```rust
/// Assemble per-page PDF-space strokes from an `.rmdoc` bundle, indexed by the
/// bundle's `.content` page order: slot `p` aligns with the manifest's
/// `region.page == p`. An un-inked page occupies its slot as an empty `Vec`, so it
/// never shifts later pages. All pages of a document share one `page_h` (Typst
/// `#set page` fixes every page to the same height). Empty if the bundle won't open.
fn strokes_by_page(device: &Remarkable, path: &Path, page_h: f64) -> Vec<Vec<Stroke>> {
    let Ok(bundle) = Bundle::open(path) else {
        return Vec::new();
    };
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

Then make it visible to the integration test by changing the signature line to `pub fn strokes_by_page(...)`.

(c) Replace the `for p in rmdocs { ... }` loop body in `pull_ink` (lines 132-142) with:

```rust
    for p in rmdocs {
        let Some(key) = p.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
            continue;
        };
        let page_h = page_h_by_key.get(&key).copied().unwrap_or(0.0);
        let pages = strokes_by_page(device, &p, page_h);
        // Insert only when the document carries some ink on some page.
        if pages.iter().any(|pg| !pg.is_empty()) {
            out.insert(key, pages);
        }
    }
```

- [ ] **Step 5: Run the test (and the app's full suite) to verify pass**

Run: `cargo test -p reading-queue --test multipage -- --nocapture`
Expected: PASS.

Run: `cargo test -p reading-queue`
Expected: PASS (existing `app`, `banner`, `shared`, `device` tests still green; `device`'s on-device tests are `#[ignore]`).

- [ ] **Step 6: Commit (staging `Cargo.lock`)**

```bash
git add apps/reading-queue/Cargo.toml apps/reading-queue/src/serve.rs \
        apps/reading-queue/tests/multipage.rs Cargo.lock
git -c core.hooksPath=.githooks commit -m "reading-queue: assemble per-page ink from multi-page .rmdoc on device pull"
```

---

### Task 3: Docs sweep + workspace-green gate

**Goal:** Ensure no doc implies a single-page device pull and the entire workspace builds and tests green.

**Files:**
- Inspect/Modify (only if a single-page-pull implication exists): `docs/appdx.md`, `docs/how-it-works.md`, `docs/remarkable-pdf-mechanics.md`

**Acceptance Criteria:**
- [ ] No doc states or implies the device pull reads only the first `.rm` / a single page.
- [ ] `cargo test --workspace` is green.

**Verify:** `cargo test --workspace` → PASS

**Steps:**

- [ ] **Step 1: Search the docs for a single-page-pull implication**

Run: `rg -n -i "first \.rm|single[- ]page|one \.rm|page 0|future step|wrap.*page" docs/`
Expected: no hit that asserts the *device pull* is single-page. (The Spec #12 design doc legitimately *describes* the old behaviour in past tense — that is correct and stays.) `appdx.md`'s "Documents, pages, and devices" already says pagination + per-page ink is *(Built.)*, which is now true on the device path too.

- [ ] **Step 2: Correct any stale implication found**

If (and only if) Step 1 surfaces prose claiming the live pull is single-page, edit that sentence to state the pull assembles per-page ink in `.content` order. Based on the current tree this is expected to be a no-op; do not invent edits.

- [ ] **Step 3: Run the full workspace test suite**

Run: `cargo test --workspace`
Expected: PASS across all crates (`rm-files`, `inkapp-core`, `inkapp-remarkable`, `inkapp-harness`, `reading-queue`, connectors). On-device `#[ignore]` tests stay skipped.

- [ ] **Step 4: Commit (only if docs changed)**

```bash
git add docs/
git -c core.hooksPath=.githooks commit -m "docs: confirm device pull assembles per-page ink (no single-page implication)"
```

If no docs changed, skip the commit — the workspace-green gate is the deliverable.

---

## Self-Review

**Spec coverage:**
- Pure assembly core (`strokes_by_page`) → Task 2 ✓
- `pull_ink` calls the core, skip-empty preserved → Task 2 ✓
- `rm_files::Page::scene_bytes()` → Task 1 ✓
- Size to `bundle.pages().len()`, no `page_count` param → Task 2 (implementation + length assertion) ✓
- Uniform `page_h` reused → Task 2 (single `page_h` threaded to every page) ✓
- Deterministic test: per-page alignment, split-region stitching, empty-middle no-shift, length check → Task 2 ✓
- Remove "future step" caveat + fix doc comment → Task 2 ✓
- Docs sweep for single-page-pull implication → Task 3 ✓
- `cargo test --workspace` green; `Cargo.lock` staged → Task 3 / Task 2 ✓

**Type consistency:** `strokes_by_page(device: &Remarkable, path: &Path, page_h: f64) -> Vec<Vec<Stroke>>` used identically in serve.rs and the test; `Page::scene_bytes(&self) -> Option<&[u8]>` consistent between Task 1 and Task 2; `render_document_in(&d, 1, &key, geom)` and `attribute(&pages, &manifest)` match their definitions in `runtime.rs`/`readback.rs`.

**Placeholder scan:** none — every code step is complete.
