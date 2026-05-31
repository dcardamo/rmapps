# Document- & Component-Level State Field — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Carry an app-defined state payload — document-level (one opaque blob) and component-level (a keyed map) — inside the already-sealed PDF manifest, so `decode` interprets ink against the base state the document was rendered with.

**Architecture:** Add a `DocState` payload to `Manifest` (rides the existing XChaCha20-Poly1305 seal — no crypto change). Components opt into state via two default `Component` methods (`state_key`/`render_state`); the render walk collects them into the manifest; `decode` reads its slice from the `&manifest` it already receives (no signature change). A new `Stepper` component proves the round-trip: its decode reads the carried base, not its own current prop.

**Tech Stack:** Rust, `serde`/`serde_json`, Typst (render), `tokio` (loop tests), the inkapp-core crate.

---

### Task 1: `DocState` data model + workspace migration

**Goal:** Add the sealed state payload to `Manifest` and make the whole workspace compile with the new field.

**Files:**
- Modify: `crates/inkapp-core/src/manifest.rs` (add `DocState`, add `Manifest.state`, derive `Default`, init in `recover_regions`)
- Modify (add `..Default::default()` to each literal `Manifest { … }`):
  `crates/inkapp-core/src/components/notice.rs:94`,
  `crates/inkapp-core/tests/calendar_view.rs:80,121`,
  `crates/inkapp-core/tests/checkbox.rs:8`,
  `crates/inkapp-core/tests/highlight_text.rs:11`,
  `crates/inkapp-core/tests/readback.rs:14,58`,
  `crates/inkapp-core/tests/checkbox_component.rs:13`,
  `crates/inkapp-core/tests/checkbox_state.rs:7`,
  `crates/inkapp-core/tests/embed.rs:13,56`,
  `crates/inkapp-core/tests/component.rs:29`,
  `crates/inkapp-harness/tests/inspector.rs:11,40,69`
- Test: `crates/inkapp-core/tests/doc_state.rs` (new)

**Acceptance Criteria:**
- [ ] `DocState` (with `doc` and `components`) serializes/deserializes round-trip.
- [ ] A manifest JSON with no `state` key still deserializes (`#[serde(default)]`).
- [ ] `cargo test -p inkapp-core -p inkapp-harness` compiles; all existing tests pass.

**Verify:** `cargo test -p inkapp-core -p inkapp-harness` → all pass.

**Steps:**

- [ ] **Step 1: Write the failing test** — `crates/inkapp-core/tests/doc_state.rs`

```rust
use inkapp_core::manifest::{DocState, Manifest};
use serde_json::json;

#[test]
fn docstate_round_trips() {
    let mut s = DocState::default();
    s.doc = Some(json!({"cursor": 3}));
    s.components.insert("stepper:c".into(), json!(5u64));
    let bytes = serde_json::to_vec(&s).unwrap();
    let back: DocState = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(s, back);
}

#[test]
fn manifest_without_state_key_deserializes() {
    // Older sealed blobs carry no `state`; serde(default) must fill it.
    let json = r#"{"version":2,"regions":[]}"#;
    let m: Manifest = serde_json::from_str(json).unwrap();
    assert_eq!(m.version, 2);
    assert_eq!(m.state, DocState::default());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inkapp-core --test doc_state`
Expected: FAIL — `DocState` not found / `Manifest` has no field `state`.

- [ ] **Step 3: Add `DocState` and the `state` field** in `crates/inkapp-core/src/manifest.rs`

Add `use std::collections::BTreeMap;` at the top. Then:

```rust
/// App-defined state carried inside the (sealed) manifest. The framework only
/// encrypts and carries it; the app/component owns the contents.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DocState {
    /// Document-level, app-owned. Set by the app in `view`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<serde_json::Value>,
    /// Component-level, keyed by each component's `state_key()`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub components: BTreeMap<String, serde_json::Value>,
}
```

Change the `Manifest` struct (add `Default` to its derive list and the field):

