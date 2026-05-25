# Index Component + Device-Neutral Theme Seam — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a reusable device-blind `Index` Display component for the reader's Library/Feed contents pages, plus the minimal `Theme` palette seam it reads so rendering is device-optimal (color on Paper Pro, grayscale elsewhere).

**Architecture:** A `Theme` is a struct of semantic color *roles* (Typst fill-expression strings), threaded through the render driver into `RenderCx` exactly as `PageGeom` is threaded today (the "page-blind" precedent). Components name roles, never literal colors or a device. `Index` renders each entry as a non-breakable `#region` box (entries never split; the list paginates between them) and `decode`s nothing. The `Article → IndexEntry` leaf conversion lives in the Readwise crate so `inkapp-core` stays connector-blind.

**Tech Stack:** Rust workspace; Typst-as-a-library render; `nix develop` dev shell. Build/test via `nix develop -c cargo test --workspace`.

**Spec:** `docs/superpowers/specs/2026-05-25-index-component-and-theme-seam-design.md`

**Working context:** worktree on branch `articles`. Do **not** stage `Cargo.lock`. Run `nix develop -c cargo fmt` before each commit (the pre-commit hook runs `cargo fmt --check` and also blocks while native tasks are open — close the task before committing). No `Co-Authored-By` lines in commit messages.

---

### Task 0: `Theme` type

**Goal:** A device-neutral palette of semantic roles with a grayscale default and the Paper Pro color palette.

**Files:**
- Create: `crates/inkapp-core/src/theme.rs`
- Modify: `crates/inkapp-core/src/lib.rs` (add `pub mod theme;` and `pub use theme::Theme;`)

**Acceptance Criteria:**
- [ ] `Theme::grayscale()` has `paper == None`; `Theme::default()` equals `grayscale()`.
- [ ] `Theme::indigo_tomato()` carries the rmreader Paper Pro colors and `paper == Some(...)`.
- [ ] `Theme` is `Debug + Clone + PartialEq`.

**Verify:** `nix develop -c cargo test -p inkapp-core theme::` → all pass.

**Steps:**

- [ ] **Step 1: Write `theme.rs` with the type, constructors, and tests**

Create `crates/inkapp-core/src/theme.rs`:

```rust
//! `Theme` — the device-blind palette seam. Components name semantic *roles*
//! (heading, byline, muted, …); the framework fills them with Typst
//! fill-expression strings tuned for the target device. Threaded through
//! `RenderCx` exactly as `PageGeom` is threaded for "page-blind" layout, so a
//! component never names a literal color or knows which device it renders for.

/// A palette of semantic roles. Each field is a Typst color *expression* string
/// interpolated into `#text(fill: …)` — e.g. `"black"`, `"luma(45%)"`,
/// `"rgb(\"#2A2F6B\")"`. `paper` is the optional page fill: `None` leaves Typst's
/// default white, so existing renders are byte-identical under the default theme.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub ink: String,
    pub heading: String,
    pub byline: String,
    pub muted: String,
    pub rule: String,
    pub paper: Option<String>,
}

impl Theme {
    /// The safe default: grayscale lumas for any e-ink device.
    pub fn grayscale() -> Self {
        Self {
            ink: "luma(10%)".into(),
            heading: "black".into(),
            byline: "luma(35%)".into(),
            muted: "luma(45%)".into(),
            rule: "luma(80%)".into(),
            paper: None,
        }
    }

