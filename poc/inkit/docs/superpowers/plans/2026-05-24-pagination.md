# Multi-page Pagination Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make a single content flow render to N pages — N differing per device profile — with regions recovered from every frame (including a region split across a page break), per-page ink stitched back into content-relative regions, and components staying page- and device-blind.

**Architecture:** Page geometry becomes a per-render `PageGeom` input. The `#region` Typst prelude gains a `breakable` variant that emits start/end markers; Rust `recover_regions` reconstructs a breakable region's per-frame rects (`split_rects`) using each frame's own height. `attribute` becomes page-aware (per-page strokes, no cross-page attribution) and stitches a logical region's ink across pages into one `RegionInk`. The runtime carries a page count and per-page preserved ink. Device-independence is proved in the harness with two `PageGeom` profiles; the runtime stays single-profile per app.

**Tech Stack:** Rust, Typst 0.14.2 (compiled as a library), `serde`, `tokio` (loop), the `inkapp-harness` deterministic simulator, `inkapp-remarkable` device transform.

**Spec:** `docs/superpowers/specs/2026-05-24-pagination-design.md` (Spec #11).

**Execution conventions (this repo — bake into every implementer prompt):**
- Commit with `git -c core.hooksPath=.githooks commit -m "..."` (the literal `git commit` substring trips a transcript-replay hook that blocks on open native tasks; this form keeps the real `cargo fmt --check` pre-commit active while dodging the blocker).
- No `Co-Authored-By` lines. Create new commits; never amend.
- `git add Cargo.lock` alongside any `Cargo.toml` change (no deps are expected in this plan, but sweep it if `cargo` touches it).
- Implementer subagents MUST NOT use any Task tools / touch the task list (native tasks are shared with the controller).
- When migrating a shared signature/type, enumerate call sites with a **worktree-root** `rg` across **both** `crates/` and `apps/`, and verify with `cargo test --workspace` (not `-p inkapp-core`) — `apps/` does not compile under `-p inkapp-core`.

---

### Task 1: `PageGeom` — geometry becomes a per-render input

**Goal:** Replace the `DOC_PAGE_W/H` constants with a `PageGeom` threaded through the render path (with default-geom convenience wrappers), and let `App` carry a settable geometry — the input that makes pagination differ per device.

**Files:**
- Modify: `crates/inkapp-core/src/geometry.rs` (add `PageGeom`)
- Modify: `crates/inkapp-core/src/runtime.rs` (remove consts; add `*_in` fns; `App.geom` + builder `.page`)
- Modify: `crates/inkapp-core/src/lib.rs` (re-export `PageGeom` + the new `*_in` fns)
- Test: `crates/inkapp-core/tests/pagination.rs` (new file)

**Acceptance Criteria:**
- [ ] `PageGeom { w, h, margin }` with `Default` (420/560/16) and `content_w()` exists in `geometry.rs`.
- [ ] `DOC_PAGE_W` / `DOC_PAGE_H` no longer exist; `rg "DOC_PAGE_W|DOC_PAGE_H"` is empty.
- [ ] `document_source_in` / `compile_document_in` / `render_document_in` take a `PageGeom`; the existing zero-geom names remain as wrappers passing `PageGeom::default()`.
- [ ] `App` carries `geom: PageGeom`, settable via `.page(geom)` on the builder; `App::render`/`App::step` render with `self.geom`.
- [ ] A doc paginates to more pages under a short `PageGeom` than under the default.

**Verify:** `cargo test -p inkapp-core --test pagination` → passes; `cargo build --workspace` → clean; `rg "DOC_PAGE_W|DOC_PAGE_H"` → no matches.

**Steps:**

- [ ] **Step 1: Write the failing test** — `crates/inkapp-core/tests/pagination.rs`

```rust
use inkapp_core::document::Document;
use inkapp_core::geometry::PageGeom;
use inkapp_core::runtime::compile_document_in;
use inkapp_core::{flow, components::notice::Notice};

/// A tall flow: 40 notice lines. Fits in few pages at the default geometry and
/// more pages on a short page — pagination is purely a function of PageGeom.
fn tall_doc() -> Document<()> {
    let lines: Vec<Notice<()>> = (0..40).map(|i| Notice::line(&format!("line number {i}"))).collect();
    // flow! needs component exprs; build the boxed flow directly for a Vec input.
    let mut f: Vec<Box<dyn inkapp_core::component::Component<Msg = ()>>> = Vec::new();
    for n in lines { f.push(Box::new(n)); }
    Document::keyed("tall", f)
}

#[test]
fn short_page_paginates_to_more_pages() {
    let doc = tall_doc();
    let default_pages = compile_document_in(&doc, PageGeom::default()).unwrap().pages.len();
    let short_pages = compile_document_in(&doc, PageGeom { w: 420.0, h: 180.0, margin: 16.0 })
        .unwrap()
        .pages
        .len();
    assert!(
        short_pages > default_pages,
        "short page must paginate to more pages: short={short_pages} default={default_pages}"
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p inkapp-core --test pagination`
Expected: FAIL to compile — `PageGeom` and `compile_document_in` don't exist yet.

- [ ] **Step 3: Add `PageGeom` to `geometry.rs`** (append after `typst_to_pdf_rect`)

```rust
/// A document's page geometry, in points. Drives Typst `#set page` and lets the
/// content column width be computed for full-width regions. A *device profile* in
/// inkapp is a `PageGeom` paired with a `Device`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGeom {
    pub w: f64,
    pub h: f64,
    pub margin: f64,
}

impl Default for PageGeom {
    /// The standard 3:4-ish e-ink profile (the former DOC_PAGE_W/H + 16pt margin).
    fn default() -> Self {
        Self { w: 420.0, h: 560.0, margin: 16.0 }
    }
}

impl PageGeom {
    /// The content column width (page width minus both margins).
    pub fn content_w(&self) -> f64 {
        self.w - 2.0 * self.margin
    }
}
```

- [ ] **Step 4: Rework `runtime.rs` render fns to be geometry-parametric**

Delete the consts:

```rust
// REMOVE these two lines and their doc comment:
// pub const DOC_PAGE_W: f64 = 420.0;
// pub const DOC_PAGE_H: f64 = 560.0;
```

Add `use crate::geometry::PageGeom;` to the imports at the top of `runtime.rs`.

Replace `document_source` with a wrapper + `_in`:

```rust
/// Assemble a document's Typst source at the default page geometry.
pub fn document_source<M>(doc: &Document<M>) -> String {
    document_source_in(doc, PageGeom::default())
}

/// Assemble a document's Typst source at an explicit page geometry: `#import`
/// lines for the prelude and authored sources, the `#set page` from `geom`, then
/// each component's render in flow order.
pub fn document_source_in<M>(doc: &Document<M>, geom: PageGeom) -> String {
    let mut cx = RenderCx::new(0);
    let mut src = String::new();
    for (path, _) in collect_typst_sources(doc) {
        src.push_str(&format!("#import \"{path}\": *\n"));
    }
    src.push_str(&format!(
        "#set page(width: {}pt, height: {}pt, margin: {}pt)\n#set text(size: 12pt)\n",
        geom.w, geom.h, geom.margin
    ));
    for c in &doc.flow {
        src.push_str(&c.render(&mut cx));
    }
    src
}
```

Replace `compile_document` likewise:

```rust
/// Compile a document at the default page geometry.
pub fn compile_document<M>(doc: &Document<M>) -> Result<typst::layout::PagedDocument> {
    compile_document_in(doc, PageGeom::default())
}