```rust
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u64,
    pub regions: Vec<Region>,
    #[serde(default)]
    pub state: DocState,
}
```

In `recover_regions`, change the returned literal to initialize the field:

```rust
    Ok(Manifest {
        version: 0,
        regions,
        state: DocState::default(),
    })
```

- [ ] **Step 4: Migrate every other literal `Manifest { … }`** — append `..Default::default()` after the existing fields at each site listed in **Files**. Example (`tests/embed.rs:13`):

```rust
    let manifest = Manifest {
        version: 7,
        regions: vec![Region { /* … unchanged … */ }],
        ..Default::default()
    };
```

Apply the identical `..Default::default()` addition to all listed sites (notice.rs, calendar_view.rs ×2, checkbox.rs, highlight_text.rs, readback.rs ×2, checkbox_component.rs, checkbox_state.rs, embed.rs ×2, component.rs, inspector.rs ×3).

- [ ] **Step 5: Run tests**

Run: `cargo test -p inkapp-core --test doc_state` → PASS
Run: `cargo test -p inkapp-core -p inkapp-harness` → all PASS

- [ ] **Step 6: Commit**

```bash
git add crates/inkapp-core/src/manifest.rs crates/inkapp-core/tests/doc_state.rs \
        crates/inkapp-core/src/components/notice.rs crates/inkapp-core/tests crates/inkapp-harness/tests/inspector.rs
git commit -m "inkapp-core: add DocState payload to Manifest (sealed state field)"
```

---

### Task 2: `Component` state hooks

**Goal:** Add opt-in `state_key`/`render_state` default methods to the `Component` trait; existing components inherit no-ops.

**Files:**
- Modify: `crates/inkapp-core/src/component.rs`
- Test: `crates/inkapp-core/tests/component_state_hooks.rs` (new)

**Acceptance Criteria:**
- [ ] `Component` has `state_key(&self) -> Option<String>` and `render_state(&self) -> Option<serde_json::Value>`, both defaulting to `None`.
- [ ] An existing stateless component (`Notice`) returns `None` from both.
- [ ] A component that overrides them returns its `Some(..)` values.

**Verify:** `cargo test -p inkapp-core --test component_state_hooks` → PASS

**Steps:**

- [ ] **Step 1: Write the failing test** — `crates/inkapp-core/tests/component_state_hooks.rs`

```rust
use inkapp_core::component::Component;
use inkapp_core::components::notice::Notice;
use inkapp_core::ink::RegionInk;
use inkapp_core::manifest::Manifest;
use inkapp_core::widget::RenderCx;
use serde_json::json;

// A minimal stateful component used only to exercise the new hooks.
struct Stateful;
impl Component for Stateful {
    type Msg = ();
    fn render(&self, _cx: &mut RenderCx) -> String { String::new() }
    fn decode(&self, _ink: &[RegionInk], _m: &Manifest) -> Vec<()> { vec![] }
    fn state_key(&self) -> Option<String> { Some("k".into()) }
    fn render_state(&self) -> Option<serde_json::Value> { Some(json!(42)) }
}

#[test]
fn stateless_component_has_no_state() {
    let n: Notice<()> = Notice::line("hi");
    assert_eq!(n.state_key(), None);
    assert_eq!(n.render_state(), None);
}

#[test]
fn stateful_component_reports_state() {
    let s = Stateful;
    assert_eq!(s.state_key(), Some("k".to_string()));
    assert_eq!(s.render_state(), Some(json!(42)));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inkapp-core --test component_state_hooks`
Expected: FAIL — no method `state_key` on `Component`.

- [ ] **Step 3: Add the default methods** to the `Component` trait in `crates/inkapp-core/src/component.rs` (after `typst_sources`, before the closing `}`):

```rust
    /// Stable, props-derived key under which this component's state is carried in
    /// the sealed manifest. `None` (default) = stateless. Derive from identity
    /// props (e.g. a name/id), never from volatile content, so the key is
    /// identical at render time and at the next cycle's pre-fold decode.
    fn state_key(&self) -> Option<String> {
        None
    }

    /// The state to seal at render time — the base the document is rendered with.
    /// `None` (default) = nothing carried.
    fn render_state(&self) -> Option<serde_json::Value> {
        None
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p inkapp-core --test component_state_hooks` → PASS

