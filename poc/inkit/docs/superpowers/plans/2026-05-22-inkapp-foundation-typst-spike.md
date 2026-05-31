# inkapp Foundation + Typst Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the greenfield `inkapp` Rust monorepo (absorbing the `rmfiles` `.rm` parser), prove Typst can power the framework's readback model via a spike, and write the foundational docs.

**Architecture:** A Cargo workspace under one nix flake and one `make test`. `crates/rmfiles` is the reused `.rm` v6 parser. `spikes/typst-readback` is a spike crate that compiles Typst **as a library** and tests, as automated checks, whether Typst can yield region rects in PDF coordinates, tappable links, acceptable HTML→Typst conversion, and a stable leading-page structure for `rmapi --content-only`. On-device quality and the device half of the content-only check are documented manual steps modelled on rmreader's known-good rmapi backend. `docs/` holds the project narrative.

**Tech Stack:** Rust (edition 2021), Nix flake (nixpkgs unstable + flake-utils), Typst as a library (`typst`, `typst-pdf`, `typst-kit`), `lopdf` for PDF annotation/structure inspection, `poppler-utils` (`pdftoppm`) + `image` for render verification, patched `rmapi` (PR #65 overlay) for on-device steps.

**Spike caveat:** The Typst library API shifts between 0.x releases. Where this plan shows Typst-integration code, treat it as the intended shape; if the pinned version's signatures differ, adapt them and verify against `cargo doc -p typst`. The *acceptance criteria and verification commands* are the contract, not the exact Typst calls.

---

### Task 1: Monorepo skeleton + absorb rmfiles

**Goal:** A Cargo workspace under one nix flake and `make test`, with `rmfiles` moved in from `~/git/rmfiles` and its tests green.

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `flake.nix`, `nix/overlays/rmapi.nix`, `.envrc`, `Makefile`, `.gitignore`
- Create: `.githooks/pre-commit`
- Create: `crates/rmfiles/**` (copied from `~/git/rmfiles`: `Cargo.toml`, `src/`, `tests/`)

**Acceptance Criteria:**
- [ ] `nix develop -c cargo build` succeeds for the workspace.
- [ ] `make test` runs and all `rmfiles` tests pass.
- [ ] `make clippy` passes with no warnings.
- [ ] The workspace `Cargo.lock` is committed.

**Verify:** `make test` → rmfiles test suite passes (bundle/highlights/strokes tests green).

**Steps:**

- [ ] **Step 1: Copy rmfiles into the workspace**

```bash
mkdir -p /home/dan/git/inkapp/crates
cp -R /home/dan/git/rmfiles/src   /home/dan/git/inkapp/crates/rmfiles/src
cp -R /home/dan/git/rmfiles/tests /home/dan/git/inkapp/crates/rmfiles/tests
cp     /home/dan/git/rmfiles/Cargo.toml /home/dan/git/inkapp/crates/rmfiles/Cargo.toml
cp     /home/dan/git/rmfiles/LICENSE     /home/dan/git/inkapp/crates/rmfiles/LICENSE
```

Leave `crates/rmfiles/Cargo.toml` unchanged (it already declares `name = "rmfiles"`, deps `thiserror`, `zip`, `serde`, `serde_json`, dev-dep `tempfile`). Do **not** rename the crate.

- [ ] **Step 2: Write the workspace root `Cargo.toml`**

```toml
[workspace]
resolver = "2"
members = ["crates/rmfiles"]

[workspace.package]
edition = "2021"
license = "MIT"

[profile.release]
lto = true
```

(The `spikes/typst-readback` member is added in Task 3, when that crate exists, so the workspace stays buildable after every task.)

- [ ] **Step 3: Write the rmapi overlay**

Copy the proven overlay verbatim from the reference repo:

```bash
mkdir -p /home/dan/git/inkapp/nix/overlays
cp /home/dan/git/rmbujo/nix/overlays/rmapi.nix /home/dan/git/inkapp/nix/overlays/rmapi.nix
```

- [ ] **Step 4: Write `flake.nix`**

This drops rmbujo's fulgur/Blitz-specific `stylo`/`python3` build inputs (inkapp does not use fulgur) and keeps fonts + poppler (for spike verification) + patched rmapi.

```nix
{
  description = "inkapp — a framework for building apps for pen-based document devices";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.flake-utils.url = "github:numtide/flake-utils";
  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import ./nix/overlays/rmapi.nix) ];
        pkgs = import nixpkgs { inherit system overlays; };
      in {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [
            pkgs.rustc pkgs.cargo pkgs.clippy pkgs.rustfmt pkgs.pkg-config
          ];
          # fontconfig + fonts: Typst (via typst-kit) loads system fonts at render time.
          # poppler-utils: pdftoppm renders spike PDFs to PNG for layout verification.
          # rmapi: reMarkable cloud client (v4-patched), for the spike's on-device steps.
          buildInputs = [
            pkgs.libiconv pkgs.fontconfig pkgs.dejavu_fonts pkgs.noto-fonts
            pkgs.poppler-utils pkgs.rmapi
          ];
        };
      });
}
```

- [ ] **Step 5: Write `.envrc`, `Makefile`, `.gitignore`, pre-commit hook**

`.envrc`:
```
use flake
```

`Makefile`:
```make
.PHONY: test build fmt fmt-check clippy hooks
test:
	nix develop -c cargo test
build:
	nix develop -c cargo build
fmt:
	nix develop -c cargo fmt
fmt-check:
	nix develop -c cargo fmt --check
clippy:
	nix develop -c cargo clippy --all-targets -- -D warnings
hooks:
	git config core.hooksPath .githooks
	@echo "pre-commit hook enabled: cargo fmt --check"
```

`.gitignore`:
```
/target
result
```

`.githooks/pre-commit`:
```sh
#!/usr/bin/env sh
exec nix develop -c cargo fmt --check
```
Then `chmod +x .githooks/pre-commit`.

- [ ] **Step 6: Build, test, lint**

Run: `cd /home/dan/git/inkapp && make build && make test && make clippy`
Expected: build succeeds; rmfiles tests pass; clippy clean.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock flake.nix nix .envrc Makefile .gitignore .githooks crates
git commit -m "Stand up monorepo workspace and absorb rmfiles"
```

---

### Task 2: docs/ narrative set

**Goal:** The foundational project docs exist (why / how-it-works / inspiration / glossary), and `remarkable-pdf-mechanics.md` is moved in. Independent of the spike, so it can run in parallel.

**Files:**
- Create: `docs/why.md`, `docs/how-it-works.md`, `docs/inspiration.md`, `docs/glossary.md`
- Create: `docs/remarkable-pdf-mechanics.md` (moved from `~/git/rmbujo/docs/`)
- Modify: `docs/README.md` (index of the docs set)

**Acceptance Criteria:**
- [ ] All five narrative files exist and are non-empty.
- [ ] `remarkable-pdf-mechanics.md` content matches the source byte-for-byte.
- [ ] Every relative link between docs resolves to an existing file.
- [ ] `docs/glossary.md` defines: app, handler, document/bundle, manifest, region, readback, sync, device.

**Verify:** `bash -c 'for f in why how-it-works inspiration glossary remarkable-pdf-mechanics; do test -s docs/$f.md || { echo MISSING $f; exit 1; }; done && echo OK'` → `OK`

**Steps:**

- [ ] **Step 1: Move the mechanics doc in**

```bash
cp /home/dan/git/rmbujo/docs/remarkable-pdf-mechanics.md /home/dan/git/inkapp/docs/remarkable-pdf-mechanics.md
```

- [ ] **Step 2: Write `docs/why.md`**

Cover, in prose (no placeholders): the problem (pen-based document devices are read-only islands; annotations are trapped on-device); why they deserve a real app framework (people already live in these devices for reading/journaling/note-taking, but every "app" today is a static PDF); and the limits of static-only documents (no reaction to what the user wrote, no state, no actions). State the thesis: documents that are *processed server-side and regenerated* turn a passive device into an interactive app surface.

- [ ] **Step 3: Write `docs/how-it-works.md`**

Describe the loop concretely: **render** (Typst) → **embed state** (a manifest of page regions + version, embedded in the PDF — never secrets) → **sync** to device (rmapi, abstracted) → user reads/writes with the pen → **sync back** → **readback** (parse `.rm` ink via rmfiles, map strokes to manifest regions) → **process** (Rust, sometimes LLMs) → **render next**. Include a short ASCII diagram of the loop. State the two invariants this rests on: state-in-document (so the server is stateless about per-doc position) and secrets-never-in-documents (users may share docs with third parties). Reference `remarkable-pdf-mechanics.md` for the device-level sync rules.

- [ ] **Step 4: Write `docs/inspiration.md`**

Web 1.0 / CGI as the mental model (HTML+form-post ⇒ PDF+`.rm`; server renders the next page; no client-side scripting). TUI frameworks and web frameworks as prior art for "make a hard surface easy to build for." Typst as the rendering foundation and why (programmatic introspection of layout, beautiful typesetting, pure-Rust, no browser).

- [ ] **Step 5: Write `docs/glossary.md`**

Define each term in one or two sentences: **app** (a handler + its document templates), **handler** (the server-side code that consumes a synced document and emits the next), **document/bundle** (the PDF + per-page `.rm` files + content index as one unit), **manifest** (the embedded region/version state), **region** (a labelled rectangle on a page that has meaning for readback), **readback** (parsing on-device ink and mapping it to regions), **sync** (push/pull via a device backend), **device** (the abstracted target: reMarkable now, Supernote/Boox later).

- [ ] **Step 6: Write `docs/README.md`**

A short index linking to all five docs with one-line descriptions.

- [ ] **Step 7: Verify links and presence, then commit**

Run the Verify command above; manually confirm each inter-doc link target exists.
```bash
git add docs
git commit -m "Add foundational docs and move in remarkable-pdf-mechanics"
```

---

### Task 3: Typst spike scaffold — compile to PDF as a library

**Goal:** A `spikes/typst-readback` crate that compiles an in-memory Typst source to a valid PDF via the Typst library, proving the World/compile/export path before the bars build on it.

**Files:**
- Modify: `Cargo.toml` (add `spikes/typst-readback` to workspace members)
- Create: `spikes/typst-readback/Cargo.toml`
- Create: `spikes/typst-readback/src/lib.rs` (the spike's reusable helpers)
- Create: `spikes/typst-readback/src/world.rs` (the Typst `World` impl)
- Create: `spikes/typst-readback/tests/scaffold.rs`

**Acceptance Criteria:**
- [ ] `compile_pdf("= Hello")` returns `Vec<u8>` beginning with `%PDF` and is non-empty.
- [ ] The produced PDF has exactly one page (verified via `lopdf`).
- [ ] Test runs under `make test` with no device or network.

**Verify:** `nix develop -c cargo test -p typst-readback --test scaffold` → PASS

**Steps:**

- [ ] **Step 1: Add the crate to the workspace**

In root `Cargo.toml`, change members to:
```toml
members = ["crates/rmfiles", "spikes/typst-readback"]
```

- [ ] **Step 2: Write `spikes/typst-readback/Cargo.toml`**

```toml
[package]
name = "typst-readback"
version = "0.1.0"
edition = "2021"
license = "MIT"
publish = false

[dependencies]
typst = "0.13"
typst-pdf = "0.13"
typst-kit = { version = "0.13", features = ["fonts"] }
comemo = "0.4"
anyhow = "1"

[dev-dependencies]
lopdf = "0.36"
image = "0.25"
tempfile = "3"
```
(Pin to whatever single coherent Typst release `nix develop -c cargo update` resolves; keep `typst`, `typst-pdf`, `typst-kit` on the **same** version.)

- [ ] **Step 3: Write `src/world.rs` — a minimal `World`**

A spike-grade `World` with one in-memory main source and system fonts via `typst-kit`. Shape (adapt signatures to the pinned version):

```rust
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, World};
use typst_kit::fonts::{FontSlot, Fonts};