/// Compile a document at an explicit page geometry, with all its Typst sources
/// (prelude + authored components) registered.
pub fn compile_document_in<M>(
    doc: &Document<M>,
    geom: PageGeom,
) -> Result<typst::layout::PagedDocument> {
    let src = document_source_in(doc, geom);
    let sources = collect_typst_sources(doc);
    crate::render::compile_to_document_with_sources(&src, &sources)
}
```

Replace `render_document` with a wrapper + `_in` (page count is added in Task 5; here just thread geom and use `geom.h` as the page-height fallback):

```rust
/// Render one document at the default page geometry.
pub fn render_document<M>(doc: &Document<M>, version: u64, key: &Key) -> Result<RenderedDoc> {
    render_document_in(doc, version, key, PageGeom::default())
}

/// Render one document at an explicit page geometry, sealing its manifest with `key`.
pub fn render_document_in<M>(
    doc: &Document<M>,
    version: u64,
    key: &Key,
    geom: PageGeom,
) -> Result<RenderedDoc> {
    let src = document_source_in(doc, geom);
    let compiled = compile_document_in(doc, geom)?;
    let page_h = compiled
        .pages
        .first()
        .map(|p| p.frame.height().to_pt())
        .unwrap_or(geom.h);
    let mut manifest = recover_regions(&compiled)?.with_version(version);
    manifest.state.doc = doc.state.clone();
    for c in &doc.flow {
        if let (Some(k), Some(v)) = (c.state_key(), c.render_state()) {
            manifest.state.components.insert(k, v);
        }
    }
    let pdf = embed_manifest(&document_to_pdf(&compiled)?, &manifest, key)?;
    Ok(RenderedDoc {
        key: doc.key.clone(),
        pdf,
        manifest,
        page_h,
        hash: hash_str(&src),
    })
}
```

- [ ] **Step 5: Give `App` a geometry + builder step**

In `App<M, Msg, Cx>` add a field `geom: PageGeom`. Update `App::new` to accept it:

```rust
pub fn new(
    model: M,
    connectors: Cx,
    update: UpdateFn<M, Msg, Cx>,
    view: ViewFn<M, Msg, Cx>,
    key: Key,
    geom: PageGeom,
) -> Self {
    Self { model, connectors, update, view, version: 1, key, geom }
}
```

In `App::render`, change the render call:

```rust
let rd = render_document_in(doc, self.version, &self.key, self.geom)?;
```

In `App::step` phase 3, change:

```rust
next_rendered.push(render_document_in(doc, self.version, &self.key, self.geom)?);
```

Carry `geom` through the builder. In `BuilderReady`, add a `geom: PageGeom` field defaulted when `key()` constructs it:

```rust
// In BuilderFull::key:
pub fn key(self, key: Key) -> BuilderReady<M, Msg, Cx> {
    BuilderReady {
        model: self.model,
        connectors: self.connectors,
        update: self.update,
        view: self.view,
        key,
        geom: PageGeom::default(),
    }
}
```

Add a `.page` setter and pass `geom` to `App::new` in `build`:

```rust
impl<M, Msg, Cx> BuilderReady<M, Msg, Cx> {
    /// Override the page geometry (device profile). Defaults to `PageGeom::default()`.
    #[must_use]
    pub fn page(mut self, geom: PageGeom) -> Self {
        self.geom = geom;
        self
    }

    pub fn build(self) -> App<M, Msg, Cx> {
        App::new(
            self.model,
            self.connectors,
            self.update,
            self.view,
            self.key,
            self.geom,
        )
    }
}
```

(Add the `geom` field to the `BuilderReady` struct definition.)

- [ ] **Step 6: Re-export from `lib.rs`**

Add `PageGeom` to the geometry re-export and the new fns to the runtime re-export:

```rust
pub use geometry::{DevicePoint, PageGeom, PdfPoint, PdfRect};
pub use runtime::{
    app, collect_typst_sources, compile_document, compile_document_in, document_source,
    document_source_in, render_document, render_document_in, App, Cycle, DocSet, RenderedDoc,
    REGION_PRELUDE,
};
```

- [ ] **Step 7: Run the test + workspace build**

Run: `cargo test -p inkapp-core --test pagination`
Expected: PASS.
Run: `cargo build --workspace`
Expected: clean (the zero-geom wrappers keep every existing caller compiling).
Run: `rg "DOC_PAGE_W|DOC_PAGE_H"`
Expected: no matches.

- [ ] **Step 8: Commit**

```bash
git add crates/inkapp-core/src/geometry.rs crates/inkapp-core/src/runtime.rs crates/inkapp-core/src/lib.rs crates/inkapp-core/tests/pagination.rs
git -c core.hooksPath=.githooks commit -m "inkapp-core: PageGeom — page geometry is a per-render input (pagination step 1)"
```

---

### Task 2: Split-aware region recovery (`split_rects` + role schema)

**Goal:** Teach `recover_regions` the role-tagged metadata schema and reconstruct a breakable region's per-frame rects from its start/end markers, each transformed with its own page height.

**Files:**
- Modify: `crates/inkapp-core/src/manifest.rs` (`RawRegion` schema, `split_rects`, `recover_regions`)
- Test: `crates/inkapp-core/src/manifest.rs` (`#[cfg(test)]` unit tests for `split_rects`)
- Test: `crates/inkapp-core/tests/pagination.rs` (recovery of a flow region across pages)

**Acceptance Criteria:**
- [ ] `RawRegion` accepts an optional `role`, and `w`/`h` are optional (flow-end has neither).
- [ ] `split_rects` (pure) emits one `Region` per page in `start.page..=end.page` with correct per-page top/bottom; unit-tested for 1-, 2-, and 3-page spans.
- [ ] `recover_regions` converts atomic rows exactly as before (checkbox/per-token unchanged) and pairs `flow-start`/`flow-end` by name into split rects.
- [ ] A document emitting flow markers across a page break recovers as ≥2 `Region`s sharing the name.

**Verify:** `cargo test -p inkapp-core --test pagination` and `cargo test -p inkapp-core --lib manifest` → pass; existing `cargo test -p inkapp-core --test regions` → still green.

**Steps:**

- [ ] **Step 1: Write the failing `split_rects` unit tests** — append to `manifest.rs` `#[cfg(test)]`