- [ ] **Step 5: Commit**

```bash
git add crates/inkapp-core/src/component.rs crates/inkapp-core/tests/component_state_hooks.rs
git commit -m "inkapp-core: add opt-in state_key/render_state hooks to Component"
```

---

### Task 3: Document-level state slot

**Goal:** Let an app attach an opaque document-level state value via `Document::keyed_with_state`.

**Files:**
- Modify: `crates/inkapp-core/src/document.rs`
- Test: `crates/inkapp-core/tests/document.rs` (extend)

**Acceptance Criteria:**
- [ ] `Document<M>` has a `state: Option<serde_json::Value>` field.
- [ ] `Document::keyed(..)` leaves `state == None`.
- [ ] `Document::keyed_with_state(key, flow, value)` sets `state == Some(value)`.

**Verify:** `cargo test -p inkapp-core --test document` → PASS

**Steps:**

- [ ] **Step 1: Write the failing test** — append to `crates/inkapp-core/tests/document.rs`

```rust
#[test]
fn keyed_has_no_state_keyed_with_state_does() {
    use inkapp_core::flow;
    use serde_json::json;
    let plain: inkapp_core::document::Document<()> = Document::keyed("k", flow![]);
    assert_eq!(plain.state, None);
    let stateful: inkapp_core::document::Document<()> =
        Document::keyed_with_state("k", flow![], json!({"cursor": 1}));
    assert_eq!(stateful.state, Some(json!({"cursor": 1})));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inkapp-core --test document`
Expected: FAIL — no field `state` / no function `keyed_with_state`.

- [ ] **Step 3: Add the field and constructor** in `crates/inkapp-core/src/document.rs`

Change the struct:

```rust
/// One document: a key plus an ordered flow of components, plus optional
/// app-owned document-level state carried in the sealed manifest.
pub struct Document<M> {
    pub key: DocKey,
    pub flow: Vec<Box<dyn Component<Msg = M>>>,
    pub state: Option<serde_json::Value>,
}
```

Update `keyed` and add `keyed_with_state` in the `impl<M> Document<M>` block:

```rust
    pub fn keyed(key: impl Into<String>, flow: Vec<Box<dyn Component<Msg = M>>>) -> Self {
        Self {
            key: DocKey::new(key),
            flow,
            state: None,
        }
    }

    /// Like `keyed`, but carries document-level state sealed into the manifest.
    pub fn keyed_with_state(
        key: impl Into<String>,
        flow: Vec<Box<dyn Component<Msg = M>>>,
        state: serde_json::Value,
    ) -> Self {
        Self {
            key: DocKey::new(key),
            flow,
            state: Some(state),
        }
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p inkapp-core --test document` → PASS
Run: `cargo test -p inkapp-core` → all PASS (confirms no other `Document::keyed` caller broke)

- [ ] **Step 5: Commit**

```bash
git add crates/inkapp-core/src/document.rs crates/inkapp-core/tests/document.rs
git commit -m "inkapp-core: add document-level state slot (keyed_with_state)"
```

---

### Task 4: Render-side state collection

**Goal:** `render_document` populates `manifest.state` (doc blob + per-component map) before sealing.

**Files:**
- Modify: `crates/inkapp-core/src/runtime.rs` (`render_document`)
- Test: `crates/inkapp-core/tests/render_state.rs` (new)

**Acceptance Criteria:**
- [ ] After `render_document`, the returned `RenderedDoc.manifest.state.doc` equals the `Document`'s state.
- [ ] Each stateful component's `render_state()` appears under its `state_key()` in `manifest.state.components`.
- [ ] Stateless components contribute nothing.

**Verify:** `cargo test -p inkapp-core --test render_state` → PASS

**Steps:**

- [ ] **Step 1: Write the failing test** — `crates/inkapp-core/tests/render_state.rs`