    /// reMarkable Paper Pro color palette ("Indigo + Tomato"). On Paper Pro
    /// e-ink, blues/reds hold their color while ambers/greens wash out, so the
    /// chrome is indigo headings + rust bylines on warm paper (proven in rmreader).
    pub fn indigo_tomato() -> Self {
        Self {
            ink: "rgb(\"#1A1A18\")".into(),
            heading: "rgb(\"#2A2F6B\")".into(),
            byline: "rgb(\"#9C3A1B\")".into(),
            muted: "rgb(\"#5E6166\")".into(),
            rule: "rgb(\"#E0DDD2\")".into(),
            paper: Some("rgb(\"#F3F1EA\")".into()),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::grayscale()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grayscale_has_no_paper_fill() {
        assert_eq!(Theme::grayscale().paper, None);
    }

    #[test]
    fn default_is_grayscale() {
        assert_eq!(Theme::default(), Theme::grayscale());
    }

    #[test]
    fn indigo_tomato_is_color_with_paper() {
        let t = Theme::indigo_tomato();
        assert_eq!(t.heading, "rgb(\"#2A2F6B\")");
        assert_eq!(t.byline, "rgb(\"#9C3A1B\")");
        assert_eq!(t.paper, Some("rgb(\"#F3F1EA\")".into()));
    }
}
```

- [ ] **Step 2: Register the module and re-export `Theme`**

In `crates/inkapp-core/src/lib.rs`, add the module declaration alphabetically near the others (after `pub mod sync;` / before `pub mod world;` is fine):

```rust
pub mod theme;
```

And add the re-export next to the other `pub use` lines (e.g. after `pub use sync::DeviceTransport;`):

```rust
pub use theme::Theme;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `nix develop -c cargo test -p inkapp-core theme::`
Expected: 3 tests pass.

- [ ] **Step 4: fmt + commit**

```bash
nix develop -c cargo fmt
git add crates/inkapp-core/src/theme.rs crates/inkapp-core/src/lib.rs
git commit -m "inkapp-core: Theme — device-neutral semantic palette (grayscale + Paper Pro)"
```

---

### Task 1: Thread the theme through the render driver

**Goal:** Carry a `Theme` from the `App` (or a `PageGeom`-compatible env) into `RenderCx` so components can read it, with zero change to existing renders under the default grayscale theme.

**Files:**
- Modify: `crates/inkapp-core/src/component.rs` (add `theme` to `RenderCx`, add `with_theme`)
- Modify: `crates/inkapp-core/src/runtime.rs` (add `RenderEnv`; change geom params to `impl Into<RenderEnv>`; apply page fill; `App` gains a `theme` field + builder)
- Modify: `crates/inkapp-core/src/lib.rs` (re-export `RenderEnv`)
- Create: `crates/inkapp-core/tests/theme_threading.rs` (page-fill behavior test)

**Acceptance Criteria:**
- [ ] `RenderCx::new(p)` defaults `theme` to grayscale; `RenderCx::new(p).with_theme(t)` sets it.
- [ ] `document_source_in` emits `fill: <paper>` in `#set page(...)` iff `theme.paper` is `Some`.
- [ ] All existing call sites that pass a `PageGeom` still compile (via `From<PageGeom> for RenderEnv`).
- [ ] `App` has a `.theme(Theme)` builder defaulting to grayscale; its render path passes the theme down.
- [ ] Whole workspace still green (no golden drift).

**Verify:** `nix develop -c cargo test --workspace` → all pass (no golden changes).

**Steps:**

- [ ] **Step 1: Add `theme` to `RenderCx`**

In `crates/inkapp-core/src/component.rs`, add the import and field, and a builder. Replace the `RenderCx` struct + impl head:

```rust
use crate::theme::Theme;

/// Render-time context: supplies the current page index, a monotonically
/// increasing id so components can mint unique region names, and the device
/// palette (`theme`) so components emit semantic-role colors, never literals.
#[derive(Debug, Default)]
pub struct RenderCx {
    pub page: usize,
    next_id: u64,
    pub theme: Theme,
}

impl RenderCx {
    pub fn new(page: usize) -> Self {
        Self {
            page,
            next_id: 0,
            theme: Theme::default(),
        }
    }

    /// Builder: set the palette this render uses (default is grayscale).
    #[must_use]
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Mint a fresh per-render id (used by components that subdivide into
    /// programmatically-named regions).
    #[must_use]
    pub fn fresh_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}
```

- [ ] **Step 2: Add `RenderEnv` and thread it through the render fns**

In `crates/inkapp-core/src/runtime.rs`, add near the top imports:

```rust
use crate::theme::Theme;
```

Add the `RenderEnv` type (place it just above `document_source` / `document_source_in`):

```rust
/// The render-time environment threaded into a document render: page geometry +
/// the device palette. `From<PageGeom>` lets existing geom-only call sites pass a
/// `PageGeom` unchanged (theme then defaults to grayscale).
#[derive(Debug, Clone, Default)]
pub struct RenderEnv {
    pub geom: PageGeom,
    pub theme: Theme,
}

impl From<PageGeom> for RenderEnv {
    fn from(geom: PageGeom) -> Self {
        Self {
            geom,
            theme: Theme::default(),
        }
    }
}
```

Replace `document_source_in` with the env-taking, theme-applying version:

```rust
/// Assemble a document's Typst source for a render environment: `#import` lines
/// for the prelude and authored sources, the `#set page` from `env.geom` (with a
/// `fill:` only when the theme sets `paper`), then each component's render in flow
/// order with the theme available on the `RenderCx`.
pub fn document_source_in<M>(doc: &Document<M>, env: impl Into<RenderEnv>) -> String {
    let env = env.into();
    let mut cx = RenderCx::new(0).with_theme(env.theme.clone());
    let mut src = String::new();
    for (path, _) in collect_typst_sources(doc) {
        src.push_str(&format!("#import \"{path}\": *\n"));
    }
    let fill = match &env.theme.paper {
        Some(p) => format!(", fill: {p}"),
        None => String::new(),
    };
    src.push_str(&format!(
        "#set page(width: {}pt, height: {}pt, margin: {}pt{})\n#set text(size: 12pt)\n",
        env.geom.w, env.geom.h, env.geom.margin, fill
    ));
    for c in &doc.flow {
        src.push_str(&c.render(&mut cx));
    }
    src
}
```

Change the four other geom-taking fns to take `env: impl Into<RenderEnv>` and pass it through. `compile_document_in`:

```rust
pub fn compile_document_in<M>(
    doc: &Document<M>,
    env: impl Into<RenderEnv>,
) -> Result<typst::layout::PagedDocument> {
    compile_document_in_with_assets(doc, env, &AssetMap::new())
}
```

`compile_document_in_with_assets` — change the `geom: PageGeom` param to `env: impl Into<RenderEnv>` and pass `env` into `document_source_in` (the rest of the body is unchanged):

```rust
pub fn compile_document_in_with_assets<M>(
    doc: &Document<M>,
    env: impl Into<RenderEnv>,
    assets: &AssetMap,
) -> Result<typst::layout::PagedDocument> {
    let src = document_source_in(doc, env);
    let sources = collect_typst_sources(doc);
    let asset_vec = assets_as_slice(assets);
    crate::render::compile_to_document_with_sources_and_assets(&src, &sources, &asset_vec)
}
```

`render_document_in`:

```rust
pub fn render_document_in<M>(
    doc: &Document<M>,
    version: u64,
    key: &Key,
    env: impl Into<RenderEnv>,
) -> Result<RenderedDoc> {
    render_document_in_with_assets(doc, version, key, env, &AssetMap::new())
}
```

`render_document_in_with_assets` — change the param, and capture the geom height *before* moving `env` into `document_source_in` (it is used in the `page_h` fallback):

```rust
pub fn render_document_in_with_assets<M>(
    doc: &Document<M>,
    version: u64,
    key: &Key,
    env: impl Into<RenderEnv>,
    assets: &AssetMap,
) -> Result<RenderedDoc> {
    let env = env.into();
    let geom_h = env.geom.h;
    let src = document_source_in(doc, env);
    let sources = collect_typst_sources(doc);
    let asset_vec = assets_as_slice(assets);
    let compiled =
        crate::render::compile_to_document_with_sources_and_assets(&src, &sources, &asset_vec)?;
    let page_h = compiled
        .pages
        .first()
        .map(|p| p.frame.height().to_pt())
        .unwrap_or(geom_h);
    // ... the remainder of the existing body is unchanged ...
```

> The default-geom wrappers (`document_source`, `compile_document`, `render_document`) keep passing `PageGeom::default()` — it now converts via `From<PageGeom>`. Leave them unchanged.

- [ ] **Step 3: Give `App` a theme**

In `crates/inkapp-core/src/runtime.rs`:

Add the field to the `App` struct (after `geom: PageGeom,`):

```rust
    geom: PageGeom,
    theme: Theme,
```

Add the `App::new` parameter (after `geom: PageGeom,`) and set the field:

```rust
        geom: PageGeom,
        theme: Theme,
        fetcher: Arc<dyn ImageFetcher>,
        asset_cache: Option<Arc<Cache>>,
    ) -> Self {
        Self {
            model,
            connectors,
            update,
            view,
            version: 1,
            key,
            geom,
            theme,
            fetcher,
            asset_cache,
        }
    }
```

In `App::render`, build a `RenderEnv` for the call (replace the `self.geom` argument):

```rust
            let rd = render_document_in_with_assets(
                doc,
                self.version,
                &self.key,
                RenderEnv {
                    geom: self.geom,
                    theme: self.theme.clone(),
                },
                &assets,
            )?;
```

In `App::step`, do the same for its `render_document_in_with_assets` call:

```rust
            next_rendered.push(render_document_in_with_assets(
                doc,
                self.version,
                &self.key,
                RenderEnv {
                    geom: self.geom,
                    theme: self.theme.clone(),
                },
                &assets,
            )?);
```

Add the `theme` field to `BuilderReady` (after `geom: PageGeom,`):

```rust
    geom: PageGeom,
    theme: Theme,
```

Initialize it in `BuilderFull::key` (where `BuilderReady` is constructed, after `geom: PageGeom::default(),`):

```rust
            geom: PageGeom::default(),
            theme: Theme::default(),
```

Add the builder method on `BuilderReady` (next to `page`):

```rust
    /// Override the render palette (default: grayscale). Set by a binary's
    /// bootstrap from the resolved device — app `update`/`view` stay device-blind.
    #[must_use]
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }
```

Pass it in `BuilderReady::build` (insert `self.theme,` after `self.geom,`):

```rust
        App::new(
            self.model,
            self.connectors,
            self.update,
            self.view,
            self.key,
            self.geom,
            self.theme,
            self.fetcher,
            self.asset_cache,
        )
```

- [ ] **Step 4: Re-export `RenderEnv`**

In `crates/inkapp-core/src/lib.rs`, add `RenderEnv` to the `runtime` re-export list:

```rust
pub use runtime::{
    app, collect_typst_sources, compile_document, compile_document_in, document_source,
    document_source_in, render_document, render_document_in, App, Cycle, DocSet, RenderEnv,
    RenderedDoc, REGION_PRELUDE,
};
```

- [ ] **Step 5: Write the page-fill threading test**

Create `crates/inkapp-core/tests/theme_threading.rs`:

```rust
//! The theme threads through `document_source_in` into the `#set page` fill and
//! is available on the `RenderCx` (only emits a page fill when paper is set).

use inkapp_core::document::Document;
use inkapp_core::geometry::PageGeom;
use inkapp_core::runtime::{document_source_in, RenderEnv};
use inkapp_core::theme::Theme;

#[test]
fn grayscale_emits_no_page_fill() {
    let doc: Document<()> = Document::keyed("d", vec![]);
    let src = document_source_in(&doc, RenderEnv::default());
    assert!(
        src.contains("#set page("),
        "page set is present: {src}"
    );
    assert!(!src.contains("fill:"), "no page fill under grayscale: {src}");
}

#[test]
fn color_theme_sets_warm_paper_fill() {
    let doc: Document<()> = Document::keyed("d", vec![]);
    let env = RenderEnv {
        geom: PageGeom::default(),
        theme: Theme::indigo_tomato(),
    };
    let src = document_source_in(&doc, env);
    assert!(
        src.contains("fill: rgb(\"#F3F1EA\")"),
        "warm paper fill applied: {src}"
    );
}

#[test]
fn pagegeom_still_accepted_via_from() {
    // Existing call sites pass a PageGeom directly; it must still compile/run.
    let doc: Document<()> = Document::keyed("d", vec![]);
    let _ = document_source_in(&doc, PageGeom::default());
}
```

- [ ] **Step 6: Run the new test, then the whole workspace**

Run: `nix develop -c cargo test -p inkapp-core --test theme_threading`
Expected: 3 pass.

Run: `nix develop -c cargo test --workspace`
Expected: all pass, **no golden changes** (grayscale default leaves every existing render byte-identical). If a golden test fails, stop — the default theme has perturbed output; fix before continuing.

- [ ] **Step 7: fmt + commit**

```bash
nix develop -c cargo fmt
git add crates/inkapp-core/src/component.rs crates/inkapp-core/src/runtime.rs crates/inkapp-core/src/lib.rs crates/inkapp-core/tests/theme_threading.rs
git commit -m "inkapp-core: thread Theme through RenderCx/RenderEnv and the App"
```

---

### Task 2: `Index` Display component

**Goal:** A device-blind `Index<M = ()>` that renders a clean per-entry listing using theme roles and decodes nothing.

**Files:**
- Create: `crates/inkapp-core/src/components/index.rs`
- Modify: `crates/inkapp-core/src/components/mod.rs` (add `pub mod index;`)
- Modify: `crates/inkapp-core/src/lib.rs` (re-export `Index`, `IndexEntry`)

**Acceptance Criteria:**
- [ ] Renders title in `theme.heading` (bold), meta `byline · reading_time` in `theme.byline`/`theme.muted`, summary in `theme.ink`; omits absent parts.
- [ ] Each entry is a `#region("idx-{i}", […])`; a `theme.rule` hairline separates entries (none after the last).
- [ ] `reading_time` is emitted verbatim; long summaries truncate with `…`.
- [ ] User text is escaped via `esc_typst_str`; `decode` returns `vec![]`.

**Verify:** `nix develop -c cargo test -p inkapp-core index::` → all pass.

**Steps:**

- [ ] **Step 1: Write `index.rs` (component + tests)**

Create `crates/inkapp-core/src/components/index.rs`:

```rust
//! `Index` — a Display-mode component: a typographically clean listing of entries
//! (the reader's Library / Feed contents pages). Generic over the app's `Msg`
//! (which it never emits, like `Notice`); `Index<()>` is the common case. Each
//! entry is a non-breakable `#region` box, so an entry never splits across a page
//! break; the list flows and Typst paginates between entries. Colors come from
//! `cx.theme` (semantic roles), never literals — the component is device-blind.

use std::marker::PhantomData;

use crate::component::{Component, RenderCx};
use crate::components::esc_typst_str;
use crate::ink::RegionInk;
use crate::manifest::Manifest;

/// Summaries longer than this many chars are truncated (with an ellipsis) so a
/// non-breakable entry box stays within one page. Contents pages want short
/// standfirsts anyway.
const DEFAULT_SUMMARY_CHARS: usize = 200;

/// One row of an index listing. Built by an app's `view` from connector data (the
/// "dumb leaf conversion" pattern); e.g. `inkapp-readwise-reader`'s
/// `From<&Article> for IndexEntry`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexEntry {
    pub title: String,
    /// Author, or site name as a fallback; `None` when neither is known.
    pub byline: Option<String>,
    /// Verbatim reading-time label (e.g. "5 min") — never parsed/reformatted.
    pub reading_time: Option<String>,
    pub summary: Option<String>,
}