```rust
#[cfg(test)]
mod split_tests {
    use super::*;

    #[test]
    fn single_page_is_one_rect() {
        let rs = split_rects("p", 0, 10.0, 100.0, 50.0, 0, 140.0, &[560.0]).unwrap();
        assert_eq!(rs.len(), 1);
        assert_eq!(rs[0].page, 0);
        assert_eq!(rs[0].rect, typst_to_pdf_rect(10.0, 100.0, 50.0, 40.0, 560.0));
    }

    #[test]
    fn two_pages_split_at_break() {
        // Starts low on page 0, ends near the top of page 1.
        let rs = split_rects("p", 0, 10.0, 500.0, 50.0, 1, 30.0, &[560.0, 560.0]).unwrap();
        assert_eq!(rs.len(), 2);
        // Page 0: from y=500 to the page bottom (560) → height 60.
        assert_eq!(rs[0], Region { name: "p".into(), page: 0, rect: typst_to_pdf_rect(10.0, 500.0, 50.0, 60.0, 560.0) });
        // Page 1: from the top (0) to y=30 → height 30.
        assert_eq!(rs[1], Region { name: "p".into(), page: 1, rect: typst_to_pdf_rect(10.0, 0.0, 50.0, 30.0, 560.0) });
    }

    #[test]
    fn three_pages_middle_is_full_height() {
        let rs = split_rects("p", 0, 10.0, 500.0, 50.0, 2, 20.0, &[560.0, 560.0, 560.0]).unwrap();
        assert_eq!(rs.len(), 3);
        assert_eq!(rs[1].rect, typst_to_pdf_rect(10.0, 0.0, 50.0, 560.0, 560.0)); // full page
        assert_eq!(rs[2].rect, typst_to_pdf_rect(10.0, 0.0, 50.0, 20.0, 560.0));
    }

    #[test]
    fn missing_page_errors() {
        assert!(split_rects("p", 0, 0.0, 0.0, 1.0, 5, 0.0, &[560.0]).is_err());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p inkapp-core --lib manifest`
Expected: FAIL to compile — `split_rects` doesn't exist.

- [ ] **Step 3: Update `RawRegion` and add `split_rects`** in `manifest.rs`

```rust
/// The raw metadata a component emits next to a `<region>` label. Coordinates are
/// Typst-space (top-left origin), in points. `role` distinguishes an atomic region
/// (`"box"` or absent — carries `w`/`h`) from the bounds of a breakable region
/// (`"flow-start"` carries `w`; `"flow-end"` carries only its position).
#[derive(Debug, Clone, Deserialize)]
struct RawRegion {
    name: String,
    page: usize,
    x: f64,
    y: f64,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    w: Option<f64>,
    #[serde(default)]
    h: Option<f64>,
}

/// Reconstruct the per-frame PDF rects of a breakable region from its start/end
/// bounds. Emits one `Region` per page in `start_page..=end_page`: the start page
/// runs from `y_start` to its bottom, interior pages are full height, the end page
/// runs from the top to `y_end`. Each rect is flipped to PDF space with its own
/// page height. `start_page == end_page` degenerates to a single `y_start..y_end`
/// rect (the no-break case).
fn split_rects(
    name: &str,
    start_page: usize,
    x: f64,
    y_start: f64,
    w: f64,
    end_page: usize,
    y_end: f64,
    page_heights: &[f64],
) -> Result<Vec<Region>> {
    let mut out = Vec::new();
    for p in start_page..=end_page {
        let page_h = *page_heights.get(p).ok_or_else(|| {
            Error::Region(format!("flow region '{name}' references missing page {p}"))
        })?;
        let top = if p == start_page { y_start } else { 0.0 };
        let bottom = if p == end_page { y_end } else { page_h };
        out.push(Region {
            name: name.to_string(),
            page: p,
            rect: typst_to_pdf_rect(x, top, w, bottom - top, page_h),
        });
    }
    Ok(out)
}
```

- [ ] **Step 4: Rewrite `recover_regions` to handle both schemas**

```rust
pub fn recover_regions(doc: &PagedDocument) -> Result<Manifest> {
    let page_heights: Vec<f64> = doc.pages.iter().map(|p| p.frame.height().to_pt()).collect();

    let label = Label::new(PicoStr::intern("region"))
        .ok_or_else(|| Error::Region("empty region label".into()))?;
    let elems = doc.introspector.query(&Selector::Label(label));

    // Parse every <region> metadata row first.
    let mut raws: Vec<RawRegion> = Vec::with_capacity(elems.len());
    for elem in &elems {
        let packed = elem
            .to_packed::<MetadataElem>()
            .ok_or_else(|| Error::Region("labelled element is not metadata".into()))?;
        let json = serde_json::to_value(&packed.value).map_err(|e| Error::Region(e.to_string()))?;
        raws.push(serde_json::from_value(json).map_err(|e| Error::Region(e.to_string()))?);
    }

    // Atomic rows convert directly (preserving query order); flow rows pair by name.
    let mut regions: Vec<Region> = Vec::new();
    let mut flow_starts: Vec<&RawRegion> = Vec::new();
    let mut flow_ends: BTreeMap<String, &RawRegion> = BTreeMap::new();

    for raw in &raws {
        match raw.role.as_deref() {
            Some("flow-start") => flow_starts.push(raw),
            Some("flow-end") => {
                flow_ends.insert(raw.name.clone(), raw);
            }
            _ => {
                // Atomic region ("box" or legacy role-less): requires w and h.
                let w = raw.w.ok_or_else(|| {
                    Error::Region(format!("atomic region '{}' missing w", raw.name))
                })?;
                let h = raw.h.ok_or_else(|| {
                    Error::Region(format!("atomic region '{}' missing h", raw.name))
                })?;
                let page_h = *page_heights.get(raw.page).ok_or_else(|| {
                    Error::Region(format!(
                        "region '{}' references missing page {}",
                        raw.name, raw.page
                    ))
                })?;
                regions.push(Region {
                    name: raw.name.clone(),
                    page: raw.page,
                    rect: typst_to_pdf_rect(raw.x, raw.y, w, h, page_h),
                });
            }
        }
    }

    for start in flow_starts {
        let end = flow_ends.get(&start.name).ok_or_else(|| {
            Error::Region(format!("flow region '{}' has no end marker", start.name))
        })?;
        let w = start
            .w
            .ok_or_else(|| Error::Region(format!("flow region '{}' start missing w", start.name)))?;
        regions.extend(split_rects(
            &start.name,
            start.page,
            start.x,
            start.y,
            w,
            end.page,
            end.y,
            &page_heights,
        )?);
    }

    Ok(Manifest {
        version: 0,
        regions,
        state: DocState::default(),
    })
}
```

(`BTreeMap` is already imported at the top of `manifest.rs`.)

- [ ] **Step 5: Write the recovery integration test** — append to `crates/inkapp-core/tests/pagination.rs`