```rust
use inkapp_core::component::Component;
use inkapp_core::crypto::Key;
use inkapp_core::document::Document;
use inkapp_core::flow;
use inkapp_core::ink::RegionInk;
use inkapp_core::manifest::Manifest;
use inkapp_core::runtime::render_document;
use inkapp_core::widget::RenderCx;
use serde_json::json;

// Minimal stateful component: emits no regions, just carries state.
struct Carrier {
    key: String,
    value: u64,
}
impl Component for Carrier {
    type Msg = ();
    fn render(&self, _cx: &mut RenderCx) -> String {
        // A trivial visible glyph so the page is non-empty.
        format!("#text[{}]\n", self.value)
    }
    fn decode(&self, _ink: &[RegionInk], _m: &Manifest) -> Vec<()> { vec![] }
    fn state_key(&self) -> Option<String> { Some(self.key.clone()) }
    fn render_state(&self) -> Option<serde_json::Value> { Some(json!(self.value)) }
}

#[test]
fn render_collects_doc_and_component_state() {
    let doc: Document<()> = Document::keyed_with_state(
        "d",
        flow![Carrier { key: "carrier:1".into(), value: 5 }],
        json!({"cursor": 3}),
    );
    let rd = render_document(&doc, 1, &Key::from_bytes([7u8; 32])).unwrap();
    assert_eq!(rd.manifest.state.doc, Some(json!({"cursor": 3})));
    assert_eq!(
        rd.manifest.state.components.get("carrier:1"),
        Some(&json!(5u64))
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inkapp-core --test render_state`
Expected: FAIL — `manifest.state.doc` is `None` and `components` is empty (collection not wired).

- [ ] **Step 3: Wire collection** in `crates/inkapp-core/src/runtime.rs`, inside `render_document`, right after the `let manifest = recover_regions(...)?.with_version(version);` line. Change that binding to `let mut manifest` and insert the collection before `embed_manifest`:

```rust
    let mut manifest = recover_regions(&compiled)?.with_version(version);
    // Collect app-defined state into the manifest before sealing: the document's
    // own blob, then each stateful component's slice keyed by state_key().
    manifest.state.doc = doc.state.clone();
    for c in &doc.flow {
        if let (Some(k), Some(v)) = (c.state_key(), c.render_state()) {
            manifest.state.components.insert(k, v);
        }
    }
    let pdf = embed_manifest(&document_to_pdf(&compiled)?, &manifest, key)?;
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p inkapp-core --test render_state` → PASS
Run: `cargo test -p inkapp-core` → all PASS

- [ ] **Step 5: Commit**

```bash
git add crates/inkapp-core/src/runtime.rs crates/inkapp-core/tests/render_state.rs
git commit -m "inkapp-core: collect doc- & component-level state into manifest at render"
```

---

### Task 5: `Stepper` component (the proof consumer)

**Goal:** Ship a genuinely stateful component whose `read`/`decode` compute their result from the **carried base** in the manifest, provably ignoring the component's own current prop.

**Files:**
- Create: `crates/inkapp-core/src/components/stepper.rs`
- Modify: `crates/inkapp-core/src/components/mod.rs` (add `pub mod stepper;`)
- Test: `crates/inkapp-core/tests/stepper_state.rs` (new)

**Acceptance Criteria:**
- [ ] `Stepper` implements `Stateful` hooks: `state_key()` → `Some("stepper:<name>")`, `render_state()` → `Some(count)`.
- [ ] `Widget::read` returns `carried_base + (# increment strokes in its region)`.
- [ ] `Component::decode` (`type Msg = u64`) emits `vec![carried_base + increments]` when ≥1 increment stroke, else `vec![]`.
- [ ] **Keystone:** a `Stepper { count: 9 }` decoded against a manifest carrying base `5` with one stroke yields `vec![6]` (uses carried base, not the prop `9`).

**Verify:** `cargo test -p inkapp-core --test stepper_state` → PASS

**Steps:**

