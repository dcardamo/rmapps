# Reader App Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the new `apps/reader` (Library.pdf + Feed.pdf, four-cell per-page ActionBand) by pushing five reusable primitives into inkapp first, then composing them in ~30 lines of app code.

**Architecture:** Five framework additions land in order (`Heading`, `Section<M>`, `Document::page_header`, `ActionBand<M>`, `push_replace_ink`), each TDD'd in isolation with no dependency on the reader. The app crate is the final composition step. Per the operator directive 2026-05-26, every gap is closed in the framework — no app-side workarounds.

**Tech Stack:** Rust workspace, Typst 0.14.2 (authored modules for `#region`/`#section`/`#action-band`), tokio, clap 4 derive, `inkapp-core`/`inkapp-content`/`inkapp-readwise-reader`/`rm-cloud`/`rm-device`.

**Spec:** `docs/superpowers/specs/2026-05-26-reader-app-design.md` (commit `b4cc73c`).

**Worktree:** Implement on a feature branch `reader-app` (use `git worktree add ~/.paseo/worktrees/reader-app reader-app` or whichever worktree tooling is in place). Merge back to `main` at the end via your standard worktree-finish flow.

**Build/test command:** `nix develop -c cargo test --workspace`. Plain `cargo` outside `nix develop` fails (the image pipeline pulls `dav1d`). Do **not** stage `Cargo.lock` in implementation commits; lockfile resync is a follow-up commit per the [[worktree-merge-cargo-lock-resync]] convention. Clear completed entries from this file's `.tasks.json` sibling before each commit (the pre-commit hook blocks on open tasks).

---

## File map

| File | Action | Responsibility |
|------|--------|----------------|
| `crates/inkapp-core/src/components/heading.rs` | create | `Heading` Display component: title + byline + reading-time + optional subtitle |
| `crates/inkapp-core/typst/heading.typ` | create | Authored render half of `Heading` |
| `crates/inkapp-core/src/components/section.rs` | create | `Section<M>` — opens per-section state + weak pagebreak, wraps a body |
| `crates/inkapp-core/typst/section.typ` | create | `#section(id, body)` authored module: state update + weak pagebreak |
| `crates/inkapp-core/src/components/action_band.rs` | create | `ActionBand<M>` — N labelled per-page-header cells with closure-to-`M` per cell |
| `crates/inkapp-core/typst/action_band.typ` | create | `#action-band(cells)` authored module reading section state |
| `crates/inkapp-core/src/components/mod.rs` | modify | `pub mod heading; pub mod section; pub mod action_band;` |
| `crates/inkapp-core/src/document.rs` | modify | `Document<M>` carries optional `page_header`; `.page_header(c)` builder |
| `crates/inkapp-core/src/runtime.rs` | modify | `document_source_in` emits `#set page(header: ...)`; `step` decodes header per cycle |
| `crates/inkapp-core/src/sync.rs` | modify | `DeviceTransport::push_replace_ink`; `sync_once` uses replace post-fold |
| `crates/rm-device/src/transport.rs` | modify | `CloudTransport::push_replace_ink` strips `.rm` blobs then `put`s |
| `apps/reader/Cargo.toml` | create | New app crate |
| `apps/reader/src/lib.rs` | create | `App`, `Msg`, `Connectors`, `AppConfig`, `update`, `view` |
| `apps/reader/src/main.rs` | create | CLI mirroring reading-queue (config/op/doctor/preview/sync/run/default-publish) |
| `apps/reader/tests/app.rs` | create | View composition over `Readwise::fake()` cassette |
| `apps/reader/tests/config.rs` | create | AppConfig resolves with defaults; from_config wires correctly |
| `Cargo.toml` (workspace) | modify | Add `apps/reader` to members |
| `docs/appdx.md` | modify | Record Heading + Section + ActionBand + page_header + push_replace_ink + reader app |

---

## Task 0: `Heading` Display component

**Goal:** A reusable theme-aware `Heading` component (title + byline + reading-time + optional subtitle) any long-form reading app can drop in.

**Files:**
- Create: `crates/inkapp-core/typst/heading.typ`
- Create: `crates/inkapp-core/src/components/heading.rs`
- Modify: `crates/inkapp-core/src/components/mod.rs`
- Create: `crates/inkapp-core/tests/heading_component.rs`

**Acceptance Criteria:**
- [ ] `Heading::new(title)` renders a heading with just a title.
- [ ] `Heading::new(title).byline("Jane Doe").reading_time("5 min").subtitle("...")` renders all four lines.
- [ ] Byline fallback: when an `Article`'s `author` is empty, callers can pass `site_name` instead (the convention test exercises this).
- [ ] Theme-aware: uses `cx.theme()` for tones (heading color, muted color).
- [ ] `decode` returns empty (Display mode).
- [ ] The rendered Typst compiles through `compile_document_in` without errors.

**Verify:** `nix develop -c cargo test -p inkapp-core --test heading_component` → all passing.

**Steps:**

- [ ] **Step 1: Write the authored Typst module `crates/inkapp-core/typst/heading.typ`**

```typst
#import "/inkapp/region.typ": region

// Heading render half. Theme tones (heading/muted) are passed in as strings so the
// Rust caller controls colour from `Theme`. Named `heading-block` to avoid Typst's
// built-in `heading` keyword.
#let heading-block(title, byline: none, meta: none, subtitle: none, heading-color: "#1a1a1a", muted-color: "#666") = {
  block(below: 6pt, text(weight: "bold", size: 18pt, fill: rgb(heading-color), title))
  if byline != none {
    block(below: 2pt, text(size: 10pt, weight: "medium", fill: rgb(muted-color), byline))
  }
  if meta != none {
    block(below: 6pt, text(size: 9pt, fill: rgb(muted-color), meta))
  }
  if subtitle != none {
    block(below: 6pt, text(size: 10pt, style: "italic", fill: rgb(muted-color), subtitle))
  }
  v(2pt)
}
```

- [ ] **Step 2: Write the failing tests in `crates/inkapp-core/tests/heading_component.rs`**

```rust
//! Heading component: render shape, byline/meta optionality, decode-empty.

use inkapp_core::component::{Component, RenderCx};
use inkapp_core::components::heading::Heading;
use inkapp_core::render::compile_to_document_with_sources;
use inkapp_core::theme::Theme;

fn render(h: &Heading) -> String {
    let theme = Theme::reader();
    let mut cx = RenderCx::new(0).with_theme(theme);
    h.render(&mut cx)
}

#[test]
fn title_only_renders() {
    let out = render(&Heading::new("Hello world"));
    assert!(out.contains("Hello world"), "title in output: {out}");
}

#[test]
fn all_fields_render() {
    let h = Heading::new("Title")
        .byline("Jane")
        .reading_time("5 min")
        .subtitle("a summary");
    let out = render(&h);
    for fragment in ["Title", "Jane", "5 min", "a summary"] {
        assert!(out.contains(fragment), "missing {fragment}: {out}");
    }
}

#[test]
fn heading_typst_compiles() {
    let h = Heading::new("Compilable").byline("Author").reading_time("3 min");
    let theme = Theme::reader();
    let mut cx = RenderCx::new(0).with_theme(theme.clone());
    let body = h.render(&mut cx);
    let src = format!(
        "#import \"/inkapp/heading.typ\": *\n#set page(width: 200pt, height: 200pt, margin: 8pt)\n{}\n{body}",
        theme.prelude()
    );
    let sources = vec![
        ("/inkapp/region.typ".into(), include_str!("../typst/region.typ").into()),
        ("/inkapp/heading.typ".into(), include_str!("../typst/heading.typ").into()),
    ];
    compile_to_document_with_sources(&src, &sources).expect("Heading typst compiles");
}

#[test]
fn decode_is_empty() {
    let h = Heading::new("x");
    let manifest = inkapp_core::manifest::Manifest::default();
    let msgs = <Heading as Component>::decode(&h, &[], &manifest);
    let _: Vec<()> = msgs; // Heading::Msg = ()
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `nix develop -c cargo test -p inkapp-core --test heading_component`
Expected: FAIL (`Heading` doesn't exist yet).

- [ ] **Step 4: Implement `crates/inkapp-core/src/components/heading.rs`**

```rust
//! `Heading` — a reusable Display component for long-form article/section
//! openers: title, optional byline (author OR site_name fallback at the call
//! site), optional reading-time, optional subtitle. Theme-aware via RenderCx.

use crate::component::{Component, RenderCx};
use crate::components::esc_typst_str;
use crate::ink::RegionInk;
use crate::manifest::Manifest;

/// Title + optional byline/reading-time/subtitle. Mirrors the metadata Readwise
/// exposes (title/author/site_name/reading_time/summary), but the component is
/// content-agnostic — pass whatever strings make sense.
pub struct Heading {
    title: String,
    byline: Option<String>,
    reading_time: Option<String>,
    subtitle: Option<String>,
}