```rust
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::compile_to_document;

#[test]
fn flow_region_recovers_one_rect_per_frame() {
    // Emit flow-start/flow-end markers directly (no prelude needed) around a tall
    // block on a short page, so the body spans several frames.
    let src = r#"
#set page(width: 200pt, height: 100pt, margin: 8pt)
#context [#metadata((name: "p", role: "flow-start", page: here().position().page - 1, x: here().position().x / 1pt, y: here().position().y / 1pt, w: 120.0)) <region>]
#block(height: 300pt, fill: luma(230))[]
#context [#metadata((name: "p", role: "flow-end", page: here().position().page - 1, x: here().position().x / 1pt, y: here().position().y / 1pt)) <region>]
"#;
    let doc = compile_to_document(src).unwrap();
    let m = recover_regions(&doc).unwrap();
    let p: Vec<_> = m.regions.iter().filter(|r| r.name == "p").collect();
    assert!(p.len() >= 2, "a 300pt body on an ~84pt page must split into ≥2 frames, got {}", p.len());
    // The frames are tagged with increasing page indices.
    let pages: Vec<usize> = p.iter().map(|r| r.page).collect();
    assert!(pages.windows(2).all(|w| w[0] < w[1]), "frames in page order: {pages:?}");
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p inkapp-core --lib manifest` → PASS (split_rects).
Run: `cargo test -p inkapp-core --test pagination` → PASS.
Run: `cargo test -p inkapp-core --test regions` → still green (atomic recovery unchanged).

- [ ] **Step 7: Commit**

```bash
git add crates/inkapp-core/src/manifest.rs crates/inkapp-core/tests/pagination.rs
git -c core.hooksPath=.githooks commit -m "inkapp-core: split-aware region recovery (split_rects + role schema)"
```

---

### Task 3: `#region(breakable:)` prelude

**Goal:** Add a `breakable` flag to the `#region` prelude — the default (boxed/atomic) path stays byte-identical; `breakable: true` emits start/end markers around a flowing body.

**Files:**
- Modify: `crates/inkapp-core/typst/region.typ`
- Test: `crates/inkapp-core/tests/region_prelude.rs` (extend with a breakable case) or `tests/pagination.rs`

**Acceptance Criteria:**
- [ ] `#region(name, body)` (default) emits one atomic `role: "box"` region, recovered as a single rect — checkbox golden/behaviour unchanged.
- [ ] `#region(name, body, breakable: true)` emits `flow-start`/`flow-end` markers; a tall body on a short page recovers as ≥2 regions sharing the name.

**Verify:** `cargo test -p inkapp-core --test pagination` → passes; `cargo test -p inkapp-core --test region_prelude` and `cargo test --test e2e -p inkapp-harness` (checkbox path) → still green.

**Steps:**

- [ ] **Step 1: Write the failing test** — append to `crates/inkapp-core/tests/pagination.rs`

```rust
use inkapp_core::render::compile_to_document_with_sources;
use inkapp_core::REGION_PRELUDE;

#[test]
fn prelude_breakable_splits_atomic_does_not() {
    let src = r#"#import "/inkapp/region.typ": *
#set page(width: 200pt, height: 100pt, margin: 8pt)
#region("p", [#block(height: 300pt, fill: luma(230))[]], breakable: true)
#region("c", box(width: 14pt, height: 14pt, stroke: 0.5pt))
"#;
    let sources = vec![(REGION_PRELUDE.0.to_string(), REGION_PRELUDE.1.to_string())];
    let doc = compile_to_document_with_sources(src, &sources).unwrap();
    let m = recover_regions(&doc).unwrap();
    assert!(
        m.regions.iter().filter(|r| r.name == "p").count() >= 2,
        "breakable region splits across frames"
    );
    assert_eq!(
        m.regions.iter().filter(|r| r.name == "c").count(),
        1,
        "atomic region stays a single rect"
    );
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p inkapp-core --test pagination -- prelude_breakable`
Expected: FAIL — the prelude doesn't accept `breakable` yet (compile error in the Typst source, surfaced as `Error::Compile`).

- [ ] **Step 3: Update `region.typ`**

```typ
// Framework prelude. region(name, body) emits <region>-labelled metadata for the
// laid-out body. By default the body is wrapped in a `box` (atomic — never breaks,
// one rect). With `breakable: true` the body flows (it may break across pages) and
// two markers (flow-start / flow-end) bound it; Rust recover_regions reconstructs
// the per-frame rects. The <region> label MUST attach to the metadata element, and
// here().position() gives a 1-based page we store 0-based; lengths are /1pt floats.
#let region(name, body, breakable: false) = {
  if not breakable {
    box[
      #context [
        #metadata((
          name: name,
          role: "box",
          page: here().position().page - 1,
          x: here().position().x / 1pt,
          y: here().position().y / 1pt,
          w: measure(body).width / 1pt,
          h: measure(body).height / 1pt,
        )) <region>
      ]
      #body
    ]
  } else {
    context [
      #metadata((
        name: name,
        role: "flow-start",
        page: here().position().page - 1,
        x: here().position().x / 1pt,
        y: here().position().y / 1pt,
        w: measure(body).width / 1pt,
      )) <region>
    ]
    body
    context [
      #metadata((
        name: name,
        role: "flow-end",
        page: here().position().page - 1,
        x: here().position().x / 1pt,
        y: here().position().y / 1pt,
      )) <region>
    ]
  }
}
```

- [ ] **Step 4: Run the test**

Run: `cargo test -p inkapp-core --test pagination -- prelude_breakable`
Expected: PASS.

- [ ] **Step 5: Guard the atomic path — run checkbox tests**

Run: `cargo test -p inkapp-core --test regions --test checkbox` and `cargo test -p inkapp-harness --test e2e`
Expected: green. The atomic branch only adds `role: "box"` to the metadata dict (a field `recover_regions` now reads); layout and the inspector PNG are unchanged.

- [ ] **Step 6: Commit**

```bash
git add crates/inkapp-core/typst/region.typ crates/inkapp-core/tests/pagination.rs
git -c core.hooksPath=.githooks commit -m "inkapp-core: #region gains breakable: true (start/end markers for split regions)"
```

---

### Task 4: Page-aware `attribute` + cross-page stitching

**Goal:** Make `attribute` take per-page strokes, never cross-attribute between pages, and stitch a logical region's ink across the pages it spans into a single `RegionInk`.

**Files:**
- Modify: `crates/inkapp-core/src/readback.rs` (new `attribute` signature + `attribute_page`)
- Modify: `crates/inkapp-harness/src/simulator.rs` (call `attribute_page`)
- Modify: `crates/inkapp-core/tests/readback.rs` (adapt existing callers; add page-aware tests)

**Acceptance Criteria:**
- [ ] `attribute(pages: &[Vec<Stroke>], manifest) -> Vec<RegionInk>` hit-tests page `p`'s strokes only against regions with `region.page == p`.
- [ ] One `RegionInk` per logical region name, with strokes concatenated across every page the region spans (stitching).
- [ ] `attribute_page(strokes, manifest)` wraps the single-page case.
- [ ] A region split across two pages stitches to one `RegionInk`; same-rect regions on different pages do not cross-attribute.

**Verify:** `cargo test -p inkapp-core --test readback` → passes; `cargo build --workspace` → clean.

**Steps:**

- [ ] **Step 1: Write failing tests** — add to `crates/inkapp-core/tests/readback.rs`