- [ ] **Step 1: Write the failing test** — `crates/inkapp-core/tests/stepper_state.rs`

```rust
use inkapp_core::component::Component;
use inkapp_core::components::stepper::Stepper;
use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::{DocState, Manifest, Region};
use inkapp_core::widget::Widget;
use serde_json::json;

const RECT: PdfRect = PdfRect { x0: 0.0, y0: 0.0, x1: 20.0, y1: 20.0 };

fn manifest_with_base(base: u64) -> Manifest {
    let mut state = DocState::default();
    state.components.insert("stepper:c".into(), json!(base));
    Manifest {
        version: 1,
        regions: vec![Region { name: "stepper:c".into(), page: 0, rect: RECT }],
        state,
    }
}

fn one_tick() -> Vec<RegionInk> {
    vec![RegionInk {
        region: "stepper:c".into(),
        strokes: vec![Stroke { points: vec![PdfPoint { x: 10.0, y: 10.0 }], highlighter: false }],
    }]
}

#[test]
fn read_uses_carried_base_not_prop() {
    let s = Stepper::new("c", 9); // current prop says 9
    assert_eq!(s.read(&one_tick(), &manifest_with_base(5)), 6); // 5 + 1, NOT 10
}

#[test]
fn decode_emits_base_relative_count() {
    let s = Stepper::new("c", 9);
    assert_eq!(s.decode(&one_tick(), &manifest_with_base(5)), vec![6u64]);
}

#[test]
fn decode_empty_without_ink() {
    let s = Stepper::new("c", 5);
    assert_eq!(s.decode(&[], &manifest_with_base(5)), Vec::<u64>::new());
}

#[test]
fn read_missing_state_treats_base_as_zero() {
    // No carried state for this key -> base 0.
    let s = Stepper::new("c", 9);
    let m = Manifest {
        version: 1,
        regions: vec![Region { name: "stepper:c".into(), page: 0, rect: RECT }],
        ..Default::default()
    };
    assert_eq!(s.read(&one_tick(), &m), 1); // 0 + 1
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inkapp-core --test stepper_state`
Expected: FAIL — module `stepper` / `Stepper` not found.

- [ ] **Step 3: Implement `Stepper`** — `crates/inkapp-core/src/components/stepper.rs`

```rust
use crate::component::Component;
use crate::ink::RegionInk;
use crate::manifest::Manifest;
use crate::widget::{is_valid_region_name, region_metadata, RenderCx, Widget};

/// A counter whose state lives ONLY in the document (no connector). It renders
/// its current count and an increment region; on readback it adds the number of
/// increment strokes to the **carried base** (the count it was rendered with),
/// not to its own current prop — proving decode interprets ink against the base
/// the document was rendered against.
pub struct Stepper {
    name: String,
    count: u64,
}

impl Stepper {
    pub fn new(name: &str, count: u64) -> Self {
        Self { name: name.to_string(), count }
    }

    fn region_name(&self) -> String {
        format!("stepper:{}", self.name)
    }

    /// The base this document was rendered with (0 if none carried).
    fn carried_base(&self, manifest: &Manifest) -> u64 {
        manifest
            .state
            .components
            .get(&self.region_name())
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    }

    /// Count strokes attributed to this stepper's region with a point inside it.
    fn increments(&self, ink: &[RegionInk], manifest: &Manifest) -> u64 {
        let name = self.region_name();
        let Some(region) = manifest.regions.iter().find(|r| r.name == name) else {
            return 0;
        };
        ink.iter()
            .filter(|ri| ri.region == name)
            .flat_map(|ri| &ri.strokes)
            .filter(|s| s.points.iter().any(|p| region.rect.contains(p.x, p.y)))
            .count() as u64
    }
}

impl Widget for Stepper {
    type Output = u64;

    fn render(&self, cx: &mut RenderCx) -> String {
        let name = self.region_name();
        assert!(
            is_valid_region_name(&name),
            "stepper region name must be valid, got: {name:?}"
        );
        let (x, y, w, h) = (20.0_f64, 40.0_f64, 16.0_f64, 16.0_f64);
        let mut s = region_metadata(&name, cx.page, x, y, w, h);
        s.push_str(&format!(
            "#place(top + left, dx: {x}pt, dy: {y}pt, rect(width: {w}pt, height: {h}pt, stroke: 0.5pt)[#align(center + horizon)[+]])\n"
        ));
        s.push_str(&format!("#text[{}]\n", self.count));
        s
    }

    fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> u64 {
        self.carried_base(manifest) + self.increments(ink, manifest)
    }
}

impl Component for Stepper {
    type Msg = u64;

    fn render(&self, cx: &mut RenderCx) -> String {
        <Self as Widget>::render(self, cx)
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<u64> {
        let increments = self.increments(ink, manifest);
        if increments > 0 {
            vec![self.carried_base(manifest) + increments]
        } else {
            vec![]
        }
    }

    fn state_key(&self) -> Option<String> {
        Some(self.region_name())
    }

    fn render_state(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!(self.count))
    }
}
```