impl IndexEntry {
    /// An entry with just a title; byline/reading_time/summary default to `None`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }
}

/// A display-only index. `M` is the app message type — `Index` never emits one,
/// so it's a phantom; `Index<()>` works when no surrounding `Msg` is needed.
pub struct Index<M = ()> {
    entries: Vec<IndexEntry>,
    _msg: PhantomData<fn() -> M>,
}

impl<M> Index<M> {
    pub fn new(entries: Vec<IndexEntry>) -> Self {
        Self {
            entries,
            _msg: PhantomData,
        }
    }
}

/// Truncate on a char boundary, appending "…" only when actually shortened.
fn truncate_summary(s: &str) -> String {
    if s.chars().count() <= DEFAULT_SUMMARY_CHARS {
        s.to_string()
    } else {
        let cut: String = s.chars().take(DEFAULT_SUMMARY_CHARS).collect();
        format!("{}…", cut.trim_end())
    }
}

impl<M> Component for Index<M> {
    type Msg = M;

    fn render(&self, cx: &mut RenderCx) -> String {
        let theme = &cx.theme;
        let mut s = String::new();
        for (i, e) in self.entries.iter().enumerate() {
            // Each entry's body: title, an optional meta line, an optional summary.
            // All user text is injected as a Typst string expression (`#"..."`)
            // via esc_typst_str so `[`, `]`, `#` stay literal (the Notice recipe).
            let mut body = String::new();
            body.push_str(&format!(
                "#text(fill: {}, weight: \"bold\", size: 13pt)[#\"{}\"]\n\n",
                theme.heading,
                esc_typst_str(&e.title)
            ));

            let byline = e.byline.as_deref().filter(|b| !b.is_empty());
            let rt = e.reading_time.as_deref().filter(|r| !r.is_empty());
            if byline.is_some() || rt.is_some() {
                if let Some(b) = byline {
                    body.push_str(&format!(
                        "#text(fill: {}, size: 9pt)[#\"{}\"]",
                        theme.byline,
                        esc_typst_str(b)
                    ));
                }
                if let Some(r) = rt {
                    if byline.is_some() {
                        body.push_str(&format!(
                            "#text(fill: {}, size: 9pt)[#\" · \"]",
                            theme.muted
                        ));
                    }
                    body.push_str(&format!(
                        "#text(fill: {}, size: 9pt)[#\"{}\"]",
                        theme.muted,
                        esc_typst_str(r)
                    ));
                }
                body.push_str("\n\n");
            }

            if let Some(sum) = e.summary.as_deref().filter(|s| !s.is_empty()) {
                body.push_str(&format!(
                    "#text(fill: {}, size: 10pt)[#\"{}\"]\n\n",
                    theme.ink,
                    esc_typst_str(&truncate_summary(sum))
                ));
            }

            // The entry as one non-breakable region box (layout/recovery anchor;
            // decode ignores it). `#region(name, body)` is the prelude default.
            s.push_str(&format!("#region(\"idx-{i}\", [{body}])\n"));

            // Hairline between entries (not after the last).
            if i + 1 < self.entries.len() {
                s.push_str(&format!(
                    "#line(length: 100%, stroke: 0.5pt + {})\n\n",
                    theme.rule
                ));
            }
        }
        s
    }

