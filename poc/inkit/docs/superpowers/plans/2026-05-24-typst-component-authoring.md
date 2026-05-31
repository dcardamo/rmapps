# Typst Component Authoring ("T") Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Typst component-authoring seam — a multi-file Typst `World`, a framework `#region` prelude, and a `Component` source-declaration hook — and prove it end-to-end by authoring `Checkbox`'s render half as a real `.typ` file.

**Architecture:** A component's render half splits in two: presentation lives in an `include_str!`-baked `.typ` function; the Rust `render()` emits a *call* to it wrapped in the framework's `#region(name, body)` prelude primitive (which emits the `<region>` metadata `recover_regions` already reads back). `InkWorld` gains a virtual filesystem so the assembled `main.typ` can `#import` those baked sources; the render driver collects each flow component's declared sources, registers them, and prepends the imports. `compile_to_document(src)` stays unchanged (≈20 callers) — a new sources-aware path carries the authored case.

**Tech Stack:** Rust, Typst 0.14.2 (used as a library: `typst`, `typst-pdf`), `serde_json` for metadata round-trip.

---

### Task 1: Multi-file `InkWorld` + sources-aware compile path

**Goal:** `InkWorld` can serve additional named Typst sources beyond `main.typ`, so an assembled document can `#import` them; a new compile entry point threads those sources in without disturbing the existing single-arg `compile_to_document`.

**Files:**
- Modify: `crates/inkapp-core/src/world.rs` (add `with_sources` constructor + source map)
- Modify: `crates/inkapp-core/src/render.rs` (add `compile_to_document_with_sources`)
- Test: `crates/inkapp-core/tests/multifile_world.rs` (create)

**Acceptance Criteria:**
- [ ] `InkWorld::with_sources(main, sources)` registers each `(path, text)` so Typst's `source()` resolves it.
- [ ] `compile_to_document_with_sources(src, &sources)` compiles a `main.typ` that `#import`s a registered source.
- [ ] `compile_to_document(src)` is unchanged in signature and behavior (delegates with an empty source set).
- [ ] A `main.typ` that imports an unregistered path still fails to compile (no silent success).

**Verify:** `cargo test -p inkapp-core --test multifile_world` → 2 passing tests.

**Steps:**

- [ ] **Step 1: Write the failing test**

Create `crates/inkapp-core/tests/multifile_world.rs`:

```rust
use inkapp_core::render::{compile_to_document, compile_to_document_with_sources};

#[test]
fn imported_source_resolves_and_compiles() {
    let lib = (
        "/lib/greet.typ".to_string(),
        "#let greet(who) = [Hello #who]\n".to_string(),
    );
    let main = "#set page(width: 120pt, height: 60pt)\n\
                #import \"/lib/greet.typ\": *\n\
                #greet(\"world\")\n";
    let doc = compile_to_document_with_sources(main, &[lib]).expect("compiles with import");
    assert_eq!(doc.pages.len(), 1);
}

#[test]
fn missing_import_fails() {
    let main = "#set page(width: 120pt, height: 60pt)\n\
                #import \"/lib/absent.typ\": *\n\
                #absent()\n";
    // No sources registered: the import cannot resolve, so compilation fails.
    assert!(compile_to_document_with_sources(main, &[]).is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inkapp-core --test multifile_world`
Expected: FAIL — `compile_to_document_with_sources` does not exist (does not compile).

- [ ] **Step 3: Add the source map to `InkWorld`**

In `crates/inkapp-core/src/world.rs`, add `use std::collections::HashMap;` at the top, add a `sources` field, and add the `with_sources` constructor. Keep `new` delegating so existing callers are untouched:

