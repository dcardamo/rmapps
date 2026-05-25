# GestureAction Component Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a reusable device-agnostic `GestureAction<M>` Control component that turns a striking pen gesture over its region into a value-message (e.g. strike through a title to archive it), with real-fixture tests.

**Architecture:** A pure `Component` (render + decode) in `inkapp-core`, mirroring `Checkbox`/`Passage` (value-message via `with_msg`, no closures). Render emits a single non-breakable `#region` carrying the target content; decode fires iff the combined bounding box of the region's *non-highlighter* strokes spans ≥ 60% of the region width. The harness simulator already has the fixture-replay path (`Gesture::Fixture`), so testing wires real captured gestures through `simulate` with no framework change.

**Tech Stack:** Rust, Typst-as-library (`#region` prelude), inkapp-harness simulator + JSON gesture fixtures, `nix develop -c cargo test`.

---

## File Structure

- **Create** `crates/inkapp-core/src/components/gesture.rs` — the `GestureAction<M>` component (render + decode + `read`).
- **Modify** `crates/inkapp-core/src/components/mod.rs` — register `pub mod gesture;` (one line).
- **Create** `crates/inkapp-core/tests/gesture_action.rs` — core unit tests (synthesized strokes, no harness dep) + render→recover→decode end-to-end.
- **Modify** `crates/inkapp-harness/tests/exercisers.rs` — add `gesture_action_exerciser` driving real fixtures through `simulate`.
- **Modify** `docs/appdx.md` — reconcile: mark `GestureAction` built (definition of done).

No simulator change: `Gesture::Fixture` already loads `tests/fixtures/gestures/<name>.json`, transplants the default sample into the target region, and sets `highlighter` from the fixture's `tool`.

---

### Task 1: `GestureAction<M>` component + core unit tests

**Goal:** The component exists, renders a single region, and decodes a striking pen gesture (not taps, not highlighter) into one message — proven by unit tests including a full render→recover→decode round trip.

**Files:**
- Create: `crates/inkapp-core/src/components/gesture.rs`
- Modify: `crates/inkapp-core/src/components/mod.rs` (add `pub mod gesture;`)
- Test: `crates/inkapp-core/tests/gesture_action.rs`

**Acceptance Criteria:**
- [ ] `GestureAction::new(name, content)` (`M=()`) and `GestureAction::with_msg(name, content, on_gesture)` compile.
- [ ] A wide non-highlighter stroke fires; a tap and a wide highlighter stroke do not.
- [ ] `render` emits `#region("<name>"` containing the content.
- [ ] End-to-end render→recover→attribute→decode fires on a region-spanning pen strike.

**Verify:** `nix develop -c cargo test -p inkapp-core --test gesture_action` → all tests pass.

**Steps:**

- [ ] **Step 1: Write the failing tests**

Create `crates/inkapp-core/tests/gesture_action.rs`:

```rust
use inkapp_core::component::{Component, RenderCx};
use inkapp_core::components::gesture::GestureAction;
use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::{Manifest, Region};

#[derive(Debug, Clone, PartialEq)]
enum M {
    Archived,
}

fn manifest_with(name: &str, rect: PdfRect) -> Manifest {
    Manifest {
        version: 1,
        regions: vec![Region {
            name: name.into(),
            page: 0,
            rect,
        }],
        ..Default::default()
    }
}

// A title region 100pt wide, 16pt tall.
const TITLE_RECT: PdfRect = PdfRect {
    x0: 10.0,
    y0: 10.0,
    x1: 110.0,
    y1: 26.0,
};

fn region_ink(highlighter: bool, points: Vec<PdfPoint>) -> Vec<RegionInk> {
    vec![RegionInk {
        region: "title".into(),
        strokes: vec![Stroke {
            points,
            highlighter,
        }],
    }]
}

#[test]
fn fires_on_wide_pen_strike() {
    let g = GestureAction::with_msg("title", "How CGI changed the web", M::Archived);
    let manifest = manifest_with("title", TITLE_RECT);
    // A horizontal pen stroke spanning ~96% of the region width.
    let ink = region_ink(
        false,
        vec![
            PdfPoint { x: 12.0, y: 18.0 },
            PdfPoint { x: 108.0, y: 18.0 },
        ],
    );
    assert_eq!(g.decode(&ink, &manifest), vec![M::Archived]);
}

#[test]
fn no_fire_on_tap() {
    let g = GestureAction::with_msg("title", "How CGI changed the web", M::Archived);
    let manifest = manifest_with("title", TITLE_RECT);
    let ink = region_ink(false, vec![PdfPoint { x: 60.0, y: 18.0 }]);
    assert!(g.decode(&ink, &manifest).is_empty(), "a single dot is not a strike");
}

#[test]
fn no_fire_on_highlighter_swipe() {
    let g = GestureAction::with_msg("title", "How CGI changed the web", M::Archived);
    let manifest = manifest_with("title", TITLE_RECT);
    // Same wide geometry as the firing case, but a highlighter — wrong tool.
    let ink = region_ink(
        true,
        vec![
            PdfPoint { x: 12.0, y: 18.0 },
            PdfPoint { x: 108.0, y: 18.0 },
        ],
    );
    assert!(
        g.decode(&ink, &manifest).is_empty(),
        "a highlighter swipe must not fire the action"
    );
}

#[test]
fn no_fire_when_empty() {
    let g = GestureAction::with_msg("title", "How CGI changed the web", M::Archived);
    let manifest = manifest_with("title", TITLE_RECT);
    assert!(g.decode(&[], &manifest).is_empty());
}

#[test]
fn render_declares_region_and_content() {
    let g = GestureAction::with_msg("title", "Hello", M::Archived);
    let markup = g.render(&mut RenderCx::new(0));
    assert!(
        markup.contains("#region(\"title\""),
        "calls the region prelude: {markup}"
    );
    assert!(markup.contains("Hello"), "content present: {markup}");
}

// Integration: render → recover → attribute → decode, single page.
use inkapp_core::document::Document;
use inkapp_core::flow;
use inkapp_core::geometry::PageGeom;
use inkapp_core::manifest::recover_regions;
use inkapp_core::readback::attribute_page;
use inkapp_core::runtime::compile_document_in;

#[test]
fn gesture_action_decodes_strike_end_to_end() {
    let doc: Document<M> = Document::keyed(
        "d",
        flow![GestureAction::with_msg(
            "title",
            "How CGI changed the web",
            M::Archived
        )],
    );
    let compiled = compile_document_in(&doc, PageGeom::default()).unwrap();
    let manifest = recover_regions(&compiled).unwrap();

    let region = manifest
        .regions
        .iter()
        .find(|r| r.name == "title")
        .expect("title region recovered");
    // A pen strike spanning the full recovered region width.
    let cy = (region.rect.y0 + region.rect.y1) / 2.0;
    let stroke = Stroke {
        points: vec![
            PdfPoint {
                x: region.rect.x0,
                y: cy,
            },
            PdfPoint {
                x: region.rect.x1,
                y: cy,
            },
        ],
        highlighter: false,
    };
    let ink = attribute_page(&[stroke], &manifest);
    let decoded = doc.flow[0].decode(&ink, &manifest);
    assert_eq!(
        decoded,
        vec![M::Archived],
        "a region-spanning pen strike decodes to one Archived"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `nix develop -c cargo test -p inkapp-core --test gesture_action`
Expected: compile error — `unresolved import inkapp_core::components::gesture` (module doesn't exist yet).

- [ ] **Step 3: Create the component**

Create `crates/inkapp-core/src/components/gesture.rs`:

```rust
//! `GestureAction` — a Control component that fires a value-message when a
//! *striking pen gesture* lands on its region. It renders its target content as a
//! single region and decodes a non-highlighter stroke whose combined bounding box
//! spans most of the region's width (a horizontal strike or scribble) into one
//! message — while ignoring incidental marks, taps, and highlighter swipes. It
//! carries the message as a value (Elm's value-message, no stored closure), like
//! `Checkbox`/`Passage`. This ports the old `rmreader` `classify.rs` intent
//! (geometry → action) as a clean, reusable component.

use crate::component::{Component, RenderCx};
use crate::components::esc_typst_str;
use crate::ink::RegionInk;
use crate::manifest::Manifest;
use crate::render::is_valid_region_name;

/// A non-highlighter gesture whose combined bbox spans at least this fraction of
/// the region width reads as a deliberate strike/scribble (the action) rather than
/// an incidental mark. A strike/scribble fills the line; a tick or dot does not.
const STRIKE_WIDTH_RATIO: f64 = 0.6;

/// A Control bound to one named region that fires `on_gesture` when struck through.
/// `M` defaults to `()` for a presence-only control.
pub struct GestureAction<M = ()> {
    name: String,
    content: String,
    on_gesture: M,
}

impl GestureAction<()> {
    /// A presence-only gesture action (no message).
    pub fn new(name: &str, content: &str) -> Self {
        Self {
            name: name.to_string(),
            content: content.to_string(),
            on_gesture: (),
        }
    }
}

impl<M> GestureAction<M> {
    /// A gesture action carrying `on_gesture` to emit when struck.
    pub fn with_msg(name: &str, content: &str, on_gesture: M) -> Self {
        Self {
            name: name.to_string(),
            content: content.to_string(),
            on_gesture,
        }
    }