    fn decode(&self, _ink: &[RegionInk], _manifest: &Manifest) -> Vec<M> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    fn entry(title: &str) -> IndexEntry {
        IndexEntry::new(title)
    }

    #[test]
    fn title_uses_heading_role_and_is_present() {
        let idx = Index::<()>::new(vec![entry("Hello World")]);
        let src = idx.render(&mut RenderCx::new(0).with_theme(Theme::indigo_tomato()));
        assert!(src.contains("fill: rgb(\"#2A2F6B\")"), "heading color: {src}");
        assert!(src.contains("#\"Hello World\""), "title text: {src}");
    }

    #[test]
    fn byline_and_reading_time_joined_with_separator() {
        let e = IndexEntry {
            title: "T".into(),
            byline: Some("Ada Lovelace".into()),
            reading_time: Some("5 min".into()),
            summary: None,
        };
        let src = Index::<()>::new(vec![e]).render(&mut RenderCx::new(0));
        assert!(src.contains("#\"Ada Lovelace\""), "byline: {src}");
        assert!(src.contains("#\" · \""), "separator present: {src}");
        assert!(src.contains("#\"5 min\""), "reading_time verbatim: {src}");
    }

    #[test]
    fn missing_meta_is_omitted() {
        let src = Index::<()>::new(vec![entry("Just a title")]).render(&mut RenderCx::new(0));
        assert!(!src.contains("#\" · \""), "no separator without meta: {src}");
    }