```rust
pub struct InkWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    main: Source,
    sources: HashMap<FileId, Source>,
}

impl InkWorld {
    pub fn new(src: &str) -> Self {
        Self::with_sources(src, &[])
    }

    /// Like `new`, but registers additional named Typst sources (e.g. component
    /// `.typ` files) so the main source can `#import` them. `sources` is a list of
    /// `(virtual_path, source_text)`; paths are root-absolute (leading `/`) to
    /// match `#import "/path.typ"`.
    pub fn with_sources(src: &str, sources: &[(String, String)]) -> Self {
        let mut fonts = Vec::new();
        for data in typst_assets::fonts() {
            let bytes = Bytes::new(data.to_vec());
            for face in Font::iter(bytes) {
                fonts.push(face);
            }
        }
        let book = FontBook::from_fonts(&fonts);
        let main_id = FileId::new(None, VirtualPath::new("main.typ"));
        let main = Source::new(main_id, src.into());
        let sources = sources
            .iter()
            .map(|(path, text)| {
                let id = FileId::new(None, VirtualPath::new(path));
                (id, Source::new(id, text.clone()))
            })
            .collect();
        Self {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(book),
            fonts,
            main,
            sources,
        }
    }
}
```

- [ ] **Step 4: Serve the registered sources from `World::source`**

In the same file, replace the `source` method body so registered sources resolve (main first, then the map, then `NotFound`):

```rust
    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            Ok(self.main.clone())
        } else if let Some(s) = self.sources.get(&id) {
            Ok(s.clone())
        } else {
            Err(FileError::NotFound(
                id.vpath().as_rootless_path().to_owned(),
            ))
        }
    }
```

(`file()` keeps returning `NotFound`: Typst resolves `#import` of a `.typ` through `source()`, not `file()`.)

- [ ] **Step 5: Add `compile_to_document_with_sources`**

In `crates/inkapp-core/src/render.rs`, refactor so both entry points share `InkWorld`:

```rust
/// Compile Typst source to a laid-out document. The single-arg form authors no
/// component `.typ` files (used by the harness and most tests).
pub fn compile_to_document(src: &str) -> Result<PagedDocument> {
    compile_to_document_with_sources(src, &[])
}

/// Compile with additional registered Typst sources the main source may `#import`
/// (component render halves + the framework prelude).
pub fn compile_to_document_with_sources(
    src: &str,
    sources: &[(String, String)],
) -> Result<PagedDocument> {
    let world = InkWorld::with_sources(src, sources);
    typst::compile::<PagedDocument>(&world)
        .output
        .map_err(|d| Error::Compile(format!("{d:?}")))
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p inkapp-core --test multifile_world`
Expected: PASS (2 tests).
Run: `cargo test -p inkapp-core` and `cargo test -p inkapp-harness`
Expected: PASS — existing `compile_to_document` callers unaffected.

- [ ] **Step 7: Commit**

```bash
git add crates/inkapp-core/src/world.rs crates/inkapp-core/src/render.rs crates/inkapp-core/tests/multifile_world.rs
git commit -m "inkapp-core: multi-file InkWorld + sources-aware compile path (Typst authoring seam, step 1)"
```

---

### Task 2: The `#region` prelude + `Component` source hook + driver assembly

**Goal:** Provide the framework `#region(name, body)` Typst primitive, a `Component::typst_sources()` declaration hook (default empty), and a render driver that collects each flow component's sources (plus the prelude), prepends the `#import`s, and compiles via the sources-aware path. Prove the prelude recovers a region rect matching the inline pattern it replaces.

**Files:**
- Create: `crates/inkapp-core/typst/region.typ` (baked prelude)
- Modify: `crates/inkapp-core/src/component.rs` (add `typst_sources` default method)
- Modify: `crates/inkapp-core/src/runtime.rs` (prelude const, source collection, import assembly, `compile_document`)
- Test: `crates/inkapp-core/tests/region_prelude.rs` (create)

**Acceptance Criteria:**
- [ ] `region.typ` defines `#let region(name, body)` emitting `<region>`-labelled metadata with `name/page/x/y/w/h`, then placing `body`.
- [ ] `Component::typst_sources(&self) -> Vec<(String, String)>` exists with a default returning `Vec::new()`.
- [ ] `document_source` prepends one `#import "<path>": *` line per collected source (prelude + components, deduped by path) before the page header.
- [ ] `compile_document(doc)` compiles a document through the sources-aware path with all collected sources registered.
- [ ] A document body using `#region("r", [..])` recovers a region named `r` whose rect matches a hand-written inline `<region>` of the same body within 0.01pt on every edge.

**Verify:** `cargo test -p inkapp-core --test region_prelude` → passing.

**Steps:**

- [ ] **Step 1: Write the failing test**

Create `crates/inkapp-core/tests/region_prelude.rs`. It compiles the same content twice — once via the `#region` prelude (through `compile_to_document_with_sources` with the baked prelude registered), once via the legacy inline `region_metadata` helper — and asserts the recovered rects match:

```rust
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::{compile_to_document, compile_to_document_with_sources};
use inkapp_core::runtime::REGION_PRELUDE;
use inkapp_core::widget::region_metadata;

const PAGE: &str = "#set page(width: 200pt, height: 120pt, margin: 12pt)\n";

#[test]
fn region_prelude_matches_inline_pattern() {
    // Via the prelude: import #region, wrap a fixed-size box.
    let prelude = (REGION_PRELUDE.0.to_string(), REGION_PRELUDE.1.to_string());
    let via_prelude = format!(
        "{PAGE}#import \"{}\": *\n#region(\"r\", box(width: 14pt, height: 14pt, stroke: 0.5pt))\n",
        REGION_PRELUDE.0
    );
    let d1 = compile_to_document_with_sources(&via_prelude, &[prelude]).unwrap();
    let m1 = recover_regions(&d1).unwrap();
    let r1 = m1.regions.iter().find(|r| r.name == "r").expect("prelude region");

    // Via the legacy inline helper at the same flow position (placed at the same
    // laid-out point: top-left of the content area, page 0).
    // region_metadata takes Typst-space coords; the box lands at the margin (12,12).
    let inline = format!("{PAGE}{}", region_metadata("r", 0, 12.0, 12.0, 14.0, 14.0));
    let d2 = compile_to_document(&inline).unwrap();
    let m2 = recover_regions(&d2).unwrap();
    let r2 = m2.regions.iter().find(|r| r.name == "r").expect("inline region");

    for (a, b, edge) in [
        (r1.rect.x0, r2.rect.x0, "x0"),
        (r1.rect.y0, r2.rect.y0, "y0"),
        (r1.rect.x1, r2.rect.x1, "x1"),
        (r1.rect.y1, r2.rect.y1, "y1"),
    ] {
        assert!((a - b).abs() < 0.01, "edge {edge}: prelude {a} vs inline {b}");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inkapp-core --test region_prelude`
Expected: FAIL — `REGION_PRELUDE` does not exist (does not compile).

- [ ] **Step 3: Write the prelude `.typ`**

Create `crates/inkapp-core/typst/region.typ`:

```typst
// Framework prelude: region(name, body) emits <region>-labelled metadata for the
// laid-out body, then places the body. recover_regions queries the <region> label
// and downcasts the MetadataElem, so the label MUST attach to the metadata element.
// here().position() gives a 1-based page; we store 0-based. Lengths are divided by
// 1pt to unitless floats. measure(body) gives the body's own size.
#let region(name, body) = box[
  #context [
    #metadata((
      name: name,
      page: here().position().page - 1,
      x: here().position().x / 1pt,
      y: here().position().y / 1pt,
      w: measure(body).width / 1pt,
      h: measure(body).height / 1pt,
    )) <region>
  ]
  #body
]
```

- [ ] **Step 4: Add the `typst_sources` hook to `Component`**

In `crates/inkapp-core/src/component.rs`, add a default method to the trait:

```rust
    /// The Typst source file(s) this component's `render` output `#import`s, as
    /// `(root-absolute virtual path, source text)`. Default: none (the component
    /// builds its Typst inline). Authored components override this to register
    /// their `.typ` render half; the render driver imports each one into `main.typ`.
    fn typst_sources(&self) -> Vec<(String, String)> {
        Vec::new()
    }
```

- [ ] **Step 5: Add prelude const, source collection, import assembly, and `compile_document` to the driver**

In `crates/inkapp-core/src/runtime.rs`, add the prelude const near the top (after the existing `use`s):

```rust
/// The framework Typst prelude, baked into the binary. Always registered and
/// imported so any component (and `#region`) is in scope.
pub const REGION_PRELUDE: (&str, &str) = ("/inkapp/region.typ", include_str!("../typst/region.typ"));
```

Add a source collector (prelude + every flow component's sources, deduped by path, prelude first):

```rust
/// Collect the Typst sources to register for this document: the prelude plus each
/// component's declared sources, deduplicated by path (first occurrence wins).
pub fn collect_typst_sources<M>(doc: &Document<M>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![(REGION_PRELUDE.0.to_string(), REGION_PRELUDE.1.to_string())];
    for c in &doc.flow {
        for src in c.typst_sources() {
            if !out.iter().any(|(p, _)| p == &src.0) {
                out.push(src);
            }
        }
    }
    out
}
```

Replace `document_source` so it prepends an `#import "<path>": *` line per collected source before the page header:

```rust
/// Assemble a document's Typst source: `#import` lines for the prelude and every
/// authored component source, then a page header, then each component's render in
/// flow order.
pub fn document_source<M>(doc: &Document<M>) -> String {
    let mut cx = RenderCx::new(0);
    let mut src = String::new();
    for (path, _) in collect_typst_sources(doc) {
        src.push_str(&format!("#import \"{path}\": *\n"));
    }
    src.push_str(&format!(
        "#set page(width: {DOC_PAGE_W}pt, height: {DOC_PAGE_H}pt, margin: 16pt)\n#set text(size: 12pt)\n"
    ));
    for c in &doc.flow {
        src.push_str(&c.render(&mut cx));
    }
    src
}

/// Compile a document through the sources-aware path with all its Typst sources
/// (prelude + authored components) registered. Shared by `render_document` and tests.
pub fn compile_document<M>(doc: &Document<M>) -> Result<typst::layout::PagedDocument> {
    let src = document_source(doc);
    let sources = collect_typst_sources(doc);
    crate::render::compile_to_document_with_sources(&src, &sources)
}
```

Update `render_document` to use it (replacing the `compile_to_document(&src)` call) while keeping the `src` for the reconcile hash:

```rust
pub fn render_document<M>(doc: &Document<M>, version: u64, key: &Key) -> Result<RenderedDoc> {
    let src = document_source(doc);
    let compiled = compile_document(doc)?;
    let page_h = compiled
        .pages
        .first()
        .map(|p| p.frame.height().to_pt())
        .unwrap_or(0.0);
    let manifest = recover_regions(&compiled)?.with_version(version);
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

Update the `import` of `compile_to_document` in `runtime.rs` if it becomes unused (remove `compile_to_document` from the `use crate::render::...` line, keep `document_to_pdf`). Export the new symbols in `crates/inkapp-core/src/lib.rs` and re-export in `crates/inkapp/src/lib.rs` alongside `document_source` (add `collect_typst_sources, compile_document, REGION_PRELUDE`).

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p inkapp-core --test region_prelude`
Expected: PASS.
Run: `cargo test -p inkapp-core -p inkapp-harness -p reading-queue -p agenda`
Expected: PASS — `document_source` callers tolerate the prepended `#import` lines (they assert on substrings still present); no component overrides `typst_sources` yet, so behavior is otherwise identical.

- [ ] **Step 7: Commit**

```bash
git add crates/inkapp-core/typst/region.typ crates/inkapp-core/src/component.rs crates/inkapp-core/src/runtime.rs crates/inkapp-core/src/lib.rs crates/inkapp/src/lib.rs crates/inkapp-core/tests/region_prelude.rs
git commit -m "inkapp-core: #region Typst prelude + Component::typst_sources hook + driver import assembly (Typst authoring seam, step 2)"
```

---

### Task 3: Convert `Checkbox` render half to authored Typst

**Goal:** Move `Checkbox`'s presentation into a baked `checkbox.typ` function, make its `Component::render` emit a `#region(name)[#checkbox(...)]`-shaped call (region wrapping only the 14×14 affordance, preserving hit-test semantics), declare the source via `typst_sources`, and prove render→recover→decode round-trips through the driver.

**Files:**
- Create: `crates/inkapp-core/typst/checkbox.typ`
- Modify: `crates/inkapp-core/src/components/checkbox.rs` (`Component::render` + `typst_sources`)
- Modify: `crates/inkapp-core/tests/checkbox_component.rs` (route the render-recovery test through the driver)

**Acceptance Criteria:**
- [ ] `checkbox.typ` defines `#let checkbox(name, label)` that wraps a 14×14 stroked box in `#region(name, …)` and places the label beside it.
- [ ] `Checkbox::render` (the `Component` impl) emits `#checkbox(name: "<name>", label: "<esc label>")` (validated name; `esc_typst_str` label) and nothing else inline.
- [ ] `Checkbox::typst_sources` returns the baked `checkbox.typ` under `/components/checkbox.typ`.
- [ ] The authored checkbox's region recovers with a 14×14 rect (within 0.01pt), and `decode` on ink inside it returns `[on_check]`; empty ink returns `[]`.

**Verify:** `cargo test -p inkapp-core --test checkbox_component` → all passing.

**Steps:**

- [ ] **Step 1: Update the round-trip test to go through the driver (failing)**

In `crates/inkapp-core/tests/checkbox_component.rs`, replace `component_render_region_recovers` (lines ~50-65) with a version that builds a `Document` and compiles via `compile_document`, and add a full decode round-trip. Replace that test fn with:

```rust
#[test]
fn authored_checkbox_round_trips_through_driver() {
    use inkapp_core::document::Document;
    use inkapp_core::flow;
    use inkapp_core::geometry::PdfPoint;
    use inkapp_core::ink::{RegionInk, Stroke};
    use inkapp_core::manifest::recover_regions;
    use inkapp_core::runtime::compile_document;

    let doc: Document<Msg> =
        Document::keyed("k", flow![Checkbox::with_msg("done", Msg::Archived(7)).label("Archive")]);
    let compiled = compile_document(&doc).unwrap();
    let m = recover_regions(&compiled).unwrap();
    let region = m.regions.iter().find(|r| r.name == "done").expect("authored region recovers");

    // The region wraps the 14x14 affordance only.
    assert!((region.rect.x1 - region.rect.x0 - 14.0).abs() < 0.01, "width ~14pt");
    assert!((region.rect.y1 - region.rect.y0 - 14.0).abs() < 0.01, "height ~14pt");

    // Ink at the region centre decodes to the carried message.
    let cx_mid = (region.rect.x0 + region.rect.x1) / 2.0;
    let cy_mid = (region.rect.y0 + region.rect.y1) / 2.0;
    let ink = vec![RegionInk {
        region: "done".into(),
        strokes: vec![Stroke { points: vec![PdfPoint { x: cx_mid, y: cy_mid }], highlighter: false }],
    }];
    let cb = Checkbox::with_msg("done", Msg::Archived(7)).label("Archive");
    assert_eq!(cb.decode(&ink, &m), vec![Msg::Archived(7)]);
    assert!(cb.decode(&[], &m).is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inkapp-core --test checkbox_component`
Expected: FAIL — `compile_document` import resolves, but the authored region rect is wrong (current render still emits the legacy inline box, not a `#region`/`#checkbox` call), so the 14pt width assertion or the region name lookup fails.

- [ ] **Step 3: Write `checkbox.typ`**

Create `crates/inkapp-core/typst/checkbox.typ`:

```typst
// Checkbox render half. The framework's #region (from the prelude) wraps the
// tappable affordance only — a fixed 14x14 box — so ink hit-testing matches the
// box, not the label. The label is placed beside it, outside the region.
#let checkbox(name, label) = [
  #region(name, box(width: 14pt, height: 14pt, stroke: 0.5pt))#h(4pt)#text[#label]
]
```

- [ ] **Step 4: Rewrite `Checkbox`'s `Component::render` and add `typst_sources`**

In `crates/inkapp-core/src/components/checkbox.rs`, change the `Component` impl. Replace the inline `render` body with a call emission, and add `typst_sources`. Add `use crate::widget::is_valid_region_name;` to the imports:

```rust
impl<M: Clone> Component for Checkbox<M> {
    type Msg = M;

    /// Render half authored in Typst (`checkbox.typ`). This emits a call to the
    /// `checkbox` function; the driver registers `checkbox.typ` (+ the `#region`
    /// prelude) and prepends the imports. Region recovery and `decode` are unchanged.
    fn render(&self, _cx: &mut RenderCx) -> String {
        assert!(
            is_valid_region_name(&self.name),
            "checkbox region name must be a valid region name, got: {:?}",
            self.name
        );
        let name = &self.name;
        let label = esc_typst_str(&self.label);
        format!("#checkbox(name: \"{name}\", label: \"{label}\")\n")
    }

    fn typst_sources(&self) -> Vec<(String, String)> {
        vec![(
            "/components/checkbox.typ".to_string(),
            include_str!("../../typst/checkbox.typ").to_string(),
        )]
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<M> {
        // Any mark emits the message: ScribbledOut reads as non-Empty too, so a
        // scribble also emits on_check. Treating a scribble as an explicit
        // "un-check" is deferred; the keystone's archive is idempotent, so this
        // is harmless for now.
        if self.read_state(ink, manifest) != CheckState::Empty {
            vec![self.on_check.clone()]
        } else {
            vec![]
        }
    }
}
```

(Leave the `Widget` impl's `render` / `render_at` untouched — the absolute-placement path is out of scope; this task converts only the `Component` render.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p inkapp-core --test checkbox_component`
Expected: PASS (including the rewritten round-trip and the existing decode tests).

- [ ] **Step 6: Commit**

```bash
git add crates/inkapp-core/typst/checkbox.typ crates/inkapp-core/src/components/checkbox.rs crates/inkapp-core/tests/checkbox_component.rs
git commit -m "inkapp-core: author Checkbox render half in checkbox.typ via #region (T proven end-to-end)"
```

---

### Task 4: App regression — `reading-queue` still renders and decodes its checkbox

**Goal:** Confirm the authored-Checkbox seam works in a live app (the worked example), and fix any test that asserts on the now-changed render source string. This is the "real caller" guard.

**Files:**
- Modify (if assertions break): `apps/reading-queue/tests/app.rs`, `apps/reading-queue/src/lib.rs` / `serve.rs` (only if they assert on checkbox render markup)
- Test: existing `reading-queue` test suite

**Acceptance Criteria:**
- [ ] `cargo test -p reading-queue` passes.
- [ ] Any assertion that matched the old inline checkbox markup (e.g. a literal `#rect` / `#metadata` substring from the Component render) is updated to the authored form or to a behavioral assertion (region recovers / decode emits `Archived`), not deleted.
- [ ] The whole workspace builds and tests green.

**Verify:** `cargo test --workspace` → all passing; `cargo fmt --check` clean.

**Steps:**

- [ ] **Step 1: Run the app suite to find breakage**

Run: `cargo test -p reading-queue`
Expected: either PASS (decode is behavioral, no source-string assertions) or FAIL on a test asserting old checkbox markup.

- [ ] **Step 2: Inspect any failure and fix the assertion**

For each failing assertion, open the test and determine whether it checks *behavior* (region recovery, decode → `Archived`) or *markup* (a literal substring of the old render). Behavioral assertions should already pass; for a markup assertion, replace the brittle substring check with a behavioral one. Example transform (only if such a test exists):

```rust
// BEFORE: asserted the inline render markup
// assert!(document_source(doc).contains("#rect(width: 14pt"));
// AFTER: assert the authored call is present (the seam), or assert behavior
assert!(document_source(doc).contains("#checkbox(name:"));
```

If a test reaches into render output for the checkbox region, prefer driving it through `render_document` / `recover_regions` and asserting the `done`/archive region recovers and `decode` yields `Msg::Archived`.

- [ ] **Step 3: Run the full workspace suite**

Run: `cargo test --workspace`
Expected: PASS across all crates and apps.

- [ ] **Step 4: Format check**

Run: `cargo fmt --all` then `cargo fmt --all --check`
Expected: clean (no diff).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "reading-queue: adapt to authored Checkbox render (behavioral assertions); workspace green"
```

---

## Self-Review

**Spec coverage:**
- Multi-file World → Task 1. ✓
- `#region` prelude (the one region primitive) → Task 2. ✓
- Component source-declaration + driver import/source assembly → Task 2. ✓
- Convert Checkbox end-to-end → Task 3. ✓
- App regression / live caller guard (spec test #4) → Task 4. ✓
- `recover_regions` unchanged → confirmed (no task touches it). ✓
- Out-of-scope items (per-device layout, Typst composition, other components, rich props) → not planned, matching the spec. ✓

**Placeholder scan:** No TBD/TODO; every code step shows complete content. Task 4's edits are conditional ("if such a test exists") because the breakage is discovered at runtime — the step gives the exact transform to apply, so it is not a placeholder.

**Type consistency:** `compile_to_document_with_sources(src, &[(String,String)])`, `InkWorld::with_sources(src, &[(String,String)])`, `Component::typst_sources(&self) -> Vec<(String,String)>`, `collect_typst_sources(doc) -> Vec<(String,String)>`, `compile_document(doc) -> Result<PagedDocument>`, `REGION_PRELUDE: (&str,&str)` — names and signatures are consistent across Tasks 1→3. `#region(name, body)` and `#checkbox(name, label)` call shapes match between `checkbox.typ` (Task 3) and the prelude (Task 2).