    /// Whether a striking pen gesture landed on this control's region: a
    /// non-highlighter stroke (or strokes) whose combined bounding box spans at
    /// least `STRIKE_WIDTH_RATIO` of the region width. Highlighter strokes are
    /// excluded, so a highlight never triggers the action.
    pub fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> bool {
        let Some(region) = manifest.regions.iter().find(|r| r.name == self.name) else {
            return false;
        };
        let region_w = region.rect.x1 - region.rect.x0;
        if region_w <= 0.0 {
            return false;
        }
        // Non-highlighter strokes attributed to this region with a point inside
        // the rect (the Checkbox two-stage filter), unioned into one bbox span so
        // a multi-stroke scribble is handled as a single gesture.
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        for p in ink
            .iter()
            .filter(|ri| ri.region == self.name)
            .flat_map(|ri| &ri.strokes)
            .filter(|s| !s.highlighter)
            .filter(|s| s.points.iter().any(|p| region.rect.contains(p.x, p.y)))
            .flat_map(|s| &s.points)
        {
            min_x = min_x.min(p.x);
            max_x = max_x.max(p.x);
        }
        if min_x > max_x {
            return false; // no qualifying pen strokes
        }
        (max_x - min_x) >= STRIKE_WIDTH_RATIO * region_w
    }
}

impl<M: Clone> Component for GestureAction<M> {
    type Msg = M;