impl Heading {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            byline: None,
            reading_time: None,
            subtitle: None,
        }
    }

    #[must_use]
    pub fn byline(mut self, s: impl Into<String>) -> Self {
        self.byline = Some(s.into());
        self
    }

    #[must_use]
    pub fn reading_time(mut self, s: impl Into<String>) -> Self {
        self.reading_time = Some(s.into());
        self
    }

    #[must_use]
    pub fn subtitle(mut self, s: impl Into<String>) -> Self {
        self.subtitle = Some(s.into());
        self
    }
}

const HEADING_TYPST: (&str, &str) = (
    "/inkapp/heading.typ",
    include_str!("../../typst/heading.typ"),
);

impl Component for Heading {
    type Msg = ();

    fn render(&self, cx: &mut RenderCx) -> String {
        let theme = cx.theme();
        let heading_hex = theme.palette().heading.clone();
        let muted_hex = theme.palette().muted.clone();
        let title = esc_typst_str(&self.title);
        let mut call = format!(
            "#heading-block(\"{title}\", heading-color: \"{heading_hex}\", muted-color: \"{muted_hex}\""
        );
        if let Some(b) = &self.byline {
            call.push_str(&format!(", byline: \"{}\"", esc_typst_str(b)));
        }
        if let Some(r) = &self.reading_time {
            call.push_str(&format!(", meta: \"{}\"", esc_typst_str(r)));
        }
        if let Some(s) = &self.subtitle {
            call.push_str(&format!(", subtitle: \"{}\"", esc_typst_str(s)));
        }
        call.push_str(")\n");
        call
    }

    fn typst_sources(&self) -> Vec<(String, String)> {
        vec![(HEADING_TYPST.0.into(), HEADING_TYPST.1.into())]
    }

    fn decode(&self, _ink: &[RegionInk], _manifest: &Manifest) -> Vec<Self::Msg> {
        Vec::new()
    }
}
```

- [ ] **Step 5: Register the module in `crates/inkapp-core/src/components/mod.rs`**

Add `pub mod heading;` alongside the existing `pub mod gesture;` etc., in alphabetical order.

- [ ] **Step 6: Confirm `RenderCx::theme()` and `Theme::palette()` return the field shapes used above**

Run: `nix develop -c cargo check -p inkapp-core`
If `palette().heading` / `palette().muted` are not the actual field names on `Palette`, fix the call sites (they should match the existing `Theme::reader()` palette). If `RenderCx::theme()` returns a borrowed `&Theme`, clone the palette fields out. The Index component uses the same surface — model after it.

- [ ] **Step 7: Run tests to verify they pass**

Run: `nix develop -c cargo test -p inkapp-core --test heading_component`
Expected: 4 passing.

- [ ] **Step 8: Mark task 0 complete in `.tasks.json` and commit**

```bash
git add crates/inkapp-core/typst/heading.typ \
        crates/inkapp-core/src/components/heading.rs \
        crates/inkapp-core/src/components/mod.rs \
        crates/inkapp-core/tests/heading_component.rs \
        docs/superpowers/plans/2026-05-26-reader-app.md.tasks.json
git commit -m "inkapp-core: Heading Display component (title/byline/meta/subtitle)"
```

---

## Task 1: `Section<M>` component + authored `#section`

**Goal:** A reusable wrapper that opens a per-section Typst state and a weak page break, so a per-page header can know which section it belongs to. The state name is fixed (`inkapp.section`) to keep the API simple — extensibility is in [open question 2] of the spec, not v1.

**Files:**
- Create: `crates/inkapp-core/typst/section.typ`
- Create: `crates/inkapp-core/src/components/section.rs`
- Modify: `crates/inkapp-core/src/components/mod.rs`
- Create: `crates/inkapp-core/tests/section_component.rs`