```rust
use inkapp_core::geometry::PdfRect;
use inkapp_core::ink::Stroke;
use inkapp_core::manifest::{Manifest, Region};
use inkapp_core::readback::{attribute, attribute_page};
use inkapp_core::geometry::PdfPoint;

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> PdfRect { PdfRect { x0, y0, x1, y1 } }
fn dot(x: f64, y: f64) -> Stroke { Stroke { points: vec![PdfPoint { x, y }], highlighter: false } }

#[test]
fn split_region_stitches_across_pages() {
    // One logical region "p" present on page 0 and page 1 (a split region).
    let m = Manifest {
        version: 0,
        regions: vec![
            Region { name: "p".into(), page: 0, rect: rect(0.0, 0.0, 100.0, 100.0) },
            Region { name: "p".into(), page: 1, rect: rect(0.0, 0.0, 100.0, 100.0) },
        ],
        ..Default::default()
    };
    let pages = vec![vec![dot(10.0, 10.0)], vec![dot(20.0, 20.0)]]; // one stroke on each page
    let out = attribute(&pages, &m);
    assert_eq!(out.len(), 1, "stitched to one RegionInk");
    assert_eq!(out[0].region, "p");
    assert_eq!(out[0].strokes.len(), 2, "ink from both pages");
}

#[test]
fn no_cross_page_attribution() {
    // Same rect, different pages, different names.
    let m = Manifest {
        version: 0,
        regions: vec![
            Region { name: "a".into(), page: 0, rect: rect(0.0, 0.0, 100.0, 100.0) },
            Region { name: "b".into(), page: 1, rect: rect(0.0, 0.0, 100.0, 100.0) },
        ],
        ..Default::default()
    };
    // Ink only on page 1.
    let pages = vec![vec![], vec![dot(50.0, 50.0)]];
    let out = attribute(&pages, &m);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].region, "b", "page-1 ink attributes to the page-1 region only");
}

#[test]
fn attribute_page_is_single_page_wrapper() {
    let m = Manifest {
        version: 0,
        regions: vec![Region { name: "a".into(), page: 0, rect: rect(0.0, 0.0, 100.0, 100.0) }],
        ..Default::default()
    };
    let out = attribute_page(&[dot(5.0, 5.0)], &m);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].region, "a");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p inkapp-core --test readback`
Expected: FAIL to compile — `attribute` signature differs / `attribute_page` missing.

- [ ] **Step 3: Rewrite `attribute` + add `attribute_page`** in `readback.rs`

```rust
use std::collections::HashMap;

/// Attribute per-page strokes to regions, then stitch each logical region's ink
/// across the pages it spans into one `RegionInk`. `pages[p]` holds page p's
/// strokes (that page's PDF space). A stroke on page p is tested ONLY against
/// regions with `region.page == p`, so same-rect regions on different pages never
/// cross-attribute. A stroke matches a region if any of its points lies in the
/// region's rect; a stroke may match (and be added to) multiple regions on its page.
/// Output order is the first-seen order of region names.
pub fn attribute(pages: &[Vec<Stroke>], manifest: &Manifest) -> Vec<RegionInk> {
    let mut order: Vec<String> = Vec::new();
    let mut by_name: HashMap<String, Vec<Stroke>> = HashMap::new();
    for region in &manifest.regions {
        let Some(strokes) = pages.get(region.page) else {
            continue;
        };
        for s in strokes {
            if s.points.iter().any(|p| region.rect.contains(p.x, p.y)) {
                if !by_name.contains_key(&region.name) {
                    order.push(region.name.clone());
                }
                by_name.entry(region.name.clone()).or_default().push(s.clone());
            }
        }
    }
    order
        .into_iter()
        .map(|name| {
            let strokes = by_name.remove(&name).unwrap_or_default();
            RegionInk { region: name, strokes }
        })
        .collect()
}

/// Single-page convenience: attribute one page's strokes (the common case for
/// single-page tests and the harness `simulate`).
pub fn attribute_page(strokes: &[Stroke], manifest: &Manifest) -> Vec<RegionInk> {
    attribute(&[strokes.to_vec()], manifest)
}
```