    fn render(&self, _cx: &mut RenderCx) -> String {
        assert!(
            is_valid_region_name(&self.name),
            "gesture-action region name must be a valid region name, got: {:?}",
            self.name
        );
        let name = &self.name;
        let content = esc_typst_str(&self.content);
        // A non-breakable region: the default `#region` wraps the body in a box,
        // so recovery yields one rect whose width is the laid-out content width —
        // the span a strike must cover. The content is injected as a Typst string
        // expression (`#"..."`) so its markup chars stay literal.
        format!("#region(\"{name}\", [#\"{content}\"])\n")
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<M> {
        if self.read(ink, manifest) {
            vec![self.on_gesture.clone()]
        } else {
            vec![]
        }
    }
}
```

- [ ] **Step 4: Register the module**

In `crates/inkapp-core/src/components/mod.rs`, add the module declaration in alphabetical order (after `pub mod checkbox;`):

```rust
pub mod calendar_view;
pub mod checkbox;
pub mod gesture;
pub mod highlight_text;
pub mod notice;
pub mod passage;
pub mod stepper;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `nix develop -c cargo test -p inkapp-core --test gesture_action`
Expected: PASS (6 tests: fire, no-tap, no-highlighter, empty, render, end-to-end).

- [ ] **Step 6: Lint**

Run: `nix develop -c cargo clippy -p inkapp-core --all-targets -- -D warnings`
Expected: clean (no warnings).

- [ ] **Step 7: Commit** (do NOT stage `Cargo.lock`)

```bash
git add crates/inkapp-core/src/components/gesture.rs \
        crates/inkapp-core/src/components/mod.rs \
        crates/inkapp-core/tests/gesture_action.rs
git commit -m "inkapp-core: GestureAction component (strike-gesture → value-message Control)"
```

---

### Task 2: Harness exerciser with real captured fixtures

**Goal:** Prove the component against real recorded gestures: strike-through and scribble-out fire; empty, highlighter swipe, and checkmark do not — all driven through the device write/read byte path via `simulate`.

**Files:**
- Modify: `crates/inkapp-harness/tests/exercisers.rs`

**Acceptance Criteria:**
- [ ] `strike-through` and `scribble-out` fixtures fire the action.
- [ ] Empty scenario, `highlight-swipe` (wrong tool), and `checkmark` (pen, wrong shape) do not fire.

**Verify:** `nix develop -c cargo test -p inkapp-harness --test exercisers gesture_action_exerciser` → passes.

**Steps:**

- [ ] **Step 1: Add the import**

At the top of `crates/inkapp-harness/tests/exercisers.rs`, add (after the existing `use inkapp_core::components::...` lines):

```rust
use inkapp_core::components::gesture::GestureAction;
```

- [ ] **Step 2: Write the exerciser test (append to the file)**

```rust
#[test]
fn gesture_action_exerciser() {
    // M = &str keeps the test message trivial; we assert via `read`, not decode.
    let g = GestureAction::with_msg("title", "How CGI changed the web", "archive");
    let mut cx = RenderCx::new(0);
    let body = g.render(&mut cx);
    let src = format!("#set page(width: 420pt, height: 120pt, margin: 16pt)\n{body}");
    let doc = compile_to_document(&src).unwrap();
    let manifest = recover_regions(&doc).unwrap().with_version(1);
    let device = Remarkable::new();

    // Helper: run one fixture against the title region and report whether it fired.
    let fires = |fixture: &'static str| -> bool {
        let trace = simulate(
            &src,
            &manifest,
            &device,
            &Scenario::new().mark("title", Gesture::Fixture(fixture)),
        )
        .unwrap();
        g.read(&trace.readback, &manifest)
    };

    // Real striking gestures fire.
    assert!(fires("strike-through"), "a real strike-through fires the action");
    assert!(fires("scribble-out"), "a real scribble-out fires the action");

    // No ink: does not fire.
    let empty = simulate(&src, &manifest, &device, &Scenario::new()).unwrap();
    assert!(!g.read(&empty.readback, &manifest), "no ink does not fire");

    // Wrong tool: a highlighter swipe (spans the width but is a highlighter) does not fire.
    assert!(!fires("highlight-swipe"), "a highlighter swipe must not fire");

    // Wrong shape: a checkmark is a pen stroke but narrow (aspect-fit) — does not fire.
    assert!(!fires("checkmark"), "a checkmark must not fire (pen but not a strike)");
}
```

- [ ] **Step 3: Run the test**

Run: `nix develop -c cargo test -p inkapp-harness --test exercisers gesture_action_exerciser`
Expected: PASS.

- [ ] **Step 4: Lint**

Run: `nix develop -c cargo clippy -p inkapp-harness --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit** (do NOT stage `Cargo.lock`)

```bash
git add crates/inkapp-harness/tests/exercisers.rs
git commit -m "inkapp-harness: gesture_action_exerciser — real fixtures fire/no-fire/wrong-tool"
```

---

### Task 3: Reconcile `docs/appdx.md` (definition of done)

**Goal:** Make the developer-experience doc true by recording the shipped `GestureAction` Control component.

**Files:**
- Modify: `docs/appdx.md`

**Acceptance Criteria:**
- [ ] `docs/appdx.md` describes `GestureAction` as a built fixed-affordance Control that decodes a striking pen gesture into a value-message.

**Verify:** `grep -n "GestureAction" docs/appdx.md` → matches the new paragraph.

**Steps:**

- [ ] **Step 1: Add the paragraph**

In `docs/appdx.md`, immediately after the `Notice` Display paragraph (the one ending "…so it drops into any `view` flow."), insert a new paragraph:

```markdown
The framework also ships a `GestureAction` **Control** component — it renders its
target content as one region and decodes a *striking pen gesture* (a non-highlighter
stroke whose combined bounding box spans most of the region's width — a horizontal
strike or scribble) into a single value-message, while ignoring incidental marks,
taps, and highlighter swipes. It is how an app turns "strike through the title to
archive it" into one `Msg`; like `Checkbox` it is a fixed-affordance Control that
carries no mode. *(Built — `inkapp-core::components::gesture`, proved against real
captured gesture fixtures in the harness exerciser.)*
```

- [ ] **Step 2: Verify the whole workspace is green**

Run: `nix develop -c cargo test --workspace`
Expected: all tests pass.

- [ ] **Step 3: Format check + lint**

Run: `nix develop -c cargo fmt --check && nix develop -c cargo clippy --all-targets -- -D warnings`
Expected: clean (the pre-commit hook runs `cargo fmt --check`).

- [ ] **Step 4: Commit** (do NOT stage `Cargo.lock`)

```bash
git add docs/appdx.md
git commit -m "docs(appdx): record GestureAction Control component as built"
```

---

## Self-Review

**Spec coverage:**
- Component `GestureAction<M>` with value-message (`new`/`with_msg`, no closures) → Task 1. ✓
- Render shows target content as its own region → Task 1 Step 3 `render`. ✓
- Decode = non-highlighter stroke spanning most of region width; taps/incidental/highlighter excluded → Task 1 `read` + unit tests. ✓
- Register in `components/mod.rs` → Task 1 Step 4. ✓
- Wire fixtures (strike-through/scribble-out fire; empty/highlight-swipe no-fire) through `simulate` → Task 2. Plus checkmark (pen, wrong shape) as a bonus specificity case. ✓
- Simulator fixture-replay variant → already exists (`Gesture::Fixture`); no change, noted in File Structure. ✓
- Render→recover→decode round trip → Task 1 `gesture_action_decodes_strike_end_to_end`. ✓
- `cargo test --workspace` green → Task 3 Step 2. ✓
- Don't stage `Cargo.lock` → every commit step. ✓
- Record in `docs/appdx.md` as final step → Task 3. ✓

**Placeholder scan:** none — every step has concrete code/commands.

**Type consistency:** `GestureAction::with_msg(name, content, on_gesture)`, `read(&self, &[RegionInk], &Manifest) -> bool`, `STRIKE_WIDTH_RATIO`, `Gesture::Fixture(&'static str)`, `Region { name, page, rect }`, `Manifest { version, regions, ..Default::default() }` — consistent across Tasks 1–3 and matched to the existing `checkbox.rs`/`passage.rs`/`simulator.rs` signatures read from the codebase.