pub struct SpikeWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<FontSlot>,
    main: Source,
}

impl SpikeWorld {
    pub fn new(src: &str) -> Self {
        let fonts = Fonts::searcher().include_system_fonts(true).search();
        let main = Source::new(FileId::new(None, VirtualPath::new("main.typ")), src.into());
        Self {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(fonts.book),
            fonts: fonts.fonts,
            main,
        }
    }
}

impl World for SpikeWorld {
    fn library(&self) -> &LazyHash<Library> { &self.library }
    fn book(&self) -> &LazyHash<FontBook> { &self.book }
    fn main(&self) -> FileId { self.main.id() }
    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() { Ok(self.main.clone()) } else { Err(FileError::NotFound(id.vpath().as_rootless_path().into())) }
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        Err(FileError::NotFound(id.vpath().as_rootless_path().into()))
    }
    fn font(&self, index: usize) -> Option<Font> { self.fonts[index].get() }
    fn today(&self, _offset: Option<i64>) -> Option<Datetime> { None }
}
```

- [ ] **Step 4: Write `src/lib.rs` — `compile_pdf`**

```rust
pub mod world;
use anyhow::{anyhow, Result};
use world::SpikeWorld;

/// Compile Typst source to PDF bytes. Spike-grade: panics-as-errors surfaced via anyhow.
pub fn compile_pdf(src: &str) -> Result<Vec<u8>> {
    let world = SpikeWorld::new(src);
    let result = typst::compile(&world);
    let document = result.output.map_err(|d| anyhow!("typst compile failed: {d:?}"))?;
    let pdf = typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default())
        .map_err(|d| anyhow!("typst pdf export failed: {d:?}"))?;
    Ok(pdf)
}
```

- [ ] **Step 5: Write the failing test `tests/scaffold.rs`**

```rust
use lopdf::Document;