**Acceptance Criteria:**
- [ ] `Section::new(id, body_components)` constructs a section with id `id` and a body.
- [ ] `render` emits an authored `#section("<id>", { ... body ... })` call.
- [ ] The body's components' Typst is composed into the section call so they're laid out inside it.
- [ ] `decode` delegates to each body component's `decode`.
- [ ] A two-section document paginates with each section starting on a fresh page (weak break — the *very* first section doesn't force a blank page before it).
- [ ] After compilation, the `inkapp.section` state at any page resolves to the most-recently-set id (verified indirectly via a probe block in the test that prints state value into a region we can recover).

**Verify:** `nix develop -c cargo test -p inkapp-core --test section_component` → all passing.

**Steps:**

- [ ] **Step 1: Authored Typst — `crates/inkapp-core/typst/section.typ`**

```typst
#import "/inkapp/region.typ": region

// One section. Sets the `inkapp.section` state to `id` and forces a weak page
// break, then lays out `body`. A per-page header that reads
// `state("inkapp.section").at(here().position())` will see this id on every page
// covered by this section.
#let section-state = state("inkapp.section", "")

#let section(id, body) = {
  // Force a fresh page (weak: no blank pages for the very first section).
  pagebreak(weak: true)
  // Update the section state — observable from any later read on this & subsequent pages.
  section-state.update(id)
  body
}
```

- [ ] **Step 2: Write the failing tests in `crates/inkapp-core/tests/section_component.rs`**

```rust
//! Section component: pagebreak between sections, body composition, decode delegation,
//! and section-state observability via a recoverable probe region.

use inkapp_core::component::{Component, RenderCx};
use inkapp_core::components::notice::Notice;
use inkapp_core::components::section::Section;
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::compile_to_document_with_sources;
use inkapp_core::theme::Theme;

fn sources() -> Vec<(String, String)> {
    vec![
        ("/inkapp/region.typ".into(), include_str!("../typst/region.typ").into()),
        ("/inkapp/section.typ".into(), include_str!("../typst/section.typ").into()),
    ]
}

#[test]
fn render_emits_section_call_with_id() {
    let s: Section<()> = Section::new(
        "art-1",
        vec![Box::new(Notice::line("hello"))],
    );
    let mut cx = RenderCx::new(0).with_theme(Theme::reader());
    let out = s.render(&mut cx);
    assert!(out.contains("section(\"art-1\""), "id in call: {out}");
    assert!(out.contains("hello"), "body composed: {out}");
}

#[test]
fn two_sections_produce_multiple_pages() {
    let theme = Theme::reader();
    let s1: Section<()> = Section::new("a", vec![Box::new(Notice::line("first"))]);
    let s2: Section<()> = Section::new("b", vec![Box::new(Notice::line("second"))]);
    let mut cx = RenderCx::new(0).with_theme(theme.clone());
    let src = format!(
        "#import \"/inkapp/section.typ\": *\n#set page(width: 200pt, height: 100pt, margin: 6pt)\n{}\n{}{}",
        theme.prelude(),
        s1.render(&mut cx),
        s2.render(&mut cx),
    );
    let doc = compile_to_document_with_sources(&src, &sources()).unwrap();
    assert!(doc.pages.len() >= 2, "two sections should paginate to ≥2 pages; got {}", doc.pages.len());
}

#[test]
fn decode_delegates_to_body() {
    // A body Notice decodes empty; this just exercises that Section's decode loops
    // over children without panic. Real per-child decode tested in ActionBand integration.
    let s: Section<()> = Section::new("x", vec![Box::new(Notice::line("noop"))]);
    let manifest = inkapp_core::manifest::Manifest::default();
    let msgs = <Section<()> as Component>::decode(&s, &[], &manifest);
    assert!(msgs.is_empty());
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `nix develop -c cargo test -p inkapp-core --test section_component`
Expected: FAIL (`Section` doesn't exist).

- [ ] **Step 4: Implement `crates/inkapp-core/src/components/section.rs`**

```rust
//! `Section<M>` — wraps a body in an authored `#section("<id>", ...)` call that
//! sets the `inkapp.section` Typst state to `id` and forces a weak page break.
//! A per-page header (see ActionBand) reads that state to know which section it
//! belongs to on any given page.

use crate::component::{Component, RenderCx};
use crate::components::esc_typst_str;
use crate::ink::RegionInk;
use crate::manifest::Manifest;

const SECTION_TYPST: (&str, &str) = (
    "/inkapp/section.typ",
    include_str!("../../typst/section.typ"),
);

pub struct Section<M> {
    id: String,
    body: Vec<Box<dyn Component<Msg = M>>>,
}

impl<M> Section<M> {
    pub fn new(id: impl Into<String>, body: Vec<Box<dyn Component<Msg = M>>>) -> Self {
        Self {
            id: id.into(),
            body,
        }
    }
}

impl<M> Component for Section<M> {
    type Msg = M;

    fn render(&self, cx: &mut RenderCx) -> String {
        let id = esc_typst_str(&self.id);
        let mut body_src = String::new();
        for c in &self.body {
            body_src.push_str(&c.render(cx));
        }
        // The body is wrapped in `[...]` so Typst treats it as content.
        format!("#section(\"{id}\", [\n{body_src}])\n")
    }

    fn typst_sources(&self) -> Vec<(String, String)> {
        let mut out = vec![(SECTION_TYPST.0.into(), SECTION_TYPST.1.into())];
        for c in &self.body {
            out.extend(c.typst_sources());
        }
        out
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<M> {
        let mut out = Vec::new();
        for c in &self.body {
            out.extend(c.decode(ink, manifest));
        }
        out
    }

    fn image_urls(&self) -> Vec<String> {
        let mut out = Vec::new();
        for c in &self.body {
            out.extend(c.image_urls());
        }
        out
    }
}
```

- [ ] **Step 5: Register in `crates/inkapp-core/src/components/mod.rs`**

Add `pub mod section;` alphabetically.

- [ ] **Step 6: Run tests to verify they pass**

Run: `nix develop -c cargo test -p inkapp-core --test section_component`
Expected: 3 passing.

- [ ] **Step 7: Mark task 1 complete in `.tasks.json` and commit**

```bash
git add crates/inkapp-core/typst/section.typ \
        crates/inkapp-core/src/components/section.rs \
        crates/inkapp-core/src/components/mod.rs \
        crates/inkapp-core/tests/section_component.rs \
        docs/superpowers/plans/2026-05-26-reader-app.md.tasks.json
git commit -m "inkapp-core: Section<M> component + #section authored module"
```

---

## Task 2: `Document::page_header` + render & decode wiring

**Goal:** A `Document<M>` can carry an optional page-header component the framework wires into `#set page(header: ...)` and decodes per cycle.

**Files:**
- Modify: `crates/inkapp-core/src/document.rs`
- Modify: `crates/inkapp-core/src/runtime.rs` (functions `collect_typst_sources`, `document_source_in`, and the decode loop inside `App::step`)
- Create: `crates/inkapp-core/tests/page_header.rs`

**Acceptance Criteria:**
- [ ] `Document::page_header(component)` is a chainable builder method (`#[must_use]`).
- [ ] `document_source_in` emits `#set page(... , header: { <header.render(cx)> })` when a header is set, AFTER `#set page(width:...)` (so the size is locked but the header overrides defaults).
- [ ] `collect_typst_sources` includes the header component's sources.
- [ ] `App::step` calls `header.decode(...)` and folds its Msgs alongside flow decodes.
- [ ] A doc with a `Notice` page header renders the notice on every page (proved via `assert!(doc.pages.iter().all(|p| ... )` or by counting recovered header regions per page frame).

**Verify:** `nix develop -c cargo test -p inkapp-core --test page_header` → all passing.

**Steps:**

- [ ] **Step 1: Write the failing tests in `crates/inkapp-core/tests/page_header.rs`**

```rust
//! Document::page_header — header is rendered into `#set page(header: ...)` and
//! its regions appear on every page frame.

use inkapp_core::components::notice::Notice;
use inkapp_core::document::Document;
use inkapp_core::manifest::recover_regions;
use inkapp_core::components::passage::Passage;
use inkapp_core::runtime::{collect_typst_sources, document_source_in};
use inkapp_core::flow;
use inkapp_core::theme::Theme;

#[test]
fn header_source_contains_set_page_header() {
    // A long passage forces multi-page; the header should be rendered into #set page(header:).
    let doc: Document<()> = Document::keyed(
        "doc",
        flow![Passage::new("body", &(0..200).map(|i| format!("line {i}")).collect::<Vec<_>>().iter().map(String::as_str).collect::<Vec<_>>())],
    ).page_header(Notice::line("HEADER MARK"));

    let src = document_source_in(&doc, Default::default(), &Theme::reader());
    assert!(src.contains("#set page(header:"), "header set: {src:.200}");
    assert!(src.contains("HEADER MARK"), "header text in source");
}

#[test]
fn collect_sources_includes_header_typst() {
    let doc: Document<()> = Document::keyed("d", flow![Notice::line("body")])
        .page_header(Notice::line("hd"));
    let sources = collect_typst_sources(&doc);
    assert!(sources.iter().any(|(p, _)| p == "/inkapp/region.typ"), "prelude present");
    // Notice has no authored sources today; the assertion is that we don't crash and we
    // include whatever the header brings. If Notice grows authored sources later, this
    // will still hold by construction.
}

#[test]
fn header_regions_appear_on_every_page() {
    // Make a wide-bodied doc with a Passage that splits across at least 2 pages, plus a
    // page header that emits a recoverable region named `hd-mark` per page.
    use inkapp_core::manifest::Manifest;
    use inkapp_core::components::passage::Passage;
    let body_lines: Vec<String> = (0..120).map(|i| format!("Line of body content {i}")).collect();
    let line_refs: Vec<&str> = body_lines.iter().map(String::as_str).collect();
    let body = Passage::new("body", &line_refs);
    let header = Passage::new("hd-mark", &["page-header sentinel"]);

    let doc: Document<()> = Document::keyed("d", flow![body]).page_header(header);
    let geom = inkapp_core::geometry::PageGeom { w: 200.0, h: 150.0, margin: 8.0 };

    let src = document_source_in(&doc, geom, &Theme::reader());
    let sources = collect_typst_sources(&doc);
    let compiled = inkapp_core::render::compile_to_document_with_sources(&src, &sources).unwrap();
    assert!(compiled.pages.len() >= 2, "test fixture must span ≥2 pages; got {}", compiled.pages.len());

    let manifest: Manifest = recover_regions(&compiled).unwrap();
    let hd_regions: Vec<_> = manifest.regions.iter().filter(|r| r.name == "hd-mark").collect();
    assert!(
        hd_regions.len() >= compiled.pages.len(),
        "expected ≥1 'hd-mark' region per page; got {} regions over {} pages",
        hd_regions.len(),
        compiled.pages.len()
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `nix develop -c cargo test -p inkapp-core --test page_header`
Expected: FAIL (`Document::page_header` doesn't exist).

- [ ] **Step 3: Extend `Document<M>` in `crates/inkapp-core/src/document.rs`**

Add the field and builder. Replace the struct definition + impl block:

```rust
/// One document: a key plus an ordered flow of components, plus optional
/// app-owned document-level state carried in the sealed manifest, plus an
/// optional per-page header component (rendered on every page).
pub struct Document<M> {
    pub key: DocKey,
    pub flow: Vec<Box<dyn Component<Msg = M>>>,
    pub state: Option<serde_json::Value>,
    pub page_header: Option<Box<dyn Component<Msg = M>>>,
}

impl<M> Document<M> {
    pub fn keyed(key: impl Into<String>, flow: Vec<Box<dyn Component<Msg = M>>>) -> Self {
        Self {
            key: DocKey::new(key),
            flow,
            state: None,
            page_header: None,
        }
    }

    pub fn keyed_with_state(
        key: impl Into<String>,
        flow: Vec<Box<dyn Component<Msg = M>>>,
        state: serde_json::Value,
    ) -> Self {
        Self {
            key: DocKey::new(key),
            flow,
            state: Some(state),
            page_header: None,
        }
    }

    /// Attach a per-page header component. The framework wires its render into
    /// `#set page(header: ...)` and calls its `decode` alongside the flow decode.
    #[must_use]
    pub fn page_header(mut self, header: impl Component<Msg = M> + 'static) -> Self {
        self.page_header = Some(Box::new(header));
        self
    }
}
```

- [ ] **Step 4: Modify `collect_typst_sources` in `crates/inkapp-core/src/runtime.rs`**

Find the existing function and update it to include the header's sources too. The function currently iterates `doc.flow`; extend with the header:

```rust
pub fn collect_typst_sources<M>(doc: &Document<M>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> =
        vec![(REGION_PRELUDE.0.to_string(), REGION_PRELUDE.1.to_string())];
    for c in &doc.flow {
        for src in c.typst_sources() {
            if !out.iter().any(|(p, _)| p == &src.0) {
                out.push(src);
            }
        }
    }
    if let Some(h) = &doc.page_header {
        for src in h.typst_sources() {
            if !out.iter().any(|(p, _)| p == &src.0) {
                out.push(src);
            }
        }
    }
    out
}
```

- [ ] **Step 5: Modify `document_source_in` in `crates/inkapp-core/src/runtime.rs`**

Update to emit `#set page(header: ...)` when a header exists. Insert a second `#set page` AFTER the existing one and AFTER the theme prelude (so the theme's text/par defaults apply inside the header), and BEFORE the body:

```rust
pub fn document_source_in<M>(doc: &Document<M>, geom: PageGeom, theme: &Theme) -> String {
    let mut cx = RenderCx::new(0).with_theme(theme.clone());
    let mut src = String::new();
    for (path, _) in collect_typst_sources(doc) {
        src.push_str(&format!("#import \"{path}\": *\n"));
    }
    src.push_str(&format!(
        "#set page(width: {}pt, height: {}pt, margin: {}pt)\n",
        geom.w, geom.h, geom.margin
    ));
    src.push_str(&theme.prelude());
    if let Some(h) = &doc.page_header {
        let header_typst = h.render(&mut cx);
        // The header content is injected verbatim into the page-header slot.
        src.push_str(&format!("#set page(header: [{header_typst}])\n"));
    }
    for c in &doc.flow {
        src.push_str(&c.render(&mut cx));
    }
    src
}
```

- [ ] **Step 6: Modify the decode loop in `App::step`**

Locate the section of `runtime.rs` that decodes ink against `doc.flow` (search for `c.decode(&region_ink, &entry.manifest)`). Insert a header decode before the body decode:

```rust
for doc in &pre.0 {
    let Some(pages) = ink_by_key.get(&doc.key.0) else { continue; };
    let Some(entry) = set.entries.get(&doc.key.0) else { continue; };
    guard_version(entry.version, &entry.manifest)?;
    let region_ink = attribute(pages, &entry.manifest);
    if let Some(h) = &doc.page_header {
        decoded.extend(h.decode(&region_ink, &entry.manifest));
    }
    for c in &doc.flow {
        decoded.extend(c.decode(&region_ink, &entry.manifest));
    }
}
```

- [ ] **Step 7: Also collect image_urls from the header in App::render**

Search runtime.rs for the `image_urls` collection (used by the image pipeline). It iterates `doc.flow`; extend to include `doc.page_header.as_ref().map(|h| h.image_urls())` if a header exists. If you can't find an explicit collection (it may live elsewhere), grep for `image_urls` in `runtime.rs` and update consistently.

```rust
// Pseudocode at the call site (real signature depends on local helper):
let mut urls: Vec<String> = doc.flow.iter().flat_map(|c| c.image_urls()).collect();
if let Some(h) = &doc.page_header {
    urls.extend(h.image_urls());
}
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `nix develop -c cargo test -p inkapp-core --test page_header`
Expected: 3 passing. Also run `nix develop -c cargo test --workspace` to confirm no regression elsewhere — Document is widely used, but our additions are field-additive (`None` by default) and the public API gains a new builder, so existing call-sites compile unchanged.

- [ ] **Step 9: Mark task 2 complete in `.tasks.json` and commit**

```bash
git add crates/inkapp-core/src/document.rs \
        crates/inkapp-core/src/runtime.rs \
        crates/inkapp-core/tests/page_header.rs \
        docs/superpowers/plans/2026-05-26-reader-app.md.tasks.json
git commit -m "inkapp-core: Document::page_header — per-page header render + decode wiring"
```

---

## Task 3: `ActionBand<M>` component + authored `#action-band`

**Goal:** A reusable N-cell per-page action band that reads the current `inkapp.section` state on each page, lays out one labeled cell per action with a `#region("action-{label}-{section_id}", ...)` wrapper, and decodes pen strikes per cell into a Msg via a per-cell `Fn(section_id) -> M` closure.

**Files:**
- Create: `crates/inkapp-core/typst/action_band.typ`
- Create: `crates/inkapp-core/src/components/action_band.rs`
- Modify: `crates/inkapp-core/src/components/mod.rs`
- Create: `crates/inkapp-core/tests/action_band.rs`

**Acceptance Criteria:**
- [ ] `ActionBand::new([(label, closure)])` builds an N-cell band.
- [ ] `render` emits an authored `#action-band(...)` call that reads section state.
- [ ] Each cell's region is named `action-{label}-{section_id}` and recovers on every page (one per page).
- [ ] Decode iterates regions, classifies non-highlighter pen strikes spanning ≥`STRIKE_WIDTH_RATIO` of cell width as a hit, parses `(label, section_id)` from the region name, and invokes the matching closure.
- [ ] A unit test drives synthetic strike ink centred on the "Archive" cell of section "art-1" and asserts the closure for "Archive" was called with `"art-1"`.
- [ ] A wrong-tool (highlighter) stroke does NOT fire.
- [ ] An empty-ink scenario fires nothing.

**Verify:** `nix develop -c cargo test -p inkapp-core --test action_band` → all passing.

**Steps:**

- [ ] **Step 1: Authored Typst — `crates/inkapp-core/typst/action_band.typ`**

```typst
#import "/inkapp/region.typ": region
#import "/inkapp/section.typ": section-state

// One row of action cells; each cell is a labelled region named
// `action-{label}-{section_id}`. `labels` is an array of strings.
// `section_id` is read per-page from the inkapp.section state.
#let action-band(labels) = context {
  let sid = section-state.at(here().position())
  if sid == "" {
    // No section yet (e.g. the index page): render an inert band with no actions.
    block(height: 18pt, [])
  } else {
    grid(
      columns: labels.len(),
      column-gutter: 6pt,
      ..labels.map(label => region(
        "action-" + label + "-" + sid,
        box(width: 100%, height: 18pt, stroke: 0.5pt, inset: 3pt, align(center + horizon, text(size: 9pt, label)))
      )),
    )
  }
}
```

- [ ] **Step 2: Write the failing tests in `crates/inkapp-core/tests/action_band.rs`**

```rust
//! ActionBand: render emits per-cell regions keyed by section, decode classifies
//! a pen strike on the right cell into the right Msg.

use std::sync::{Arc, Mutex};

use inkapp_core::component::{Component, RenderCx};
use inkapp_core::components::action_band::ActionBand;
use inkapp_core::components::notice::Notice;
use inkapp_core::components::passage::Passage;
use inkapp_core::components::section::Section;
use inkapp_core::document::Document;
use inkapp_core::flow;
use inkapp_core::geometry::{PageGeom, PdfRect};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::{recover_regions, Manifest};
use inkapp_core::render::compile_to_document_with_sources;
use inkapp_core::runtime::{collect_typst_sources, document_source_in};
use inkapp_core::theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
enum TestMsg { Inbox(String), Archive(String) }

fn band_with_recorder() -> (ActionBand<TestMsg>, Arc<Mutex<Vec<TestMsg>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let log_a = log.clone();
    let log_i = log.clone();
    let band = ActionBand::new([
        ("Inbox".to_string(), Box::new(move |id: &str| { let m = TestMsg::Inbox(id.into()); log_i.lock().unwrap().push(m.clone()); m }) as Box<dyn Fn(&str) -> TestMsg + Send + Sync>),
        ("Archive".to_string(), Box::new(move |id: &str| { let m = TestMsg::Archive(id.into()); log_a.lock().unwrap().push(m.clone()); m })),
    ]);
    (band, log)
}

fn compile_doc_with_band(band: ActionBand<TestMsg>) -> (Document<TestMsg>, Manifest, Vec<usize>) {
    // Two short sections so the band sees two distinct section ids on different pages.
    let s1: Section<TestMsg> = Section::new("art-1", vec![Box::new(Notice::line("first"))]);
    let s2: Section<TestMsg> = Section::new("art-2", vec![Box::new(Notice::line("second"))]);
    let doc: Document<TestMsg> = Document::keyed("library", flow![s1, s2]).page_header(band);
    let geom = PageGeom { w: 200.0, h: 120.0, margin: 6.0 };
    let src = document_source_in(&doc, geom, &Theme::reader());
    let sources = collect_typst_sources(&doc);
    let compiled = compile_to_document_with_sources(&src, &sources).unwrap();
    let manifest = recover_regions(&compiled).unwrap();
    let page_count = compiled.pages.len();
    (doc, manifest, (0..page_count).collect())
}

#[test]
fn render_produces_per_section_action_regions() {
    let (band, _log) = band_with_recorder();
    let (_doc, manifest, _) = compile_doc_with_band(band);
    let names: Vec<_> = manifest.regions.iter().map(|r| r.name.clone()).collect();
    assert!(names.iter().any(|n| n == "action-Inbox-art-1"),   "Inbox/art-1 region: {names:?}");
    assert!(names.iter().any(|n| n == "action-Archive-art-1"), "Archive/art-1 region: {names:?}");
    assert!(names.iter().any(|n| n == "action-Inbox-art-2"),   "Inbox/art-2 region: {names:?}");
}

fn strike_across(rect: &PdfRect) -> Stroke {
    let y_mid = (rect.y0 + rect.y1) / 2.0;
    Stroke {
        points: (0..=10).map(|i| inkapp_core::geometry::PdfPoint {
            x: rect.x0 + (rect.x1 - rect.x0) * (i as f64 / 10.0),
            y: y_mid,
        }).collect(),
        highlighter: false,
    }
}

#[test]
fn pen_strike_on_archive_art1_fires_the_archive_closure() {
    let (band, _log) = band_with_recorder();
    let (doc, manifest, _) = compile_doc_with_band(band);
    let header = doc.page_header.as_ref().unwrap();

    let target = manifest.regions.iter().find(|r| r.name == "action-Archive-art-1").unwrap();
    let region_ink = vec![RegionInk {
        region: "action-Archive-art-1".into(),
        strokes: vec![strike_across(&target.rect)],
    }];
    let msgs = header.decode(&region_ink, &manifest);
    assert_eq!(msgs, vec![TestMsg::Archive("art-1".into())]);
}

#[test]
fn highlighter_stroke_does_not_fire() {
    let (band, _log) = band_with_recorder();
    let (doc, manifest, _) = compile_doc_with_band(band);
    let header = doc.page_header.as_ref().unwrap();

    let target = manifest.regions.iter().find(|r| r.name == "action-Archive-art-1").unwrap();
    let mut s = strike_across(&target.rect);
    s.highlighter = true;
    let region_ink = vec![RegionInk { region: "action-Archive-art-1".into(), strokes: vec![s] }];
    let msgs = header.decode(&region_ink, &manifest);
    assert!(msgs.is_empty(), "highlighter must not fire actions: {msgs:?}");
}

#[test]
fn empty_ink_fires_nothing() {
    let (band, _log) = band_with_recorder();
    let (doc, manifest, _) = compile_doc_with_band(band);
    let header = doc.page_header.as_ref().unwrap();
    let msgs = header.decode(&[], &manifest);
    assert!(msgs.is_empty());
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `nix develop -c cargo test -p inkapp-core --test action_band`
Expected: FAIL (`ActionBand` doesn't exist).

- [ ] **Step 4: Implement `crates/inkapp-core/src/components/action_band.rs`**

```rust
//! `ActionBand<M>` — a per-page header of N labelled cells. Each cell carries
//! a `Fn(section_id: &str) -> M` closure; on decode, a non-highlighter pen
//! stroke that spans most of a cell's width fires that cell's closure with
//! the section id parsed from the region name (`action-{label}-{section_id}`).
//!
//! The closure is the appdx-documented escape hatch: a reusable content
//! component whose message depends on both *which cell was struck* (label) and
//! *which section the page belonged to* (section id) — both content-derived.

use crate::component::{Component, RenderCx};
use crate::components::esc_typst_str;
use crate::ink::RegionInk;
use crate::manifest::Manifest;

/// Strike width as a fraction of the cell's width. A non-highlighter stroke
/// must span at least this much of the cell's width in X to count.
const STRIKE_WIDTH_RATIO: f64 = 0.5;

type Handler<M> = Box<dyn Fn(&str) -> M + Send + Sync>;

const ACTION_BAND_TYPST: (&str, &str) = (
    "/inkapp/action_band.typ",
    include_str!("../../typst/action_band.typ"),
);

pub struct ActionBand<M> {
    cells: Vec<(String, Handler<M>)>,
}

impl<M> ActionBand<M> {
    pub fn new(cells: impl IntoIterator<Item = (String, Handler<M>)>) -> Self {
        Self { cells: cells.into_iter().collect() }
    }

    /// The labels, in order — used for the Typst call.
    fn labels(&self) -> Vec<&str> {
        self.cells.iter().map(|(l, _)| l.as_str()).collect()
    }
}

impl<M> Component for ActionBand<M> {
    type Msg = M;

    fn render(&self, _cx: &mut RenderCx) -> String {
        let labels = self
            .labels()
            .iter()
            .map(|l| format!("\"{}\"", esc_typst_str(l)))
            .collect::<Vec<_>>()
            .join(", ");
        format!("#action-band(({labels}, ))\n") // trailing comma keeps single-cell as array
    }

    fn typst_sources(&self) -> Vec<(String, String)> {
        // The action_band module imports section.typ; both must be registered.
        vec![
            (ACTION_BAND_TYPST.0.into(), ACTION_BAND_TYPST.1.into()),
            ("/inkapp/section.typ".into(), include_str!("../../typst/section.typ").into()),
        ]
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<M> {
        let mut out = Vec::new();
        for ri in ink {
            // Region name shape: "action-{label}-{section_id}".
            let Some(rest) = ri.region.strip_prefix("action-") else { continue; };
            // We need to split into (label, section_id). Labels never contain '-'
            // in this app, but defensively: match against known labels.
            let Some((label, section_id)) = self.cells.iter().find_map(|(lbl, _)| {
                rest.strip_prefix(lbl.as_str())
                    .and_then(|s| s.strip_prefix('-'))
                    .map(|sid| (lbl.as_str(), sid))
            }) else {
                continue;
            };

            // Find this region's rect in the manifest to know the cell's width.
            let Some(region) = manifest.regions.iter().find(|r| r.name == ri.region) else { continue; };
            let cell_w = region.rect.x1 - region.rect.x0;

            // Classify: any non-highlighter stroke spanning ≥ STRIKE_WIDTH_RATIO of cell width.
            let fires = ri.strokes.iter().any(|s| {
                if s.highlighter { return false; }
                let xs: Vec<f64> = s.points.iter().map(|p| p.x).collect();
                if xs.len() < 2 { return false; }
                let min_x = xs.iter().cloned().fold(f64::INFINITY, f64::min);
                let max_x = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                (max_x - min_x) >= STRIKE_WIDTH_RATIO * cell_w
            });
            if fires {
                if let Some((_, handler)) = self.cells.iter().find(|(l, _)| l == label) {
                    out.push(handler(section_id));
                }
            }
        }
        out
    }
}
```

- [ ] **Step 5: Register in `crates/inkapp-core/src/components/mod.rs`**

Add `pub mod action_band;` alphabetically.

- [ ] **Step 6: Run tests to verify they pass**

Run: `nix develop -c cargo test -p inkapp-core --test action_band`
Expected: 4 passing. If the Typst `#action-band` call shape doesn't compile (e.g. the labels-array syntax), iterate on `action_band.typ` and the Rust render call until both tests `render_produces_per_section_action_regions` and `pen_strike_on_archive_art1_fires_the_archive_closure` pass.

- [ ] **Step 7: Mark task 3 complete in `.tasks.json` and commit**

```bash
git add crates/inkapp-core/typst/action_band.typ \
        crates/inkapp-core/src/components/action_band.rs \
        crates/inkapp-core/src/components/mod.rs \
        crates/inkapp-core/tests/action_band.rs \
        docs/superpowers/plans/2026-05-26-reader-app.md.tasks.json
git commit -m "inkapp-core: ActionBand<M> — per-page header N-cell action band"
```

---

## Task 4: `push_replace_ink` on `DeviceTransport` + `CloudTransport` impl

**Goal:** A second push variant that replaces (not preserves) the on-device ink. `publish` keeps the existing `push` (preserve pending ink); `sync_once`'s post-fold push uses `push_replace_ink` because the just-pulled ink has been folded into state and the next render embeds its effect.

**Files:**
- Modify: `crates/inkapp-core/src/sync.rs`
- Modify: `crates/rm-device/src/transport.rs`
- Create: `crates/inkapp-core/tests/sync_replace_ink.rs`
- Modify: `crates/inkapp-core/tests` existing FakeTransport (extend it; check where it lives — likely `crates/inkapp-core/src/sync.rs`'s `#[cfg(test)]` mod or a `tests/common`)

**Acceptance Criteria:**
- [ ] `DeviceTransport` trait has `async fn push_replace_ink(&self, key: &str, pdf: &[u8]) -> Result<()>` with a default implementation that calls `push` (so existing transports keep working).
- [ ] `sync_once` uses `transport.push_replace_ink(...)` for the post-fold push.
- [ ] `publish` (and the initial `app.render(set)` push inside `sync_once`'s pre-pull-rebuild step) still uses `transport.push(...)`.
- [ ] `CloudTransport::push_replace_ink` overrides the default: fetches `DocFiles` for the doc, replaces the `.pdf` blob, strips every `.rm` blob, and calls `client.put(docfiles)`.
- [ ] A unit test against a fake transport asserts: `publish` invokes only `push`; `sync_once` after a pull that yields one stroke invokes `push_replace_ink` (not `push`) for the rendered output.

**Verify:** `nix develop -c cargo test -p inkapp-core --test sync_replace_ink` → passing; `nix develop -c cargo test -p rm-device` still passing.

**Steps:**

- [ ] **Step 1: Extend the trait in `crates/inkapp-core/src/sync.rs`**

```rust
#[async_trait::async_trait]
pub trait DeviceTransport: Send + Sync {
    /// Push a rendered document (its key + PDF bytes) to the device, preserving
    /// any existing on-device ink layer (the default `publish` semantic).
    async fn push(&self, key: &str, pdf: &[u8]) -> Result<()>;

    /// Push a rendered document AND replace the on-device ink layer with empty.
    /// `sync_once` calls this after a fold — the pulled ink has been turned into
    /// state, the next render reflects it, so preserving the (now-stale-against-
    /// shifted-content) per-page raster is wrong.
    ///
    /// Default implementation delegates to `push` for transports that don't yet
    /// distinguish; specific backends (reMarkable `CloudTransport`) override.
    async fn push_replace_ink(&self, key: &str, pdf: &[u8]) -> Result<()> {
        self.push(key, pdf).await
    }

    /// Delete a document by key. Best-effort: a missing document is not an error.
    async fn delete(&self, key: &str);

    /// Pull all device ink, keyed by document key, as PDF-space strokes.
    async fn pull(&self, page_h_by_key: &HashMap<String, f64>)
        -> HashMap<String, Vec<Vec<Stroke>>>;
}
```

- [ ] **Step 2: Change the post-fold push in `sync_once`**

Find the existing post-fold push loop and switch the method:

```rust
for rd in &cycle.rendered {
    transport.push_replace_ink(&rd.key.0, &rd.pdf).await?;
}
```

- [ ] **Step 3: Override `CloudTransport::push_replace_ink` in `crates/rm-device/src/transport.rs`**

Append to the `impl DeviceTransport for CloudTransport` block:

```rust
async fn push_replace_ink(&self, key: &str, pdf: &[u8]) -> Result<()> {
    let folder_id = self.folder_id().await?;
    let Some(id) = self.doc_id_for(&folder_id, key).await? else {
        // New doc — no ink yet; create via the standard put path.
        return self
            .client
            .put(rm_cloud::DocFiles::new_pdf(key, &folder_id, pdf.to_vec()))
            .await
            .map_err(|e| Error::Transport(format!("rm-cloud put {key}: {e}")));
    };
    // Existing doc: pull current files, swap the PDF, strip every .rm ink blob,
    // and put the whole bundle back. The device re-derives the page list from
    // the new PDF on next open (per rm-cloud DocFiles::new_pdf docs).
    let mut files = self
        .client
        .get(&id)
        .await
        .map_err(|e| Error::Transport(format!("rm-cloud get {id}: {e}")))?;
    let pdf_name = format!("{id}.pdf");
    let pdf_slot = files
        .files
        .iter_mut()
        .find(|(n, _)| *n == pdf_name)
        .ok_or_else(|| Error::Transport(format!("doc {id} missing .pdf blob")))?;
    pdf_slot.1 = pdf.to_vec();
    files.files.retain(|(n, _)| !n.ends_with(".rm"));
    self.client
        .put(files)
        .await
        .map_err(|e| Error::Transport(format!("rm-cloud put_replace_ink {key}: {e}")))
}
```

If `rm_cloud::DocFiles` is not re-exported from `rm_cloud`, fix the import path to wherever the test file imports it from in the existing transport.rs.

- [ ] **Step 4: Write the failing test `crates/inkapp-core/tests/sync_replace_ink.rs`**

Mirror the existing FakeTransport pattern from `crates/inkapp-core/src/sync.rs`'s `#[cfg(test)]` block. If FakeTransport already has a counter for `push` calls, extend it with a counter for `push_replace_ink`. Otherwise:

```rust
//! sync_once uses push_replace_ink for the post-fold push; publish does not.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use inkapp_core::connector::ConnectorSet;
use inkapp_core::document::{Document, Documents};
use inkapp_core::error::Result;
use inkapp_core::flow;
use inkapp_core::ink::Stroke;
use inkapp_core::components::notice::Notice;
use inkapp_core::runtime::{app, App, DocSet};
use inkapp_core::sync::{publish, sync_once, DeviceTransport};
use inkapp_core::theme::Theme;

#[derive(Default)]
struct CountingTransport {
    pushes: Mutex<Vec<String>>,
    replace_pushes: Mutex<Vec<String>>,
    canned_ink: Mutex<HashMap<String, Vec<Vec<Stroke>>>>,
}

#[async_trait]
impl DeviceTransport for CountingTransport {
    async fn push(&self, key: &str, _pdf: &[u8]) -> Result<()> {
        self.pushes.lock().unwrap().push(key.into());
        Ok(())
    }
    async fn push_replace_ink(&self, key: &str, _pdf: &[u8]) -> Result<()> {
        self.replace_pushes.lock().unwrap().push(key.into());
        Ok(())
    }
    async fn delete(&self, _key: &str) {}
    async fn pull(&self, _page_h_by_key: &HashMap<String, f64>) -> HashMap<String, Vec<Vec<Stroke>>> {
        self.canned_ink.lock().unwrap().clone()
    }
}

struct M;
#[derive(Clone, Debug)] enum Msg { Noop }
struct Cx;
impl ConnectorSet for Cx { fn connectors(&self) -> Vec<std::sync::Arc<dyn inkapp_core::connector::Connector>> { vec![] } }
fn update(_m: Msg, _: &mut M, _: &Cx) {}
fn view(_m: &M, _: &Cx) -> Documents<Msg> {
    Documents(vec![Document::keyed("d", flow![Notice::line("hi")])])
}

#[tokio::test]
async fn publish_calls_push_not_replace() {
    let mut a: App<M, Msg, Cx> = app(M).connector(Cx).update(update).view(view)
        .key(inkapp_core::crypto::Key::from_bytes([0u8; 32])).theme(Theme::reader()).build();
    let mut set = DocSet::default();
    let t = CountingTransport::default();
    publish(&mut a, &mut set, &t).await.unwrap();
    assert_eq!(t.pushes.lock().unwrap().as_slice(), &["d".to_string()]);
    assert!(t.replace_pushes.lock().unwrap().is_empty(), "publish must not replace ink");
}

#[tokio::test]
async fn sync_once_post_fold_uses_replace_ink() {
    let mut a: App<M, Msg, Cx> = app(M).connector(Cx).update(update).view(view)
        .key(inkapp_core::crypto::Key::from_bytes([0u8; 32])).theme(Theme::reader()).build();
    let mut set = DocSet::default();
    let t = CountingTransport::default();
    sync_once(&mut a, &mut set, &t).await.unwrap();
    // sync_once internally renders the set THEN pushes via replace_ink for any
    // rendered docs. The initial publish that sync_once does (if any) uses push.
    assert!(!t.replace_pushes.lock().unwrap().is_empty(),
        "sync_once must use push_replace_ink for the post-fold push");
}
```

(If `App` builder method names like `theme(...)` are slightly different, mirror an existing test in `inkapp-core/src/sync.rs`'s `#[cfg(test)]` block.)

- [ ] **Step 5: Run tests to verify they fail then pass**

Run: `nix develop -c cargo test -p inkapp-core --test sync_replace_ink`
Expected: FAIL until trait + sync_once switch land, then PASS.

Then run the broader test to confirm no regression:

Run: `nix develop -c cargo test --workspace`
Expected: all green.

- [ ] **Step 6: Mark task 4 complete in `.tasks.json` and commit**

```bash
git add crates/inkapp-core/src/sync.rs \
        crates/rm-device/src/transport.rs \
        crates/inkapp-core/tests/sync_replace_ink.rs \
        docs/superpowers/plans/2026-05-26-reader-app.md.tasks.json
git commit -m "DeviceTransport::push_replace_ink — sync_once wipes ink post-fold"
```

---

## Task 5: Scaffold `apps/reader` (crate + main, no view yet)

**Goal:** A new app crate that compiles, exposes the same CLI surface as reading-queue, and resolves config — but has an empty `view`. Locks the wiring before composition lands.

**Files:**
- Modify: `Cargo.toml` (workspace) — add `apps/reader` to members
- Create: `apps/reader/Cargo.toml`
- Create: `apps/reader/src/lib.rs`
- Create: `apps/reader/src/main.rs`
- Create: `apps/reader/tests/config.rs`

**Acceptance Criteria:**
- [ ] `cargo build -p reader` succeeds inside `nix develop`.
- [ ] `cargo run -p reader -- config path` prints the config path.
- [ ] `cargo run -p reader -- doctor` exits 0 on a populated SecretStore + config (same as reading-queue).
- [ ] `AppConfig` defaults to `device_folder = "/Reader"` and `readwise = ConnectorRef{kind:"readwise", instance:"main"}`.
- [ ] Test `apps/reader/tests/config.rs` resolves AppConfig + DeviceConfig + PageConfig from a temp config file.

**Verify:** `nix develop -c cargo test -p reader` → passing; `nix develop -c cargo build -p reader` clean.

**Steps:**

- [ ] **Step 1: Add to workspace `Cargo.toml`**

```toml
# In the [workspace] members list, add (alphabetical with siblings):
"apps/reader",
```

- [ ] **Step 2: `apps/reader/Cargo.toml`** (copy reading-queue's exactly, change name + description):

```toml
[package]
name = "reader"
version = "0.1.0"
edition = "2021"
license = "MIT"
description = "inkapp Reader app: Library + Feed PDFs with per-page ActionBand"

[dependencies]
inkapp = { path = "../../crates/inkapp" }
inkapp-config = { path = "../../crates/inkapp-config" }
inkapp-content = { path = "../../crates/inkapp-content" }
inkapp-core = { path = "../../crates/inkapp-core" }
inkapp-readwise-reader = { path = "../../crates/inkapp-readwise-reader" }
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "signal"] }

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Copy `apps/reading-queue/src/lib.rs` to `apps/reader/src/lib.rs`** and modify minimally:

```rust
//! The Reader app — Library.pdf + Feed.pdf with a per-page ActionBand.
//! For v1 the `view` returns an empty Documents set; task 6 wires the full
//! composition. This task scaffolds the crate + CLI only.

use std::sync::Arc;

use inkapp::{Document, Documents};
use inkapp_core::connector::{Connector, ConnectorSet};
use inkapp_readwise_reader::{ArticleId, Location, Readwise};

pub use inkapp_core::components::checkbox::Checkbox;

pub struct App;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    Highlighted { article: ArticleId, text: String },
    Move        { article: ArticleId, to: Location },
    Delete      { article: ArticleId },
}

#[derive(Debug, Clone, serde::Deserialize, inkapp_config::Config)]
#[serde(default)]
#[config(kind = "reader", namespace = "app")]
pub struct AppConfig {
    #[config(default = String::from("/Reader"))]
    pub device_folder: String,

    #[config(default = inkapp_config::ConnectorRef { kind: "readwise".into(), instance: "main".into() })]
    pub readwise: inkapp_config::ConnectorRef,
}

pub struct Connectors { pub readwise: Arc<Readwise> }

impl Connectors {
    pub fn fake() -> Self { Connectors { readwise: Arc::new(Readwise::fake()) } }

    pub async fn from_config(
        store: &inkapp_config::ConfigStore,
        app: &AppConfig,
        secrets: &inkapp_core::secrets::SecretStore,
        cache_dir: std::path::PathBuf,
    ) -> Result<Self, inkapp_config::ConfigError> {
        use inkapp_config::Namespace;
        let rw = &app.readwise;
        store.require_instance(Namespace::Connector, &rw.kind, &rw.instance)?;
        let cfg: inkapp_readwise_reader::ReaderConfig = store.resolve(&rw.instance)?;
        let conn = Readwise::from_config(cfg, secrets, cache_dir)
            .await
            .map_err(|e| inkapp_config::ConfigError::Connector(e.to_string()))?;
        Ok(Connectors { readwise: Arc::new(conn) })
    }
}

impl ConnectorSet for Connectors {
    fn connectors(&self) -> Vec<Arc<dyn Connector>> { vec![self.readwise.clone()] }
}

pub fn update(msg: Msg, _m: &mut App, cx: &Connectors) {
    match msg {
        Msg::Highlighted { article, text } => cx.readwise.add_highlight(&article, &text),
        Msg::Move        { article, to }   => cx.readwise.move_to(&article, to),
        Msg::Delete      { article }       => cx.readwise.delete(&article),
    }
}

/// Stub view — task 6 replaces this with the full Library + Feed composition.
pub fn view(_m: &App, _cx: &Connectors) -> Documents<Msg> {
    Documents(Vec::new())
}
```

- [ ] **Step 4: `apps/reader/src/main.rs` is a copy of `apps/reading-queue/src/main.rs`** with the binary/instance label `reader` substituted everywhere `reading-queue` appears. Use the existing reading-queue file as the template — `cp apps/reading-queue/src/main.rs apps/reader/src/main.rs && sed -i 's/reading-queue/reader/g; s/Reading-Queue/Reader/g; s/reading_queue/reader/g' apps/reader/src/main.rs` then read the result and fix any wrong substitutions. The `build_app` and `run_doctor` helpers come along unchanged; the CLI struct gains nothing new this task.

- [ ] **Step 5: Write `apps/reader/tests/config.rs`**

```rust
//! Reader AppConfig + DeviceConfig + PageConfig resolve from a config file.

use inkapp_config::ConfigStore;
use reader::AppConfig;
use std::fs::write;

const SAMPLE: &str = r#"
[device]
backend = "remarkable"
sync_interval_secs = 30

[page]
width = 420.0
height = 560.0
margin = 16.0

[connector.readwise.main]
token = "readwise"
library_locations = ["new"]
library_max = 10
feed_enabled = false
feed_max = 100

[app.reader.default]
device_folder = "/Reader"
readwise = "readwise.main"
"#;

#[test]
fn config_resolves_with_reader_section() {
    let td = tempfile::tempdir().unwrap();
    let path = td.path().join("config.toml");
    write(&path, SAMPLE).unwrap();
    let store = ConfigStore::open(&path).unwrap();
    let cfg: AppConfig = store.resolve("default").unwrap();
    assert_eq!(cfg.device_folder, "/Reader");
    assert_eq!(cfg.readwise.kind, "readwise");
    assert_eq!(cfg.readwise.instance, "main");
}

#[test]
fn config_uses_defaults_when_section_omitted() {
    // Without [app.reader.<instance>], resolve falls back to derive-default values.
    let td = tempfile::tempdir().unwrap();
    let path = td.path().join("config.toml");
    write(&path, r#"
[device]
backend = "remarkable"
sync_interval_secs = 30

[page]
width = 420.0
height = 560.0
margin = 16.0

[connector.readwise.main]
token = "readwise"
"#).unwrap();
    let store = ConfigStore::open(&path).unwrap();
    let cfg: AppConfig = store.resolve("default").unwrap_or_default();
    assert_eq!(cfg.device_folder, "/Reader");
}
```

- [ ] **Step 6: Build and test**

Run: `nix develop -c cargo build -p reader`
Expected: clean build.

Run: `nix develop -c cargo test -p reader --test config`
Expected: 2 passing.

- [ ] **Step 7: Mark task 5 complete in `.tasks.json` and commit**

```bash
git add Cargo.toml \
        apps/reader/Cargo.toml apps/reader/src/lib.rs apps/reader/src/main.rs \
        apps/reader/tests/config.rs \
        docs/superpowers/plans/2026-05-26-reader-app.md.tasks.json
git commit -m "reader: scaffold app crate (CLI + AppConfig + stub view)"
```

(Do NOT stage Cargo.lock; it'll regenerate as a follow-on commit per workspace convention.)

---

## Task 6: Reader `view` — Library + Feed Documents with full composition

**Goal:** Replace the stub `view` with the spec's full composition: Index page + per-article (Heading + Article body) inside `Section`s, with an `ActionBand` page header for each Document. Library and Feed produced from the connector.

**Files:**
- Modify: `apps/reader/src/lib.rs`
- Create: `apps/reader/tests/app.rs`
- Create: `apps/reader/tests/shared.rs` (small helper for building a fake-cassette App)

**Acceptance Criteria:**
- [ ] `view` against a cassette with Library articles only → one `Document::keyed("library", ...)`.
- [ ] `view` against a cassette with both Library and Feed → two Documents in order (library first).
- [ ] Each Document's `page_header` is set to an `ActionBand` with the four labelled cells.
- [ ] Each Document's flow opens with an `Index` listing every article in the collection, then one `Section` per article whose body is `flow![Heading::for_article(a), inkapp_content::Article::new(...)]`.
- [ ] When `readwise.failed_writes()` is non-empty, a third `Document::keyed("_banner", ...)` is prepended with a `Notice::line(...)` summary.
- [ ] An end-to-end render test compiles the Library Document via the framework and recovers ≥1 `tok-*` region per article (the Article body) AND ≥1 `action-*` region per page (the ActionBand).

**Verify:** `nix develop -c cargo test -p reader` → all passing.

**Steps:**

- [ ] **Step 1: Replace `view` (and add helpers) in `apps/reader/src/lib.rs`**

```rust
use inkapp::{flow, Document, Documents};
use inkapp_core::components::action_band::ActionBand;
use inkapp_core::components::heading::Heading;
use inkapp_core::components::index::{Index, IndexEntry};
use inkapp_core::components::notice::Notice;
use inkapp_core::components::section::Section;
use inkapp_content::Article;
use inkapp_readwise_reader::{Article as ApiArticle, Location};

fn heading_for(a: &ApiArticle) -> Heading {
    let byline = if !a.author.is_empty() { a.author.clone() } else { a.site_name.clone() };
    let mut h = Heading::new(a.title.clone());
    if !byline.is_empty() { h = h.byline(byline); }
    if let Some(rt) = a.reading_time.clone().filter(|s| !s.is_empty()) {
        h = h.reading_time(rt);
    }
    h
}

fn article_body(a: &ApiArticle) -> Article<Msg> {
    let id = a.id.clone();
    Article::new(
        a.html_content.as_deref().unwrap_or(""),
        &a.highlights,
        move |s| Msg::Highlighted { article: id.clone(), text: s.to_string() },
    )
}

fn action_band_msg() -> ActionBand<Msg> {
    ActionBand::new([
        ("Inbox".into(),
            Box::new(|id: &str| Msg::Move { article: ArticleId::new(id), to: Location::New })
                as Box<dyn Fn(&str) -> Msg + Send + Sync>),
        ("Archive".into(),
            Box::new(|id: &str| Msg::Move { article: ArticleId::new(id), to: Location::Archive })),
        ("Later".into(),
            Box::new(|id: &str| Msg::Move { article: ArticleId::new(id), to: Location::Later })),
        ("Delete".into(),
            Box::new(|id: &str| Msg::Delete { article: ArticleId::new(id) })),
    ])
}

fn collection_doc(key: &str, articles: Vec<ApiArticle>) -> Option<Document<Msg>> {
    if articles.is_empty() { return None; }
    let entries: Vec<IndexEntry> = articles.iter().map(IndexEntry::from).collect();
    let mut items: Vec<Box<dyn inkapp_core::component::Component<Msg = Msg>>> =
        vec![Box::new(Index::<Msg>::new(entries))];
    for a in &articles {
        let body: Vec<Box<dyn inkapp_core::component::Component<Msg = Msg>>> = vec![
            Box::new(heading_for(a)),
            Box::new(article_body(a)),
        ];
        items.push(Box::new(Section::<Msg>::new(&a.id.0, body)));
    }
    Some(Document::keyed(key, items).page_header(action_band_msg()))
}

pub fn view(_m: &App, cx: &Connectors) -> Documents<Msg> {
    let mut docs: Vec<Document<Msg>> = Vec::new();

    let failed = cx.readwise.failed_writes();
    if !failed.is_empty() {
        docs.push(Document::keyed(
            "_banner",
            flow![Notice::line(&format!("couldn't sync {} change(s) to Readwise", failed.len()))],
        ));
    }

    if let Some(d) = collection_doc("Library", cx.readwise.library()) { docs.push(d); }
    if let Some(d) = collection_doc("Feed",    cx.readwise.feed())    { docs.push(d); }

    Documents(docs)
}
```

(Note the doc keys are `"Library"` and `"Feed"` — capitalised — so the on-device `visibleName` is pretty without needing the optional `Document::visible_name` field. This is the cheaper of the two routes called out in the spec's open questions.)

- [ ] **Step 2: Note: `Index<Msg>::new` — fix the cast if needed**

Index's signature is `Index<M = ()>`. Passing it through a `Box<dyn Component<Msg = Msg>>` may require explicit `Index::<Msg>::new(entries)` (or building via an intermediate). If the build complains about `()` vs `Msg`, follow the type errors and adjust.

- [ ] **Step 3: Write `apps/reader/tests/shared.rs`** (a helper, not a `#[test]`):

```rust
//! Shared helpers for reader tests. Construct an App<reader::App, reader::Msg, reader::Connectors>
//! over the Readwise::fake() cassette so view/update can be exercised without I/O.

use inkapp::app;
use inkapp_core::crypto::Key;
use inkapp_core::runtime::App;
use inkapp_core::theme::Theme;
use reader::{update, view, App as Model, Connectors, Msg};

pub fn fake_app() -> App<Model, Msg, Connectors> {
    app(Model)
        .connector(Connectors::fake())
        .update(update)
        .view(view)
        .key(Key::from_bytes([0u8; 32]))
        .theme(Theme::reader())
        .build()
}
```

- [ ] **Step 4: Write `apps/reader/tests/app.rs`**

```rust
//! Reader view composition: emits Library + Feed Documents, page-header is an
//! ActionBand, each article has a Section with Heading + body, recovery yields
//! both article token regions AND action regions.

mod shared;

use inkapp_core::components::index::IndexEntry;
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::compile_to_document_with_sources;
use inkapp_core::runtime::{collect_typst_sources, document_source_in};
use inkapp_core::geometry::PageGeom;
use inkapp_core::theme::Theme;

#[tokio::test]
async fn view_yields_library_and_feed_when_both_present() {
    let mut a = shared::fake_app();
    let mut set = inkapp_core::runtime::DocSet::default();
    let rendered = a.render(&mut set).await.unwrap();
    let keys: Vec<&str> = rendered.iter().map(|r| r.key.0.as_str()).collect();
    // The Readwise::fake() cassette has Library content; Feed depends on the cassette
    // layout. Assert at least Library is present and ordering is Library-first.
    assert!(keys.first().map_or(false, |k| *k == "Library" || *k == "_banner"),
        "first doc should be Library or banner; got {keys:?}");
    assert!(keys.iter().any(|k| *k == "Library"), "Library doc present: {keys:?}");
}

#[test]
fn library_compiles_and_recovers_action_plus_token_regions() {
    use reader::Connectors;
    let cx = Connectors::fake();
    let docs = reader::view(&reader::App, &cx);
    let library = docs.0.iter().find(|d| d.key.0 == "Library").expect("Library doc");
    let geom = PageGeom { w: 420.0, h: 560.0, margin: 16.0 };
    let src = document_source_in(library, geom, &Theme::reader());
    let sources = collect_typst_sources(library);
    let compiled = compile_to_document_with_sources(&src, &sources).unwrap();
    let manifest = recover_regions(&compiled).unwrap();
    let names: Vec<&str> = manifest.regions.iter().map(|r| r.name.as_str()).collect();
    assert!(names.iter().any(|n| n.starts_with("tok-")), "≥1 token region: {names:?}");
    assert!(names.iter().any(|n| n.starts_with("action-")), "≥1 action region: {names:?}");
}
```

- [ ] **Step 5: Run tests**

Run: `nix develop -c cargo test -p reader`
Expected: tests pass. If a fake-cassette field is renamed or absent, adjust the test assertions to whatever fields `Readwise::fake()` actually populates — the goal is "Library exists, action regions appear, token regions appear." 

- [ ] **Step 6: Mark task 6 complete in `.tasks.json` and commit**

```bash
git add apps/reader/src/lib.rs \
        apps/reader/tests/app.rs apps/reader/tests/shared.rs \
        docs/superpowers/plans/2026-05-26-reader-app.md.tasks.json
git commit -m "reader: full view — Library + Feed Documents with per-page ActionBand"
```

---

## Task 7: appdx + final docs

**Goal:** Record what landed. Per the project convention, `appdx.md` is the definition of done.

**Files:**
- Modify: `docs/appdx.md`

**Acceptance Criteria:**
- [ ] `docs/appdx.md` contains a paragraph noting: `Heading`, `Section<M>`, `ActionBand<M>`, `Document::page_header`, and `DeviceTransport::push_replace_ink` are all built.
- [ ] A short "Reader app" paragraph (or section) names `apps/reader` as the proof-point reading app composing all of the above, and reads as a peer to the existing reading-queue mention.
- [ ] The status header (the block-quote at the top) is updated if you grew the build-order spine.

**Verify:** `grep -E "Heading|Section<M>|ActionBand|page_header|push_replace_ink|apps/reader" docs/appdx.md` produces hits for each term.

**Steps:**

- [ ] **Step 1: Open `docs/appdx.md`** and find the status block-quote at the top. Add the five new primitives and the reader app in the same style as the existing "Beyond the spine" prose.

- [ ] **Step 2: Verify the grep**

```bash
for term in Heading "Section<M>" "ActionBand" "page_header" "push_replace_ink" "apps/reader"; do
  echo "--- $term ---"
  grep -F "$term" docs/appdx.md | head -2
done
```

Each term should produce at least one matching line.

- [ ] **Step 3: Mark task 7 complete in `.tasks.json` and commit**

```bash
git add docs/appdx.md docs/superpowers/plans/2026-05-26-reader-app.md.tasks.json
git commit -m "appdx: record Heading, Section, ActionBand, page_header, push_replace_ink, apps/reader"
```

---

## Self-review checklist (run after implementation completes)

- Each of the spec's five framework additions has a corresponding task that creates and tests it (tasks 0, 1, 2, 3, 4).
- The app's `view` matches the spec's worked example end to end (task 6).
- The reading-queue worked example is untouched (the spec says it stays).
- No task references types or methods that aren't defined in an earlier task: `Heading` (T0) → used in T6; `Section` (T1) → used in T3 tests + T6; `Document::page_header` (T2) → used in T3 tests + T6; `ActionBand` (T3) → used in T6; `push_replace_ink` (T4) → used implicitly via `sync_once` from T6's runtime.
- No placeholders / TBDs / "implement appropriately" — every step ships exact code or an exact command.
- The mid-cycle ink race (spec's "Open question") is documented; v1 keeps the simpler replace-ink behaviour.

## What this delivers when complete

- Two reusable framework primitives (`Heading`, `Section<M>`), one big one (`ActionBand<M>`), one structural addition (`Document::page_header`), and one transport semantic (`push_replace_ink`).
- An `apps/reader` that, run end-to-end with a real reMarkable, produces `Library.pdf` and `Feed.pdf` with a four-cell per-page ActionBand and per-article token-region highlights. The serial on-device manual-verification step from the spec's testing strategy becomes runnable.
- `~/git/rmreader` (fulgur) is now historically interesting but no longer the reader.