- [ ] **Step 4: Register the module** — add to `crates/inkapp-core/src/components/mod.rs` (keep the list alphabetical-ish, after `notice`):

```rust
pub mod stepper;
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p inkapp-core --test stepper_state` → PASS
Run: `cargo test -p inkapp-core` → all PASS

- [ ] **Step 6: Commit**

```bash
git add crates/inkapp-core/src/components/stepper.rs crates/inkapp-core/src/components/mod.rs crates/inkapp-core/tests/stepper_state.rs
git commit -m "inkapp-core: add stateful Stepper component (carried-base decode proof)"
```

---

### Task 6: Sealed-PDF travel proof

**Goal:** Prove the state payload survives the real seal/extract round-trip and never appears in cleartext in the PDF.

**Files:**
- Modify: `crates/inkapp-core/tests/embed.rs` (add two tests)

**Acceptance Criteria:**
- [ ] `embed_manifest` → `extract_manifest` returns a manifest whose `state` equals the input (doc blob + component map).
- [ ] A distinctive doc-level marker string and the component count's bytes do **not** appear in the embedded PDF bytes.

**Verify:** `cargo test -p inkapp-core --test embed` → PASS

**Steps:**

- [ ] **Step 1: Write the failing tests** — append to `crates/inkapp-core/tests/embed.rs`

```rust
#[test]
fn state_round_trips_and_stays_sealed() {
    use inkapp_core::manifest::DocState;
    use serde_json::json;

    let doc = compile_to_document("#set page(width: 100pt, height: 100pt)\nhi").unwrap();
    let pdf = document_to_pdf(&doc).unwrap();

    let mut state = DocState::default();
    // A distinctive marker we can search for in cleartext.
    state.doc = Some(json!({"marker": "SEKRIT_CURSOR_7"}));
    state.components.insert("stepper:c".into(), json!(424242u64));

    let manifest = Manifest {
        version: 3,
        regions: vec![],
        state,
    };

    let key = Key::from_bytes([5u8; 32]);
    let embedded = embed_manifest(&pdf, &manifest, &key).unwrap();

    // No-cleartext tier: neither the doc marker nor the component value leaks.
    assert!(
        !embedded.windows(15).any(|w| w == b"SEKRIT_CURSOR_7"),
        "doc-level state leaked into the PDF in cleartext"
    );
    assert!(
        !embedded.windows(6).any(|w| w == b"424242"),
        "component state value leaked into the PDF in cleartext"
    );

    let got = extract_manifest(&embedded, &key).unwrap();
    assert_eq!(got, manifest);
    assert_eq!(got.state.doc, manifest.state.doc);
    assert_eq!(got.state.components.get("stepper:c"), Some(&json!(424242u64)));
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p inkapp-core --test embed`
Expected: PASS (the seal already carries the full manifest; this test confirms the new field rides it and stays encrypted). If it fails, the bug is upstream in Task 1/4 — fix there.

- [ ] **Step 3: Commit**