#[test]
fn compiles_hello_to_single_page_pdf() {
    let pdf = typst_readback::compile_pdf("= Hello").expect("compile");
    assert!(pdf.starts_with(b"%PDF"), "missing PDF header");
    let doc = Document::load_mem(&pdf).expect("parse pdf");
    assert_eq!(doc.get_pages().len(), 1, "expected one page");
}
```

- [ ] **Step 6: Run (fails to compile/find API), then fix until green**

Run: `nix develop -c cargo test -p typst-readback --test scaffold`
Expected first: compile errors if the pinned Typst API differs — resolve against `cargo doc -p typst` until PASS.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock spikes/typst-readback
git commit -m "Spike: compile Typst to PDF as a library"
```

---

### Task 4: Bar 1 (make-or-break) — recover region rects in PDF coordinates

**Goal:** Prove a Typst document can compute its own labelled region rectangles and expose them, and that those rects — converted to PDF bottom-left coordinates — match where the regions actually render.

**Files:**
- Create: `spikes/typst-readback/src/regions.rs` (query + coordinate conversion)
- Create: `spikes/typst-readback/tests/regions.rs`
- Create: `spikes/typst-readback/tests/fixtures/regions.typ`
- Modify: `spikes/typst-readback/src/lib.rs` (expose `regions`, add `compile_with_regions`)