Remove the now-obsolete "does NOT filter by page / one page per cycle" caveat from the old doc comment (it's replaced by the new contract above).

- [ ] **Step 4: Update the harness `simulate` caller** in `crates/inkapp-harness/src/simulator.rs`

```rust
// was: let readback = attribute(&strokes, manifest);
let readback = attribute_page(&strokes, manifest);
```

Update the import: `use inkapp_core::readback::attribute_page;`.

- [ ] **Step 5: Fix any other single-page `attribute` callers**

Run from the worktree root: `rg -n "attribute\(" crates apps` and convert each single-page call to `attribute_page(...)` (the per-page `attribute` callers are only `App::step`, handled in Task 5).

- [ ] **Step 6: Run tests + build**

Run: `cargo test -p inkapp-core --test readback` → PASS.
Run: `cargo build --workspace` → clean.

- [ ] **Step 7: Commit**

```bash
git add crates/inkapp-core/src/readback.rs crates/inkapp-harness/src/simulator.rs crates/inkapp-core/tests/readback.rs
git -c core.hooksPath=.githooks commit -m "inkapp-core: page-aware attribute + cross-page ink stitching"
```

---

### Task 5: Runtime + device per-page ink

**Goal:** Carry a page count through the runtime, make preserved ink per-page, and switch `App::step`'s ink input to per-page strokes (the `Device` trait stays unchanged — callers loop over pages).

**Files:**
- Modify: `crates/inkapp-core/src/runtime.rs` (`RenderedDoc`, `DocEntry`, `DocSet`, `App::step`, `render_document_in`)
- Modify: `crates/inkapp-core/tests/loop_driver.rs`, `crates/inkapp-core/tests/stepper_loop.rs` (per-page ink shape)
- Modify: `crates/inkapp-harness/tests/app_loop.rs`, `crates/inkapp-harness/tests/agenda_loop.rs` (per-page ink shape)
- Modify: `apps/*/tests/*` step callers as found by grep

**Acceptance Criteria:**
- [ ] `RenderedDoc` and `DocEntry` carry `page_count: usize`; `render_document_in` sets it from `compiled.pages.len()`.
- [ ] `DocSet` preserved ink is `Vec<Vec<Stroke>>`; `DocSet::ink(key) -> &[Vec<Stroke>]`; new `DocSet::page_count(key)`.
- [ ] `App::step` takes `ink_by_key: &HashMap<String, Vec<Vec<Stroke>>>` and attributes via the page-aware `attribute`.
- [ ] `render_document_in` returns `page_count > 1` for a tall doc under a short geometry.
- [ ] All existing loop tests pass with the per-page ink shape.

**Verify:** `cargo test --workspace` → green.

**Steps:**

- [ ] **Step 1: Write a failing page-count test** — append to `crates/inkapp-core/tests/pagination.rs`

```rust
use inkapp_core::runtime::render_document_in;
use inkapp_core::crypto::Key;

#[test]
fn rendered_doc_reports_multiple_pages() {
    let doc = tall_doc(); // from Task 1
    let key = Key::from_bytes([7u8; 32]);
    let rd = render_document_in(&doc, 1, &key, PageGeom { w: 420.0, h: 180.0, margin: 16.0 }).unwrap();
    assert!(rd.page_count > 1, "tall doc on a short page is multi-page, got {}", rd.page_count);
}
```

(Confirm the `Key::from_bytes([..;32])` constructor name against `crypto.rs`; the harness `common::test_key()` shows the canonical construction if it differs.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p inkapp-core --test pagination -- rendered_doc_reports_multiple_pages`
Expected: FAIL — `RenderedDoc` has no `page_count`.

- [ ] **Step 3: Add `page_count` to `RenderedDoc`** and set it in `render_document_in`

```rust
pub struct RenderedDoc {
    pub key: DocKey,
    pub pdf: Vec<u8>,
    pub manifest: Manifest,
    pub page_h: f64,
    /// Number of pages this document paginated to under its render geometry.
    pub page_count: usize,
    pub hash: u64,
}
```

In `render_document_in`, after compiling:

```rust
let page_count = compiled.pages.len();
```

and add `page_count,` to the returned `RenderedDoc`.

- [ ] **Step 4: Make `DocEntry` ink per-page + add `page_count`**

```rust
struct DocEntry {
    manifest: Manifest,
    page_h: f64,
    page_count: usize,
    hash: u64,
    version: u64,
    /// Accumulated user ink (PDF space) per page — preserved across re-renders by key.
    ink: Vec<Vec<Stroke>>,
}
```

Update `DocSet`:

```rust
/// The preserved per-page ink on `key` (empty slice if none / unknown).
pub fn ink(&self, key: &DocKey) -> &[Vec<Stroke>] {
    self.entries.get(&key.0).map(|e| e.ink.as_slice()).unwrap_or(&[])
}

/// The page count last rendered for `key`.
pub fn page_count(&self, key: &DocKey) -> Option<usize> {
    self.entries.get(&key.0).map(|e| e.page_count)
}
```

In `App::render`, build each `DocEntry` with `page_count: rd.page_count` and `ink: Vec::new()`.

- [ ] **Step 5: Switch `App::step` to per-page ink**

Change the signature:

```rust
pub async fn step(
    &mut self,
    set: &mut DocSet,
    ink_by_key: &HashMap<String, Vec<Vec<Stroke>>>,
) -> Result<Cycle<Msg>>
where
    Msg: Clone,
{
```

In the decode loop (phase 1), replace the flat-stroke lookup + `attribute` call:

```rust
let Some(pages) = ink_by_key.get(&doc.key.0) else {
    continue;
};
let Some(entry) = set.entries.get(&doc.key.0) else {
    continue;
};
guard_version(entry.version, &entry.manifest)?;
let region_ink = attribute(pages, &entry.manifest);
for c in &doc.flow {
    decoded.extend(c.decode(&region_ink, &entry.manifest));
}
```

In phase 5 (rebuild entries), preserve and append per page:

```rust
for rd in next_rendered {
    // Preserve prior per-page ink for this key, then append this cycle's input.
    let mut ink: Vec<Vec<Stroke>> = set
        .entries
        .get(&rd.key.0)
        .map(|e| e.ink.clone())
        .unwrap_or_default();
    if let Some(new_pages) = ink_by_key.get(&rd.key.0) {
        if ink.len() < new_pages.len() {
            ink.resize(new_pages.len(), Vec::new());
        }
        for (p, strokes) in new_pages.iter().enumerate() {
            ink[p].extend(strokes.iter().cloned());
        }
    }
    let is_changed = changed.contains_key(rd.key.0.as_str());
    new_entries.insert(
        rd.key.0.clone(),
        DocEntry {
            manifest: rd.manifest.clone(),
            page_h: rd.page_h,
            page_count: rd.page_count,
            hash: rd.hash,
            version: self.version,
            ink,
        },
    );
    if is_changed {
        rendered_out.push(rd);
    }
}
```

- [ ] **Step 6: Migrate every `step` caller to per-page ink**

Enumerate from the worktree root: `rg -n "\.step\(" crates apps`. For each, wrap the per-doc strokes in a single page (existing content is single-page): change `ink.insert(key, strokes)` to `ink.insert(key, vec![strokes])` and the map type to `HashMap<String, Vec<Vec<Stroke>>>`. Known sites:
  - `crates/inkapp-harness/tests/app_loop.rs` (two inserts → `vec![device_ink(...)]`)
  - `crates/inkapp-harness/tests/agenda_loop.rs`
  - `crates/inkapp-core/tests/loop_driver.rs`
  - `crates/inkapp-core/tests/stepper_loop.rs`
  - any `apps/*/tests/*` that call `.step(`

`set.ink(&key)` assertions (`!set.ink(&x).is_empty()`) keep working — the outer `Vec` is non-empty when a page has ink.

- [ ] **Step 7: Run the full suite**

Run: `cargo test --workspace`
Expected: green. (Use `--workspace`, not `-p inkapp-core`, so `apps/` call sites are compiled — per the repo's app-tree lesson.)

- [ ] **Step 8: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "inkapp-core: per-page ink through the runtime (page_count, per-page DocSet ink, page-aware step)"
```

---

### Task 6: `Passage` Capture component

**Goal:** Add a minimal reusable Capture component whose single **breakable** region captures any ink on it as one region regardless of pagination — the vehicle that exercises true split-stitch end to end.

**Files:**
- Create: `crates/inkapp-core/src/components/passage.rs`
- Modify: `crates/inkapp-core/src/components/mod.rs` (`pub mod passage;`)
- Test: `crates/inkapp-core/tests/passage.rs` (new)

**Acceptance Criteria:**
- [ ] `Passage<M>` with `new(name, lines)` and `with_msg(name, lines, on_capture)`; `render` emits `#region(name, breakable: true)[…]`; `decode` emits `on_capture` once iff any ink landed in the (stitched) region.
- [ ] A `Passage` rendered in a `Document`, recovered, and given a stroke in its region decodes to one message.

**Verify:** `cargo test -p inkapp-core --test passage` → passes; `cargo build --workspace` → clean.

**Steps:**

- [ ] **Step 1: Write the failing tests** — `crates/inkapp-core/tests/passage.rs`

```rust
use inkapp_core::components::passage::Passage;
use inkapp_core::component::Component;
use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::Manifest;

#[derive(Debug, Clone, PartialEq)]
enum M { Captured }

#[test]
fn decode_fires_once_on_any_ink() {
    let p = Passage::with_msg("notes", &["hello world", "second line"], M::Captured);
    let ink = vec![RegionInk {
        region: "notes".into(),
        strokes: vec![Stroke { points: vec![PdfPoint { x: 1.0, y: 1.0 }], highlighter: true }],
    }];
    assert_eq!(p.decode(&ink, &Manifest::default()), vec![M::Captured]);
    assert!(p.decode(&[], &Manifest::default()).is_empty());
}

#[test]
fn render_emits_breakable_region() {
    let p = Passage::with_msg("notes", &["a", "b"], M::Captured);
    let src = p.render(&mut inkapp_core::component::RenderCx::new(0));
    assert!(src.contains("#region(\"notes\""), "calls the region prelude: {src}");
    assert!(src.contains("breakable: true"), "as a breakable region: {src}");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p inkapp-core --test passage`
Expected: FAIL — `passage` module doesn't exist.

- [ ] **Step 3: Implement `Passage`** — `crates/inkapp-core/src/components/passage.rs`

```rust
//! `Passage` — a Capture-mode component: a breakable block of read-only text that
//! captures any ink on it as a single region, regardless of how it paginates. It
//! carries the value-message to emit when inked (Elm's value-message, no stored
//! closure), so it drops into any `view` flow. It is the component that exercises a
//! region split across a page break (the framework stitches per-page ink into one
//! RegionInk before `decode`).

use crate::component::{Component, RenderCx};
use crate::components::esc_typst_str;
use crate::ink::RegionInk;
use crate::manifest::Manifest;
use crate::render::is_valid_region_name;

/// A breakable text passage bound to one named region, carrying `on_capture` to
/// emit when any ink lands on it. `M` defaults to `()` for a presence-only passage.
pub struct Passage<M = ()> {
    name: String,
    lines: Vec<String>,
    on_capture: M,
}

impl Passage<()> {
    /// A presence-only passage (no message).
    pub fn new(name: &str, lines: &[&str]) -> Self {
        Self {
            name: name.to_string(),
            lines: lines.iter().map(|s| s.to_string()).collect(),
            on_capture: (),
        }
    }
}

impl<M> Passage<M> {
    /// A passage carrying `on_capture` to emit when inked.
    pub fn with_msg(name: &str, lines: &[&str], on_capture: M) -> Self {
        Self {
            name: name.to_string(),
            lines: lines.iter().map(|s| s.to_string()).collect(),
            on_capture,
        }
    }

    /// Whether any ink landed in this passage's (stitched) region.
    pub fn read(&self, ink: &[RegionInk], _manifest: &Manifest) -> bool {
        ink.iter()
            .filter(|ri| ri.region == self.name)
            .any(|ri| !ri.strokes.is_empty())
    }
}

impl<M: Clone> Component for Passage<M> {
    type Msg = M;

    fn render(&self, _cx: &mut RenderCx) -> String {
        assert!(
            is_valid_region_name(&self.name),
            "passage region name must be a valid region name, got: {:?}",
            self.name
        );
        let name = &self.name;
        // Each line is injected as a Typst *string expression* (`#"..."`) so its
        // markup chars stay literal; lines are separated by linebreaks so the body
        // is a single flowing (breakable) block.
        let body: String = self
            .lines
            .iter()
            .map(|l| format!("#\"{}\" #linebreak() ", esc_typst_str(l)))
            .collect();
        format!("#region(\"{name}\", [{body}], breakable: true)\n")
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<M> {
        if self.read(ink, manifest) {
            vec![self.on_capture.clone()]
        } else {
            vec![]
        }
    }
}
```

(The `#region` prelude is always registered and imported by the runtime, so `Passage` needs no `typst_sources` override.)

- [ ] **Step 4: Register the module** in `crates/inkapp-core/src/components/mod.rs`

```rust
pub mod passage;
```

(Insert in alphabetical order, after `pub mod notice;`.)

- [ ] **Step 5: Run tests + build**

Run: `cargo test -p inkapp-core --test passage` → PASS.
Run: `cargo build --workspace` → clean.

- [ ] **Step 6: Commit**

```bash
git add crates/inkapp-core/src/components/passage.rs crates/inkapp-core/src/components/mod.rs crates/inkapp-core/tests/passage.rs
git -c core.hooksPath=.githooks commit -m "inkapp-core: Passage — a breakable Capture component (split-stitch vehicle)"
```

---

### Task 7: Two-profile device-blindness harness test (the headline)

**Goal:** Prove the bet: identical content rendered on two `PageGeom` profiles paginates to **different page counts** yet decodes to **identical `Msg`s** — through the real `.rm` write/read path, page-aware attribution, and cross-page stitching, with a per-token highlight, a split passage, and a checkbox.

**Files:**
- Create: `crates/inkapp-harness/tests/pagination_device_blind.rs`

**Acceptance Criteria:**
- [ ] One `Document` (highlight body + `Passage` + `Checkbox`) renders to **different** `page_count`s under profile A and profile B.
- [ ] The same logical gestures (highlight a chosen token, ink the passage, tick the checkbox), mapped to each profile's per-page device ink and round-tripped through `Remarkable::write_ink`/`read_ink`, decode to the **same** sorted `Msg` set across both profiles.

**Verify:** `cargo test -p inkapp-harness --test pagination_device_blind` → passes.

**Steps:**

- [ ] **Step 1: Write the test** — `crates/inkapp-harness/tests/pagination_device_blind.rs`

```rust
mod common;

use std::collections::BTreeSet;

use inkapp_core::component::{Component, RenderCx};
use inkapp_core::components::checkbox::Checkbox;
use inkapp_core::components::highlight_text::HighlightableText;
use inkapp_core::components::passage::Passage;
use inkapp_core::device::Device;
use inkapp_core::document::Document;
use inkapp_core::geometry::PageGeom;
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::Manifest;
use inkapp_core::readback::attribute;
use inkapp_core::runtime::render_document_in;
use inkapp_remarkable::Remarkable;

/// The test app's messages. `Ord` so we can compare sets independent of order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Msg {
    Hi(String),
    Note,
    Done,
}

/// A bespoke content component: per-token highlightable body that emits Hi(token).
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

/// Build the same logical document each time (tall, so geometry changes page count).
fn doc() -> Document<Msg> {
    // 30 tokens; we highlight index 7. Distinct strings so the highlighted token is
    // unambiguous regardless of pagination.
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

/// Build per-page device-round-tripped ink for the given targets. Each target is a
/// (region name, is_highlighter) pair; for every recovered region with that name we
/// synthesize a swipe across its rect on that region's page, then write+read it
/// through the real .rm path so the test exercises the byte path.
fn device_pages(
    manifest: &Manifest,
    device: &Remarkable,
    page_h: f64,
    page_count: usize,
    targets: &[(&str, bool)],
) -> Vec<Vec<Stroke>> {
    let mut per_page: Vec<Vec<Stroke>> = vec![Vec::new(); page_count];
    for (name, hl) in targets {
        for r in manifest.regions.iter().filter(|r| &r.name == name) {
            let cy = (r.rect.y0 + r.rect.y1) / 2.0;
            let stroke = Stroke {
                points: vec![
                    inkapp_core::geometry::PdfPoint { x: r.rect.x0, y: cy },
                    inkapp_core::geometry::PdfPoint { x: r.rect.x1, y: cy },
                ],
                highlighter: *hl,
            };
            per_page[r.page].push(stroke);
        }
    }
    // Round-trip each page through the device's real write+read.
    per_page
        .into_iter()
        .map(|strokes| {
            let bytes = device.write_ink(&strokes, page_h).unwrap();
            device.read_ink(&bytes, page_h).unwrap()
        })
        .collect()
}

/// Render + ink + attribute + decode for one profile; returns (page_count, msgs).
fn run_profile(geom: PageGeom) -> (usize, BTreeSet<Msg>) {
    let key = common::test_key();
    let device = Remarkable::new();
    let d = doc();
    let rd = render_document_in(&d, 1, &key, geom).unwrap();

    // Highlight token index 7 ("word07"), ink the passage, tick the checkbox.
    let targets: &[(&str, bool)] = &[("tok-7", true), ("notes", true), ("done", false)];
    let pages = device_pages(&rd.manifest, &device, rd.page_h, rd.page_count, targets);

    let region_ink = attribute(&pages, &rd.manifest);
    let mut msgs = BTreeSet::new();
    for c in &d.flow {
        for m in c.decode(&region_ink, &rd.manifest) {
            msgs.insert(m);
        }
    }
    (rd.page_count, msgs)
}

#[test]
fn same_content_two_profiles_decode_identically() {
    // Profile A: standard. Profile B: shorter page → more pages, different breaks.
    let (pages_a, msgs_a) = run_profile(PageGeom::default());
    let (pages_b, msgs_b) = run_profile(PageGeom { w: 420.0, h: 240.0, margin: 16.0 });

    assert_ne!(
        pages_a, pages_b,
        "the two profiles must paginate to different page counts (A={pages_a}, B={pages_b})"
    );
    let expected: BTreeSet<Msg> =
        [Msg::Hi("word07".into()), Msg::Note, Msg::Done].into_iter().collect();
    assert_eq!(msgs_a, expected, "profile A decoded the expected messages");
    assert_eq!(
        msgs_a, msgs_b,
        "decoded messages are identical across profiles (page-/device-blind)"
    );
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p inkapp-harness --test pagination_device_blind`
Expected: PASS. If the two profiles happen to produce the same page count, increase the token/line counts or lower profile B's height until `pages_a != pages_b` (the content must be tall enough that halving the usable height changes the page count). If the highlighted token resolves to a different word, confirm `tok-7` maps to `word07` (token index == region suffix).

- [ ] **Step 3: Commit**

```bash
git add crates/inkapp-harness/tests/pagination_device_blind.rs
git -c core.hooksPath=.githooks commit -m "inkapp-harness: two-profile device-blindness test (identical Msgs, different page counts)"
```

---

### Task 8: appdx update + caveat cleanup + final green

**Goal:** Make `docs/appdx.md` (the definition of done) reflect that pagination is built, remove the single-page caveats left in code, and confirm the whole workspace is green.

**Files:**
- Modify: `docs/appdx.md` ("Documents, pages, and devices", Status banner, parking lot / FUTURE cross-ref)
- Verify-only: `crates/inkapp-core/src/runtime.rs`, `crates/inkapp-core/src/readback.rs`, `crates/inkapp-core/src/render.rs` (no stale single-page comments remain)

**Acceptance Criteria:**
- [ ] `docs/appdx.md` "Documents, pages, and devices" reads as built (paginate to N pages per device; regions recovered from every frame; per-page ink stitched into content-relative regions before decode), keeping the author-facing rules.
- [ ] The Status banner notes pagination built; remaining future work is *(doc × device)* fan-out plus the already-future items.
- [ ] No stale "single-page" caveat remains in code (`rg -n "single-page|Single-page|one page per cycle"` over `crates/` returns nothing meaningful).
- [ ] `cargo test --workspace` is green.

**Steps:**

- [ ] **Step 1: Update the "Documents, pages, and devices" section** of `docs/appdx.md`

Change the framing from promise to built. Replace the lead paragraph's implication that this is aspirational with a built statement, e.g. add after the diagram:

```markdown
*(Built.)* The framework paginates a content flow to N pages per device profile
(page geometry is a render input), recovers each region from **every frame** it
touches — splitting a region that crosses a page break into one rect per frame — and
lifts per-page ink back into content-relative regions, **stitching** a split
region's ink into one logical `RegionInk` before the component decodes it. Components
stay page- and device-blind: the same content on two device profiles paginates to
different page counts and decodes to identical messages (proved in the harness).
Simultaneous *(logical doc × multiple devices)* fan-out in one run remains future
(see the threat model's "multi-device per user").
```

- [ ] **Step 2: Update the Status banner** (top of `docs/appdx.md`)

Add pagination to the built list and adjust "what remains". Example edit to the banner prose: note that **pagination** (one content flow → N-page, device-parametric render with split-region recovery and per-page ink stitching) is now built, and that the only remaining non-future item folded in is *(doc × device)* fan-out, which is itself flagged future.

- [ ] **Step 3: Update the parking lot / FUTURE cross-reference**

Ensure the open-questions/parking-lot list names *simultaneous per-device fan-out* as the remaining near-future refinement (additive: per-device ink streams in one `DocSet`), not "single-page only".

- [ ] **Step 4: Sweep for stale code caveats**

Run: `rg -n "single-page|Single-page|one page per cycle|Single-page only this spec" crates`
Expected: nothing. (The `runtime.rs` const comment was removed in Task 1; the `attribute` caveat was rewritten in Task 4.) Fix any stragglers (e.g. a comment in `render.rs`).

- [ ] **Step 5: Final workspace verification**

Run: `cargo test --workspace`
Expected: green.
Run: `cargo fmt --check` (the `.githooks` pre-commit runs it anyway)
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add docs/appdx.md crates/
git -c core.hooksPath=.githooks commit -m "appdx: pagination built — N-page device-parametric render with split-region recovery + ink stitching"
```

---

## Self-Review

**Spec coverage:**
- Parametric page geometry → Task 1. ✓
- Split-aware region recovery (markers + Rust `split_rects`) → Tasks 2 (Rust) + 3 (prelude). ✓
- Page-aware attribution + cross-page stitching → Task 4. ✓
- Runtime/device per-page ink (`page_count`, per-page `DocSet` ink, per-page `step`) → Task 5. ✓
- `Passage` Capture component → Task 6. ✓
- Two-profile device-blindness proof + split-recovery + stitching tests → Tasks 2/4 (unit), 7 (headline). ✓
- appdx update + caveat removal → Task 8. ✓
- `breakable:false` for affordances → already true (the `#region` `box` branch is the default; Task 3 keeps it byte-identical). ✓

**Type consistency:** `PageGeom`, `split_rects` (same arg order in def + all call sites), `attribute(&[Vec<Stroke>], …)` / `attribute_page`, `RenderedDoc.page_count`, `DocEntry.ink: Vec<Vec<Stroke>>`, `DocSet::ink -> &[Vec<Stroke>]`, `App::step(ink_by_key: &HashMap<String, Vec<Vec<Stroke>>>)`, `Passage::with_msg`, region roles `"box"`/`"flow-start"`/`"flow-end"` — all consistent across tasks.

**Placeholder scan:** No TBD/TODO; every code step shows the code. Two execution-time tunings are explicitly flagged (Task 7 content sizes so page counts differ; `Key` constructor name to confirm against `crypto.rs`/`common::test_key`) — these are verification adjustments, not missing content.

**Scope fences honored:** no `inkapp-core::cache` module, no connector-crate edits, `lib.rs` change limited to re-exports, `appdx.md` edits localized.