    #[test]
    fn reading_time_alone_has_no_separator() {
        let e = IndexEntry {
            title: "T".into(),
            byline: None,
            reading_time: Some("3 min".into()),
            summary: None,
        };
        let src = Index::<()>::new(vec![e]).render(&mut RenderCx::new(0));
        assert!(src.contains("#\"3 min\""), "reading_time present: {src}");
        assert!(!src.contains("#\" · \""), "no separator without byline: {src}");
    }

    #[test]
    fn escapes_quotes_in_title() {
        let src = Index::<()>::new(vec![entry(r#"a "quote""#)]).render(&mut RenderCx::new(0));
        assert!(src.contains(r#"a \"quote\""#), "title escaped: {src}");
    }

    #[test]
    fn emits_a_region_per_entry_with_rule_between() {
        let src = Index::<()>::new(vec![entry("one"), entry("two")]).render(&mut RenderCx::new(0));
        assert!(src.contains("#region(\"idx-0\""), "first region: {src}");
        assert!(src.contains("#region(\"idx-1\""), "second region: {src}");
        assert_eq!(src.matches("#line(").count(), 1, "one rule between two: {src}");
    }

    #[test]
    fn long_summary_is_truncated() {
        let long = "x".repeat(DEFAULT_SUMMARY_CHARS + 50);
        let e = IndexEntry {
            title: "T".into(),
            byline: None,
            reading_time: None,
            summary: Some(long),
        };
        let src = Index::<()>::new(vec![e]).render(&mut RenderCx::new(0));
        assert!(src.contains("…"), "truncation ellipsis present: {src}");
    }

    #[test]
    fn decode_is_always_empty() {
        let idx = Index::<u8>::new(vec![entry("x")]);
        assert!(idx.decode(&[], &Manifest::default()).is_empty());
    }
}
```

- [ ] **Step 2: Register the module and re-export**

In `crates/inkapp-core/src/components/mod.rs`, add (keeping the list alphabetical):

```rust
pub mod index;
```

In `crates/inkapp-core/src/lib.rs`, add a re-export near the other `pub use` lines:

```rust
pub use components::index::{Index, IndexEntry};
```

- [ ] **Step 3: Run tests**

Run: `nix develop -c cargo test -p inkapp-core index::`
Expected: all unit tests pass.

- [ ] **Step 4: fmt + commit**

```bash
nix develop -c cargo fmt
git add crates/inkapp-core/src/components/index.rs crates/inkapp-core/src/components/mod.rs crates/inkapp-core/src/lib.rs
git commit -m "inkapp-core: Index Display component (theme-aware, paginating contents list)"
```

---

### Task 3: `Article → IndexEntry` leaf conversion

**Goal:** A Readwise-side `From<&Article> for IndexEntry` mapping connector data to component props (byline fallback, verbatim reading_time), keeping core connector-blind.

**Files:**
- Create: `crates/inkapp-readwise-reader/src/index_entry.rs`
- Modify: `crates/inkapp-readwise-reader/src/lib.rs` (add `mod index_entry;`)

**Acceptance Criteria:**
- [ ] `byline` = `author` if non-empty, else `site_name` if non-empty, else `None`.
- [ ] `reading_time` is the article's `Option<String>` passed through unchanged.
- [ ] `summary` is `Some` only when non-empty; `title` copied through.

**Verify:** `nix develop -c cargo test -p inkapp-readwise-reader index_entry` → all pass.

**Steps:**

- [ ] **Step 1: Write `index_entry.rs` (impl + tests)**

Create `crates/inkapp-readwise-reader/src/index_entry.rs`:

```rust
//! Leaf conversion: a Readwise `Article` → the framework's `IndexEntry`. Apps
//! call this in `view` to feed the device-agnostic `Index` component connector
//! data ("dumb leaf conversion"). It lives here, not in `inkapp-core`, because
//! core must stay connector-blind; the orphan rule permits it since `Article` is
//! local to this crate.

use inkapp_core::components::index::IndexEntry;

use crate::Article;

impl From<&Article> for IndexEntry {
    fn from(a: &Article) -> Self {
        let byline = if !a.author.is_empty() {
            Some(a.author.clone())
        } else if !a.site_name.is_empty() {
            Some(a.site_name.clone())
        } else {
            None
        };
        IndexEntry {
            title: a.title.clone(),
            byline,
            // Verbatim String passthrough — reading_time is a label like "5 min",
            // never a number; do not parse or reformat it.
            reading_time: a.reading_time.clone(),
            summary: (!a.summary.is_empty()).then(|| a.summary.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ArticleId;

    fn base() -> Article {
        Article {
            id: ArticleId::new("a1"),
            title: "A Title".into(),
            ..Article::default()
        }
    }

    #[test]
    fn byline_prefers_author() {
        let a = Article {
            author: "Ada".into(),
            site_name: "example.com".into(),
            ..base()
        };
        assert_eq!(IndexEntry::from(&a).byline, Some("Ada".into()));
    }

    #[test]
    fn byline_falls_back_to_site_name() {
        let a = Article {
            author: String::new(),
            site_name: "example.com".into(),
            ..base()
        };
        assert_eq!(IndexEntry::from(&a).byline, Some("example.com".into()));
    }

    #[test]
    fn byline_none_when_both_empty() {
        assert_eq!(IndexEntry::from(&base()).byline, None);
    }

    #[test]
    fn reading_time_passthrough() {
        let a = Article {
            reading_time: Some("5 min".into()),
            ..base()
        };
        assert_eq!(IndexEntry::from(&a).reading_time, Some("5 min".into()));
        assert_eq!(IndexEntry::from(&base()).reading_time, None);
    }

    #[test]
    fn summary_empty_becomes_none() {
        assert_eq!(IndexEntry::from(&base()).summary, None);
        let a = Article {
            summary: "hi".into(),
            ..base()
        };
        assert_eq!(IndexEntry::from(&a).summary, Some("hi".into()));
    }

    #[test]
    fn title_copied() {
        assert_eq!(IndexEntry::from(&base()).title, "A Title");
    }
}
```

- [ ] **Step 2: Register the module**

In `crates/inkapp-readwise-reader/src/lib.rs`, add directly under `pub mod http;`:

```rust
mod index_entry;
```

> A bare `mod` is enough — the file only adds a trait impl, which is active without re-exporting any item.

- [ ] **Step 3: Run tests**

Run: `nix develop -c cargo test -p inkapp-readwise-reader index_entry`
Expected: all pass.

- [ ] **Step 4: fmt + commit**

```bash
nix develop -c cargo fmt
git add crates/inkapp-readwise-reader/src/index_entry.rs crates/inkapp-readwise-reader/src/lib.rs
git commit -m "inkapp-readwise-reader: From<&Article> for IndexEntry leaf conversion"
```

---

### Task 4: Harness clean rasteriser + golden pagination test

**Goal:** A clean (overlay-free) page rasteriser in the harness, and a golden test proving a multi-entry `Index` renders and paginates across two pages.

**Files:**
- Modify: `crates/inkapp-harness/src/inspector.rs` (extract `render_page` / `rasterize` / `encode_png`)
- Create: `crates/inkapp-harness/tests/index_render.rs`
- Create (generated on first run, then committed): `crates/inkapp-harness/tests/golden/index_page0.png`, `index_page1.png`

**Acceptance Criteria:**
- [ ] `inkapp_harness::inspector::render_page(doc, page_index)` returns clean PNG bytes for any page, no region/ink overlays.
- [ ] `inspect` still renders page 0 with overlays (behavior unchanged — existing harness goldens stay green).
- [ ] The test asserts `pages.len() == 2` and `idx-*` regions on both page 0 and page 1, and goldens both pages.

**Verify:** `nix develop -c cargo test -p inkapp-harness --test index_render` → pass (after golden bootstrap, see Step 3).

**Steps:**

- [ ] **Step 1: Refactor `inspector.rs` to share a rasteriser**

In `crates/inkapp-harness/src/inspector.rs`, add the helpers and rewrite `inspect` to use them. Add after the `SCALE` const:

```rust
/// Rasterize one page of `doc` to an RGBA image plus its page height in points.
/// Shared by `render_page` (clean) and `inspect` (which then draws overlays).
fn rasterize(doc: &PagedDocument, page_index: usize) -> Result<(RgbaImage, f32)> {
    let page = doc
        .pages
        .get(page_index)
        .ok_or_else(|| Error::Render(format!("no page {page_index}")))?;
    let page_h_pt = page.frame.height().to_pt() as f32;
    let pixmap = typst_render::render(page, SCALE);
    let img: RgbaImage = RgbaImage::from_raw(pixmap.width(), pixmap.height(), pixmap.data().to_vec())
        .ok_or_else(|| Error::Render("pixmap->image size mismatch".into()))?;
    Ok((img, page_h_pt))
}

fn encode_png(img: &RgbaImage) -> Result<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png)
        .map_err(|e| Error::Render(format!("png encode: {e}")))?;
    Ok(out.into_inner())
}

/// Render one page of `doc` to PNG bytes with **no overlays** (clean typographic
/// raster). `page_index` is 0-based. Used for golden renders of Display content.
pub fn render_page(doc: &PagedDocument, page_index: usize) -> Result<Vec<u8>> {
    let (img, _) = rasterize(doc, page_index)?;
    encode_png(&img)
}
```

Replace the body of `inspect` so it uses `rasterize`/`encode_png` (drop the inline `typst_render::render` + final PNG encode; keep the overlay drawing exactly as-is):

```rust
pub fn inspect(doc: &PagedDocument, manifest: &Manifest, ink: &[Stroke]) -> Result<Vec<u8>> {
    let (mut img, page_h_pt) = rasterize(doc, 0)?;

    // Convert PDF-space (pt, y-up) to image-space (px, y-down).
    let to_px = |x_pt: f64, y_pt: f64| -> (i64, i64) {
        let px = (x_pt as f32 * SCALE).round() as i64;
        let py = ((page_h_pt - y_pt as f32) * SCALE).round() as i64;
        (px, py)
    };

    // Draw region outlines in blue.
    let blue = Rgba([0_u8, 80, 220, 255]);
    for r in &manifest.regions {
        if r.page != 0 {
            continue;
        }
        let (x0, y0) = to_px(r.rect.x0, r.rect.y1);
        let (x1, y1) = to_px(r.rect.x1, r.rect.y0);
        draw_rect_outline(&mut img, x0, y0, x1, y1, blue);
    }

    // Draw ink strokes: red for pen, yellow for highlighter.
    for s in ink {
        let color = if s.highlighter {
            Rgba([230_u8, 210, 0, 255])
        } else {
            Rgba([220_u8, 0, 0, 255])
        };
        let mut prev: Option<(i64, i64)> = None;
        for p in &s.points {
            let cur = to_px(p.x, p.y);
            if let Some(pp) = prev {
                draw_line(&mut img, pp.0, pp.1, cur.0, cur.1, color);
            }
            prev = Some(cur);
        }
    }

    encode_png(&img)
}
```

> Keep the file's existing imports (`image::{ImageFormat, Rgba, RgbaImage}`, `Error`, `Result`, `Stroke`, `Manifest`, `PagedDocument`) and the `put`/`draw_rect_outline`/`draw_line` helpers unchanged.

- [ ] **Step 2: Confirm `inspect` is unchanged behaviorally**

Run: `nix develop -c cargo test -p inkapp-harness`
Expected: existing exercisers (`checkbox_exerciser`, `highlight_exerciser`, …) still pass — their goldens are unaffected by the refactor.

- [ ] **Step 3: Write the golden pagination test**

Create `crates/inkapp-harness/tests/index_render.rs`:

```rust
mod common;
use common::assert_golden;

use std::collections::HashSet;

use inkapp_core::components::index::{Index, IndexEntry};
use inkapp_core::document::Document;
use inkapp_core::flow;
use inkapp_core::geometry::PageGeom;
use inkapp_core::manifest::recover_regions;
use inkapp_core::runtime::compile_document_in;
use inkapp_harness::inspector::render_page;

fn sample_entries(n: usize) -> Vec<IndexEntry> {
    (0..n)
        .map(|i| IndexEntry {
            title: format!("Article number {i}: a reasonably long headline"),
            byline: Some(format!("Author {i}")),
            reading_time: Some(format!("{} min", i + 2)),
            summary: Some(
                "A concise standfirst describing the piece in a sentence or two so the \
                 reader can decide whether to open it."
                    .into(),
            ),
        })
        .collect()
}

#[test]
fn index_renders_and_paginates() {
    // Enough entries to overflow one 420×560 page and force a second.
    // Calibrate `n` so pages.len() == 2 (≈12–16 at default geometry).
    let n = 14;
    let doc: Document<()> = Document::keyed("contents", flow![Index::<()>::new(sample_entries(n))]);
    let compiled = compile_document_in(&doc, PageGeom::default()).unwrap();
    assert_eq!(compiled.pages.len(), 2, "{n} entries paginate to two pages");

    let manifest = recover_regions(&compiled).unwrap();
    let pages: HashSet<usize> = manifest
        .regions
        .iter()
        .filter(|r| r.name.starts_with("idx-"))
        .map(|r| r.page)
        .collect();
    assert!(
        pages.contains(&0) && pages.contains(&1),
        "entries land on both pages: {pages:?}"
    );

    assert_golden("index_page0", &render_page(&compiled, 0).unwrap());
    assert_golden("index_page1", &render_page(&compiled, 1).unwrap());
}
```

- [ ] **Step 4: Bootstrap and review goldens**

Run: `nix develop -c cargo test -p inkapp-harness --test index_render`

First run: if `pages.len()` is not 2, adjust `n` until it is (more entries → more pages). Once pagination holds, bootstrap the goldens. Note `assert_golden` writes the *first* missing golden then panics, so it takes **two runs** to write both (`index_page0` on run 1, `index_page1` on run 2; each panics with "did not exist; wrote it — review and re-run"). After both exist, **open both PNGs** and confirm: a clean grayscale list (no numbers) — bold titles, gray `byline · N min` meta, gray summaries, hairline rules between entries; page 1 continues the list. They must have **no** blue region boxes or colored ink overlays (that would mean `render_page` is wrong).

- [ ] **Step 5: Re-run to verify green against committed goldens**

Run: `nix develop -c cargo test -p inkapp-harness --test index_render`
Expected: PASS (compares against the just-written goldens).

- [ ] **Step 6: fmt + commit (including goldens)**

```bash
nix develop -c cargo fmt
git add crates/inkapp-harness/src/inspector.rs crates/inkapp-harness/tests/index_render.rs crates/inkapp-harness/tests/golden/index_page0.png crates/inkapp-harness/tests/golden/index_page1.png
git commit -m "inkapp-harness: clean page rasteriser + Index render/pagination golden"
```

---

### Task 5: Mark it built in `docs/appdx.md`

**Goal:** Reconcile the developer-experience spec — the project's definition of done — with the new `Index` component and `Theme` seam.

**Files:**
- Modify: `docs/appdx.md`

**Acceptance Criteria:**
- [ ] appdx describes the `Index` Display component alongside `Notice`.
- [ ] appdx notes the device-neutral `Theme`/`RenderCx` palette seam (semantic roles; device-blind components).

**Verify:** `grep -n "Index" docs/appdx.md` shows the new paragraph; full read confirms accuracy.

**Steps:**

- [ ] **Step 1: Insert the `Index` + `Theme` paragraph**

In `docs/appdx.md`, immediately after the `Notice` paragraph (the one ending "…so it drops into any `view` flow.", around line 306), insert:

```markdown

The framework ships an `Index` **Display** component — a typographically clean,
paginating listing for landing/contents documents (the reader's Library and Feed
pages). It takes a list of `IndexEntry { title, byline, reading_time, summary }`
rows and renders each as a non-breakable region box (entries never split across a
page break; the list flows and paginates between them); like `Notice` it decodes
nothing and is generic over the app's `Msg`. Apps map connector data to entries
with a dumb leaf conversion in `view` — `inkapp-readwise-reader` provides
`From<&Article> for IndexEntry` (byline = author or site name; `reading_time`
passed through verbatim) so core stays connector-blind.

Color is device-optimal without leaking the device into components. Components name
semantic palette **roles** (`heading`, `byline`, `muted`, `ink`, `rule`, `paper`)
from a `Theme` carried on the `RenderCx`, never literal colors — the framework
fills the palette, exactly as it supplies page geometry for "page-blind" layout.
The default is grayscale; the reMarkable Paper Pro palette ("Indigo + Tomato")
renders the same document optimally on color e-ink. The palette is set on the
`App` (`.theme(...)`) by a binary's bootstrap, so app `update`/`view` stay
device-blind.
```

- [ ] **Step 2: Verify and run the whole suite once more**

Run: `grep -n "Index Display\|Theme" docs/appdx.md` → shows the inserted text.
Run: `nix develop -c cargo test --workspace` → all green.
Run: `nix develop -c cargo clippy --all-targets -- -D warnings` → clean.

- [ ] **Step 3: fmt + commit**

```bash
nix develop -c cargo fmt
git add docs/appdx.md
git commit -m "docs(appdx): mark Index component + Theme palette seam built"
```

---

## Final verification

- [ ] `nix develop -c cargo test --workspace` — all pass, no golden drift beyond the two new `index_page*.png`.
- [ ] `nix develop -c cargo clippy --all-targets -- -D warnings` — clean.
- [ ] `git status` — `Cargo.lock` is **not** staged in any commit; no stray files.