**Acceptance Criteria:**
- [ ] A `.typ` placing labelled boxes emits, via `metadata`+`query`, each region's label, page, top-left position, and size.
- [ ] `typst_to_pdf_rect` converts a Typst top-left rect to a PDF bottom-left rect.
- [ ] For a deterministic layout, each recovered PDF rect is within 1.0 pt of the expected rect.
- [ ] Independent check: rendering the page to PNG shows the region's drawn border falling inside the recovered rect (a sampled border pixel is non-white and inside; the rect interior center matches the box).

**Verify:** `nix develop -c cargo test -p typst-readback --test regions` → PASS

**Steps:**

- [ ] **Step 1: Author the region test document (string constant in the test)**

A page with two labelled, fixed-size boxes at known offsets, each emitting its geometry as metadata. The document computes geometry from `locate(...).position()` (top-left origin, pt) plus the known box size:

```typst
#set page(width: 200pt, height: 300pt, margin: 0pt)
#let region(name, body) = context {
  let loc = here()
  let pos = loc.position()
  let size = measure(body)
  metadata((
    name: name,
    page: pos.page,
    x: pos.x.pt(), y: pos.y.pt(),
    w: size.width.pt(), h: size.height.pt(),
  )) <region>
  box(stroke: 1pt, width: size.width, height: size.height, body)
}
#place(top + left, dx: 20pt, dy: 40pt, region("a", box(width: 60pt, height: 24pt)))
#place(top + left, dx: 100pt, dy: 200pt, region("b", box(width: 50pt, height: 30pt)))
```
(If `measure` inside `context` proves awkward in the pinned version, pass explicit `w`/`h` into `region(...)` since the framework controls box size anyway — record which path worked in the findings doc.)

- [ ] **Step 2: Implement `compile_with_regions` in `src/regions.rs`**

`compile_with_regions(src) -> Result<(Vec<u8>, Vec<TypstRegion>, f64)>` compiles once to get the `PagedDocument` (for PDF bytes + page height), runs `query` for `<region>` metadata, and returns `(pdf, regions, page_height_pt)`. Shape:

```rust
use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct TypstRegion { pub name: String, pub page: usize, pub x: f64, pub y: f64, pub w: f64, pub h: f64 }

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PdfRect { pub x0: f64, pub y0: f64, pub x1: f64, pub y1: f64 }

/// Typst gives top-left origin (y down); PDF user space is bottom-left (y up).
pub fn typst_to_pdf_rect(r: &TypstRegion, page_height_pt: f64) -> PdfRect {
    PdfRect {
        x0: r.x,
        y0: page_height_pt - (r.y + r.h),
        x1: r.x + r.w,
        y1: page_height_pt - r.y,
    }
}
```
Recover the metadata by querying the compiled document for the `<region>` label and deserializing each `value` into `TypstRegion` (use the introspection/query API on the compiled document; cross-check with `typst query main.typ '<region>' --field value` via the CLI if the library query path is unclear, and record the chosen path).