```bash
git add crates/inkapp-core/tests/embed.rs
git commit -m "inkapp-core: prove state field round-trips sealed in the PDF"
```

---

### Task 7: Loop-level carried-base proof

**Goal:** Through `App::step`, prove decode interprets ink against the carried base even after the server `Model` has moved on.

**Files:**
- Test: `crates/inkapp-core/tests/stepper_loop.rs` (new)

**Acceptance Criteria:**
- [ ] Render with model count `5` (so the stored manifest carries base `5`).
- [ ] Mutate `app.model` to `9` with no re-render.
- [ ] `step` with one increment stroke on the stepper region yields `cycle.decoded == vec![6]` (= carried `5` + 1), **not** `vec![10]`.

**Verify:** `cargo test -p inkapp-core --test stepper_loop` → PASS

**Steps:**

- [ ] **Step 1: Write the failing test** — `crates/inkapp-core/tests/stepper_loop.rs`

```rust
use std::collections::HashMap;
use std::sync::Arc;

use inkapp_core::components::stepper::Stepper;
use inkapp_core::connector::{Connector, ConnectorSet};
use inkapp_core::crypto::Key;
use inkapp_core::document::{DocKey, Document, Documents};
use inkapp_core::flow;
use inkapp_core::geometry::PdfPoint;
use inkapp_core::ink::Stroke;
use inkapp_core::runtime::{app, DocSet};

// Model is just the current counter value; Msg is the decoded new count.
type Model = u64;
type Msg = u64;

struct Cx;
impl ConnectorSet for Cx {
    fn connectors(&self) -> Vec<Arc<dyn Connector>> {
        vec![]
    }
}

fn update(msg: Msg, m: &mut Model, _cx: &Cx) {
    *m = msg;
}

fn view(m: &Model, _cx: &Cx) -> Documents<Msg> {
    Documents(vec![Document::keyed("c", flow![Stepper::new("c", *m)])])
}

fn tick(set: &DocSet, key: &str) -> Vec<Stroke> {
    let m = set.manifest(&DocKey::new(key)).expect("rendered doc");
    let r = m
        .regions
        .iter()
        .find(|r| r.name == "stepper:c")
        .expect("stepper region");
    let cx = (r.rect.x0 + r.rect.x1) / 2.0;
    let cy = (r.rect.y0 + r.rect.y1) / 2.0;
    vec![Stroke { points: vec![PdfPoint { x: cx, y: cy }], highlighter: false }]
}

#[tokio::test]
async fn decode_uses_carried_base_through_loop() {
    let mut app = app(5u64)
        .connector(Cx)
        .update(update)
        .view(view)
        .key(Key::from_bytes([9u8; 32]))
        .build();
    let mut set = DocSet::default();

    // Cycle 0: render at count 5 -> stored manifest carries base 5.
    app.render(&mut set).await.unwrap();

    // Server state moves on to 9 with NO re-render (the device still shows 5).
    app.model = 9;

    // The user inks one increment on the stale (base-5) document.
    let mut ink: HashMap<String, Vec<Stroke>> = HashMap::new();
    ink.insert("c".into(), tick(&set, "c"));

    let cycle = app.step(&mut set, &ink).await.unwrap();

    // Decoded against carried base 5 (=5+1=6), NOT current model 9 (=10).
    assert_eq!(cycle.decoded, vec![6u64]);
}
```

- [ ] **Step 2: Run test to verify it fails (then passes)**

Run: `cargo test -p inkapp-core --test stepper_loop`
Expected: PASS once Tasks 4 & 5 are in. If it yields `vec![10]`, decode is reading the prop instead of the carried base — fix `Stepper::decode`/`carried_base` (Task 5).

- [ ] **Step 3: Commit**

```bash
git add crates/inkapp-core/tests/stepper_loop.rs
git commit -m "inkapp-core: loop test proves decode uses carried base over current model"
```

---

### Task 8: Mark the appdx state field built (definition of done)

**Goal:** Update `docs/appdx.md` so its State and Encryption sections — and the top banner — record the state-field payload as built. This is the spec's stated goal.