- [ ] **Step 3: Write the failing test `tests/regions.rs`**

```rust
use typst_readback::regions::{typst_to_pdf_rect, PdfRect};

const DOC: &str = include_str!("fixtures/regions.typ"); // the document authored in Step 1

#[test]
fn recovers_region_rects_in_pdf_coords() {
    let (pdf, regions, page_h) = typst_readback::regions::compile_with_regions(DOC).unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    let a = regions.iter().find(|r| r.name == "a").unwrap();
    let got = typst_to_pdf_rect(a, page_h);
    // page height 300pt; region "a" at top-left (20,40), size 60x24
    let want = PdfRect { x0: 20.0, y0: 300.0 - 64.0, x1: 80.0, y1: 300.0 - 40.0 };
    let close = |x: f64, y: f64| (x - y).abs() < 1.0;
    assert!(close(got.x0, want.x0) && close(got.y0, want.y0)
         && close(got.x1, want.x1) && close(got.y1, want.y1), "got {got:?} want {want:?}");
}
```

- [ ] **Step 4: Add the independent PNG cross-check**

Render the PDF to PNG with `pdftoppm` at a known DPI and assert the box border lies inside the recovered rect:

```rust
#[test]
fn rendered_border_falls_inside_recovered_rect() {
    let (pdf, regions, page_h) = typst_readback::regions::compile_with_regions(DOC).unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.pdf"), &pdf).unwrap();
    // 72 dpi => 1px == 1pt, simplest mapping
    let st = std::process::Command::new("pdftoppm")
        .args(["-r", "72", "-png", "p.pdf", "p"]).current_dir(dir.path()).status().unwrap();
    assert!(st.success());
    let img = image::open(dir.path().join("p-1.png")).unwrap().to_luma8();
    let a = regions.iter().find(|r| r.name == "a").unwrap();
    let r = typst_to_pdf_rect(a, page_h);
    // PDF y-up -> image y-down: img_y = page_h - pdf_y. Sample the top border midpoint.
    let px = ((r.x0 + r.x1) / 2.0) as u32;
    let py = (page_h - r.y1) as u32; // top edge in image space
    let v = img.get_pixel(px.min(img.width()-1), py.min(img.height()-1)).0[0];
    assert!(v < 200, "expected dark border pixel at box top, got luma {v}");
}
```

- [ ] **Step 5: Run, iterate to green**

Run: `nix develop -c cargo test -p typst-readback --test regions`
Expected: both tests PASS. If the query path or `position()` semantics differ, adjust `compile_with_regions` and the coordinate math until the rects align within tolerance.

- [ ] **Step 6: Commit**

```bash
git add spikes/typst-readback
git commit -m "Spike bar 1: recover region rects in PDF coordinates"
```

---

### Task 5: Bar 2 — tappable internal + external links

**Goal:** Prove links survive Typst→PDF as real annotations the device can tap.

**Files:**
- Create: `spikes/typst-readback/tests/links.rs`
- Create: `spikes/typst-readback/tests/fixtures/links.typ`

**Acceptance Criteria:**
- [ ] A doc with an internal link (`#link(<target>)[...]` to a labelled element on another page) and an external link (`#link("https://example.com")[...]`) compiles.
- [ ] The PDF contains a `/Link` annotation with a `/Dest` or `GoTo` action (internal) and one with a `/URI` action equal to `https://example.com` (external), found via `lopdf`.

**Verify:** `nix develop -c cargo test -p typst-readback --test links` → PASS

**Steps:**

- [ ] **Step 1: Author the link document**

```typst
#set page(width: 200pt, height: 200pt)
See #link("https://example.com")[the site] and #link(<p2>)[page two].
#pagebreak()
= Page two <p2>
```

- [ ] **Step 2: Write the failing test**

```rust
use lopdf::{Document, Object};

fn link_annotations(doc: &Document) -> Vec<lopdf::Dictionary> {
    let mut out = Vec::new();
    for (_id, obj) in doc.objects.iter() {
        if let Ok(d) = obj.as_dict() {
            if d.get(b"Subtype").and_then(|o| o.as_name()).ok() == Some(b"Link") {
                out.push(d.clone());
            }
        }
    }
    out
}

#[test]
fn pdf_has_internal_and_external_links() {
    let pdf = typst_readback::compile_pdf(include_str!("fixtures/links.typ")).unwrap();
    let doc = Document::load_mem(&pdf).unwrap();
    let anns = link_annotations(&doc);
    assert!(anns.len() >= 2, "expected >=2 link annotations, got {}", anns.len());

    let has_uri = anns.iter().any(|d| {
        d.get(b"A").and_then(|a| a.as_dict()).ok()
            .and_then(|a| a.get(b"URI").ok())
            .and_then(|u| u.as_str().ok())
            .map(|s| s == b"https://example.com").unwrap_or(false)
    });
    assert!(has_uri, "missing external URI link");

    let has_internal = anns.iter().any(|d| {
        d.has(b"Dest")
            || d.get(b"A").and_then(|a| a.as_dict()).ok()
                .and_then(|a| a.get(b"S").ok()).and_then(|s| s.as_name().ok())
                == Some(b"GoTo")
    });
    assert!(has_internal, "missing internal destination link");
}
```

- [ ] **Step 3: Run, iterate to green**

Run: `nix develop -c cargo test -p typst-readback --test links`
Expected: PASS. Adjust the annotation-shape assertions to match how the pinned `typst-pdf` emits links (Dest vs GoTo action) — record the actual shape in the findings doc.

- [ ] **Step 4: Commit**

```bash
git add spikes/typst-readback
git commit -m "Spike bar 2: tappable internal and external links survive to PDF"
```

---

### Task 6: Bar 3 — HTML content into Typst

**Goal:** Determine the realistic path for getting representative HTML article content into a Typst document, and document its limits.

**Files:**
- Create: `spikes/typst-readback/src/html.rs` (HTML→Typst conversion)
- Create: `spikes/typst-readback/tests/html.rs`
- Create: `spikes/typst-readback/tests/fixtures/article.html`

**Acceptance Criteria:**
- [ ] A representative HTML fixture (headings, paragraphs, bold/italic, a list, a link) converts to Typst markup.
- [ ] The converted doc compiles to a PDF whose extracted text contains the fixture's heading text, a body sentence, and the list items.
- [ ] Conversion limits (unsupported tags, lossy cases) are written to the findings doc in Task 8.

**Verify:** `nix develop -c cargo test -p typst-readback --test html` → PASS

**Steps:**

- [ ] **Step 1: Write `tests/fixtures/article.html`**

A small but representative article: an `<h1>`, two `<p>` (one containing `<strong>` and `<em>` and an `<a href>`), and a `<ul>` with three `<li>`.

- [ ] **Step 2: Implement `html_to_typst` in `src/html.rs`**

A pragmatic converter over a parsed DOM. Use `scraper` (or `lol_html`, already proven in rmreader) to walk nodes and map a fixed tag set to Typst markup: `h1..h6`→`=`..`======`, `p`→paragraph + blank line, `strong/b`→`*..*`, `em/i`→`_.._`, `ul/li`→`- `, `a`→`#link("href")[text]`, escaping Typst-special chars (`#`, `*`, `_`, `@`, `<`, `$`, `\``, `\`). Unknown tags: recurse into children, emitting text. Add `scraper = "0.20"` to `[dependencies]`.

```rust
/// Convert a small, known subset of HTML to Typst markup. Returns the markup;
/// records nothing about losses here (the caller/findings doc notes limits).
pub fn html_to_typst(html: &str) -> String { /* walk DOM, map tags as above */ unimplemented!() }
```
Implement it fully (no `unimplemented!` in the committed code) following the mapping above.

- [ ] **Step 3: Write the failing test**

```rust
fn pdf_text(pdf: &[u8]) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("d.pdf"), pdf).unwrap();
    let out = std::process::Command::new("pdftotext")
        .args(["d.pdf", "-"]).current_dir(dir.path()).output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn html_article_renders_with_expected_text() {
    let html = include_str!("fixtures/article.html");
    let typ = typst_readback::html::html_to_typst(html);
    let pdf = typst_readback::compile_pdf(&typ).unwrap();
    let text = pdf_text(&pdf);
    for needle in ["Article Title", "first list item"] {
        assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
    }
}
```
(`pdftotext` ships with `poppler-utils`, already in the flake. Match the needles to the fixture's actual text.)

- [ ] **Step 4: Run, iterate to green; note limits**

Run: `nix develop -c cargo test -p typst-readback --test html`
Expected: PASS. Keep a scratch list of tags/cases that convert poorly — it feeds Task 8's findings.

- [ ] **Step 5: Commit**

```bash
git add spikes/typst-readback
git commit -m "Spike bar 3: convert representative HTML into a Typst document"
```

---

### Task 7: Bar 5 (structure half) — content-only leading-page invariant

**Goal:** Prove that re-rendering a Typst doc with appended trailing pages leaves the leading pages' identity/order intact, so an `rmapi --content-only` push won't scramble on-device ink. (The on-device half is the manual step in Task 8.)

**Files:**
- Create: `spikes/typst-readback/tests/content_only.rs`

**Acceptance Criteria:**
- [ ] Render "v1" (N leading pages) and "v2" (same N leading pages + appended trailing pages).
- [ ] v2 has more pages than v1; the first N pages have identical `MediaBox` and identical page order/count for the leading section.
- [ ] A leading page's recovered region rect (from Bar 1's machinery) is unchanged between v1 and v2 within 1.0 pt.

**Verify:** `nix develop -c cargo test -p typst-readback --test content_only` → PASS

**Steps:**

- [ ] **Step 1: Author v1 and v2 documents**

v1: a fixed leading section (e.g. 2 pages) ending with a labelled region. v2: byte-identical leading section, then `#pagebreak()` and extra trailing content. In `lib.rs` add `pub mod test_docs { pub const LEADING: &str = "..."; pub const V1: &str = LEADING; pub const V2: &str = concat!(LEADING, "\n#pagebreak()\nTrailing page"); }` so the leading bytes are provably identical and the consts are reusable from tests.