**Files:**
- Modify: `docs/appdx.md`

**Acceptance Criteria:**
- [ ] The State section's `(Seam ready; no state-field payload is carried yet — (open))` note is replaced with a `(Built …)` note describing document- and component-level state riding the sealed manifest.
- [ ] The Encryption section's matching state-field note reads as built.
- [ ] The top status banner no longer implies the state field is unbuilt.
- [ ] No other `(open)`/future markers are altered (event sourcing, multi-user/cloud, tidies stay as-is).

**Verify:** `rg -n "state-field|state field|no state-field payload" docs/appdx.md` shows the updated, built wording; `rg -n "Seam ready" docs/appdx.md` returns nothing.

**Steps:**

- [ ] **Step 1: Update the State section.** In `docs/appdx.md`, replace the bullet that currently reads:

```
- The app's **state field** carried in the document (document- and component-level)
  will be encrypted through the same seam — your code works in plaintext, the
  framework encrypts on write and decrypts on read. *(Seam ready; no state-field
  payload is carried yet — **(open)**.)*
```

with:

```
- The app's **state field** is carried in the document at both document- and
  component-level, encrypted through the same seam — your code works in plaintext,
  the framework encrypts on write and decrypts on read. *(Built: a `DocState`
  payload rides the sealed manifest; components opt in via `state_key`/`render_state`
  and `decode` reads its slice from the manifest, so ink is interpreted against the
  base the document was rendered with. `Stepper` proves it end-to-end.)*
```

- [ ] **Step 2: Update the Encryption bullet** in the State section (the `(Seam ready…)` parenthetical near "everything embedded is encrypted"). Replace:

```
**Encryption — everything embedded is encrypted.** *(Built: the embedded manifest
is sealed today; the per-component state field below rides the same seam when it
lands.)*
```

with:

```
**Encryption — everything embedded is encrypted.** *(Built: the embedded manifest
is sealed today, and the document- and component-level state field rides the same
seam.)*
```

- [ ] **Step 3: Update the top banner.** In the status blockquote at the top of `docs/appdx.md`, the live text reads (across wrapped lines): *"…`Checkbox`'s render half authored in `checkbox.typ`) are all implemented and tested. What remains is the explicitly-future material — event sourcing/CRDT, multi-user/cloud — not the spine."* Replace the sentence run *"are all implemented and tested. What remains is the explicitly-future material"* with:

```
are all implemented and tested, and the **document- & component-level state field**
now rides the sealed manifest. What remains is only the explicitly-future material
```

- [ ] **Step 4: Update the Build-order line.** A few lines below, the banner reads *"**T** Typst authoring *(all five done)*"*. Change `*(all five done)*` to `→ state field *(all done)*` so the line records the state field as the completed step after T. Keep the rest of that sentence intact.

- [ ] **Step 5: Verify wording**

Run: `rg -n "Seam ready" docs/appdx.md` → no matches
Run: `rg -n "state field" docs/appdx.md` → shows the built wording

- [ ] **Step 6: Commit**

```bash
git add docs/appdx.md
git commit -m "appdx: state field (document- & component-level) built"
```

---

## Notes for the implementer

- **Run from the repo root.** All `cargo` commands target the workspace; `-p inkapp-core` scopes to the core crate.
- **`serde_json` is already a dependency** of `inkapp-core` (used in `manifest.rs`); no `Cargo.toml` change is needed. If a test crate complains it's not in scope, it's available transitively via `inkapp_core::` re-exports used in the snippets, or add `serde_json` under `[dev-dependencies]` if the harness crate needs it directly.
- **The seal/crypto code (`embed.rs`, `crypto.rs`) is intentionally untouched** — it serializes and seals the whole `Manifest`, so the new field rides along for free. If a state round-trip test fails, the bug is in collection (Task 4) or the data model (Task 1), not the seal.
- **Commit hook:** this repo's pre-commit hook blocks commits while native tasks are open. Mark each task `completed` before committing it (the executing skill handles this).
```