- [ ] **Step 2: Write the failing test**

```rust
use lopdf::Document;

fn mediaboxes(pdf: &[u8]) -> Vec<Vec<f64>> {
    let doc = Document::load_mem(pdf).unwrap();
    doc.get_pages().values().map(|&id| {
        let page = doc.get_dictionary(id).unwrap();
        page.get(b"MediaBox").and_then(|m| m.as_array()).unwrap()
            .iter().map(|o| o.as_float().unwrap_or(o.as_i64().unwrap() as f32) as f64).collect()
    }).collect()
}

#[test]
fn appending_trailing_pages_preserves_leading_pages() {
    let v1 = typst_readback::compile_pdf(typst_readback::test_docs::V1).unwrap();
    let v2 = typst_readback::compile_pdf(typst_readback::test_docs::V2).unwrap();
    let (mb1, mb2) = (mediaboxes(&v1), mediaboxes(&v2));
    assert!(mb2.len() > mb1.len(), "v2 should have more pages");
    assert_eq!(&mb2[..mb1.len()], &mb1[..], "leading MediaBoxes changed");
}
```
(Expose `V1`/`V2` as `pub const` in a `test_docs` module in `lib.rs`, or `include_str!` two fixtures — pick one and use it consistently.)

- [ ] **Step 3: Add the leading-region-stability assertion**

Using `compile_with_regions` from Task 4 on both v1 and v2, assert the leading region's `typst_to_pdf_rect` matches within 1.0 pt.

- [ ] **Step 4: Run to green, commit**

Run: `nix develop -c cargo test -p typst-readback --test content_only`
```bash
git add spikes/typst-readback
git commit -m "Spike bar 5: leading-page invariant holds when appending trailing pages"
```

---

### Task 8: On-device harness + go/no-go findings doc

**Goal:** Provide an opt-in on-device verification (modelled on rmreader's rmapi backend) for the manual bars (4, and the device half of 5), and write the spike's go/no-go findings.

**Files:**
- Create: `spikes/typst-readback/src/rmapi.rs` (thin push helper, mirrors rmreader)
- Create: `spikes/typst-readback/tests/on_device.rs` (`#[ignore]` by default)
- Create: `docs/superpowers/spikes/2026-05-22-typst-readback-findings.md`

**Acceptance Criteria:**
- [ ] `push_content_only(pdf_path, folder)` shells out to `rmapi -ni put --content-only <pdf> <folder>` and reports success/failure (no token-clobber handling needed for a spike, but call `rmapi` non-interactively with null stdin).
- [ ] `tests/on_device.rs` is `#[ignore]`d so `make test` stays device-free; running it explicitly pushes a sample doc and prints the remote path to inspect on the tablet.
- [ ] The findings doc states a clear **go / no-go on Typst**, records the chosen approach for Bar 1 (library query vs CLI query; `measure` vs explicit size), the link-annotation shape from Bar 2, the HTML→Typst limits from Bar 3, the Bar 5 result, and — if no-go — what adopting Typst would require.

**Verify:** `nix develop -c cargo test -p typst-readback` → all non-ignored tests PASS; `bash -c 'test -s docs/superpowers/spikes/2026-05-22-typst-readback-findings.md && grep -qiE "go/no-go|verdict|recommendation" docs/superpowers/spikes/2026-05-22-typst-readback-findings.md && echo OK'` → `OK`

**Steps:**

- [ ] **Step 1: Implement `src/rmapi.rs`**

```rust
use anyhow::{bail, Result};
use std::path::Path;
use std::process::{Command, Stdio};

/// Push a PDF preserving on-device ink (content-only). Spike-grade: assumes a paired,
/// v4-patched rmapi is on PATH (provided by the flake). Mirrors rmreader's arg order.
pub fn push_content_only(pdf: &Path, folder: &str) -> Result<()> {
    let _ = Command::new("rmapi").args(["-ni", "mkdir", folder]).stdin(Stdio::null()).status();
    let ok = Command::new("rmapi")
        .args(["-ni", "put", "--content-only", pdf.to_str().unwrap(), folder])
        .stdin(Stdio::null()).status()?.success();
    if !ok { bail!("rmapi put --content-only failed for {}", pdf.display()); }
    Ok(())
}
```

- [ ] **Step 2: Write the ignored on-device test**

```rust
#[test]
#[ignore = "requires a paired reMarkable; run manually: cargo test -p typst-readback --test on_device -- --ignored --nocapture"]
fn pushes_sample_doc_for_visual_check() {
    let pdf = typst_readback::compile_pdf("= inkapp spike\n\nWrite on me with the pen.").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inkapp-spike.pdf");
    std::fs::write(&path, &pdf).unwrap();
    typst_readback::rmapi::push_content_only(&path, "/inkapp-spike").unwrap();
    eprintln!("pushed to /inkapp-spike — inspect quality on the tablet");
}
```

- [ ] **Step 3: Run the automated suite (ignored test skipped)**

Run: `nix develop -c cargo test -p typst-readback`
Expected: all bars 1/2/3/5 PASS; `on_device` shows as ignored.

- [ ] **Step 4: Optionally run the manual device check**

Run (only with a paired tablet): `nix develop -c cargo test -p typst-readback --test on_device -- --ignored --nocapture`, then inspect the pushed doc on the device for Bar 4 (quality) and, after writing on it and syncing, Bar 5's device half.

- [ ] **Step 4b: (stretch, optional) Close the full loop**

Only if cheap and a tablet is paired: after the manual push, write a highlighter stroke over a known region on-device, sync, fetch the bundle (`rmapi -ni get`), open it with `rmfiles::Bundle`, take the highlighter stroke's bbox, and confirm its center falls inside that region's recovered PDF rect (from Bar 1). This proves the full render→ink→readback→region mapping end to end. Capture the result in the findings doc; skip without penalty if not paired.

- [ ] **Step 5: Write the findings doc**

`docs/superpowers/spikes/2026-05-22-typst-readback-findings.md` with: a top-line **verdict (go/no-go on Typst)**; per-bar results (1 region rects + chosen approach, 2 link shape, 3 HTML limits, 4 on-device quality status, 5 invariant); the coordinate-conversion notes; and, if no-go, the work Typst adoption would need. If Bar 4 hasn't been run on hardware yet, mark it explicitly as "pending manual on-device check" rather than claiming it.

- [ ] **Step 6: Commit**

```bash
git add spikes/typst-readback docs/superpowers/spikes
git commit -m "Spike: on-device harness and Typst go/no-go findings"
```

---

## Notes for the implementer

- **Keep all three Typst crates on one version.** Mismatched `typst`/`typst-pdf`/`typst-kit` versions will not compile.
- **Coordinate origin is the #1 bug source.** Typst `position()` is top-left, y-down, in pt; PDF user space is bottom-left, y-up; raster images are top-left, y-down. The conversions live in `typst_to_pdf_rect` and the PNG cross-check — change them in one place.
- **The bars are the contract; the Typst calls are not.** If a pinned API differs, adapt the call and keep the acceptance criteria. Record what actually worked in the findings doc.
- **Reference, don't import, the old apps.** rmreader/rmbujo are at `~/git/`; read them for known-good patterns (especially `rmreader/src/deploy/rmapi.rs`) but do not pull them into the workspace.
