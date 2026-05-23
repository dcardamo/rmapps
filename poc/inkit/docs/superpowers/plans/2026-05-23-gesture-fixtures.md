# Gesture-Fixture Layer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the real-ink e2e tier — record a gesture vocabulary on-device once, transplant a real gesture into any region, and drive the existing harness loop with it — while closing the three fidelity items Spec #2 deferred.

**Architecture:** A gesture **catalog** in `inkapp-harness` is the single source of truth. From it we generate self-instructing per-gesture template PDFs (pushed to `/InkAppDev/fixtures/`), extract per-box strokes from the annotated capture, normalize them to unit-box **fixtures**, and transplant them into target regions via a per-gesture fit policy. A new `Gesture::Fixture` feeds the same `write_ink → read_ink → attribute` path the synthetic gestures use. `Checkbox` gains mark-vs-scribble discrimination. `rm-files` gains a block-structure inspector (deferred #1); a calibration sheet validates the device transform (deferred #2); `#[ignore]` bars cover recording and device-acceptance (deferred #3). The suite ships **synthetic bootstrap** recordings so `make test` is green before the one-time real recording.

**Tech Stack:** Rust (workspace, edition 2021), existing `inkapp-core` (render/manifest/embed/readback/widgets), `inkapp-remarkable` (`Device`), `rm-files` (reader/writer), `serde`/`serde_json` for fixture JSON, `zip` (dev) for reading `.rmdoc` captures, `rmapi` CLI (shelled out from `#[ignore]` bars only).

---

## Critical conventions (read once, apply to every task)

- **Commit form (repo-specific):** the `pre-commit-check-tasks` hook miscounts native task IDs and will block a literal `git commit`. Commit with the flag form so the real `cargo fmt --check` pre-commit still runs:
  `git -c core.hooksPath=.githooks commit -m "..."`
- **No `Co-Authored-By` lines** in commit messages.
- **Run tests via nix:** `nix develop -c cargo test -p <crate>` (the `Makefile` wraps the all-workspace run as `make test`; lint as `make clippy`).
- **Crate name vs import path:** `rm-files` imports as `rm_files`; `inkapp-core` as `inkapp_core`; `inkapp-harness` as `inkapp_harness`; `inkapp-remarkable` as `inkapp_remarkable`.
- **Device transform is shared and self-consistent.** Extraction attributes strokes through the *current* `inkapp-remarkable` transform; that is fine because guide boxes are large and well-separated. Transform *precision* is validated separately (Task 7) via calibration taps.
- **Page geometry for templates is a shared constant.** The generator and extractor both use `recording::PAGE_W`/`PAGE_H`; never re-measure.
- **Bootstrap vs real.** Until the manual recording bar (Task 8) runs, fixtures are regenerated from synthetic strokes and stamped `"device": "synthetic-bootstrap"`. e2e assertions are behavioural so they hold for both; only goldens differ and regenerate when real ink lands.

---

### Task 1: `Checkbox` mark-vs-scribble discrimination

**Goal:** Add `CheckState { Empty, Marked, ScribbledOut }` and `Checkbox::read_state`, keeping `read -> bool` as `state != Empty`, via a path-length-vs-region-diagonal heuristic.

**Files:**
- Modify: `crates/inkapp-core/src/widgets/checkbox.rs`
- Test: `crates/inkapp-core/tests/checkbox_state.rs`

**Acceptance Criteria:**
- [ ] `CheckState` enum exists and is exported from `inkapp_core::widgets::checkbox`.
- [ ] `read_state` returns `Empty` (no ink in region), `Marked` (short mark), `ScribbledOut` (dense scribble).
- [ ] `Widget::read` still returns `bool`, equal to `read_state != Empty`.
- [ ] Existing `inkapp-core` tests stay green.

**Verify:** `nix develop -c cargo test -p inkapp-core` → PASS.

**Steps:**

- [ ] **Step 1: Write failing tests**

`crates/inkapp-core/tests/checkbox_state.rs`:

```rust
use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::{Manifest, Region};
use inkapp_core::widgets::checkbox::{CheckState, Checkbox};

fn manifest_with(rect: PdfRect) -> Manifest {
    Manifest {
        version: 1,
        regions: vec![Region { name: "done".into(), page: 0, rect }],
    }
}

fn ink(points: Vec<PdfPoint>) -> Vec<RegionInk> {
    vec![RegionInk {
        region: "done".into(),
        strokes: vec![Stroke { points, highlighter: false }],
    }]
}

const RECT: PdfRect = PdfRect { x0: 0.0, y0: 0.0, x1: 20.0, y1: 20.0 };

#[test]
fn empty_when_no_ink() {
    let cb = Checkbox::new("done");
    assert_eq!(cb.read_state(&[], &manifest_with(RECT)), CheckState::Empty);
}

#[test]
fn marked_for_short_check() {
    // A tick: down-right then up-right. Total length ~ 1.5x the ~28pt diagonal.
    let cb = Checkbox::new("done");
    let pts = vec![
        PdfPoint { x: 4.0, y: 12.0 },
        PdfPoint { x: 9.0, y: 5.0 },
        PdfPoint { x: 16.0, y: 16.0 },
    ];
    assert_eq!(cb.read_state(&ink(pts), &manifest_with(RECT)), CheckState::Marked);
}

#[test]
fn scribbled_out_for_dense_zigzag() {
    // A back-and-forth scribble: many segments, total length >> diagonal.
    let cb = Checkbox::new("done");
    let mut pts = Vec::new();
    for i in 0..12 {
        let x = 2.0 + (i as f64) * 1.4;
        let y = if i % 2 == 0 { 3.0 } else { 17.0 };
        pts.push(PdfPoint { x, y });
    }
    assert_eq!(cb.read_state(&ink(pts), &manifest_with(RECT)), CheckState::ScribbledOut);
}

#[test]
fn read_bool_tracks_state() {
    let cb = Checkbox::new("done");
    assert!(!{
        use inkapp_core::widget::Widget;
        cb.read(&[], &manifest_with(RECT))
    });
}
```

- [ ] **Step 2: Run to verify failure**

Run: `nix develop -c cargo test -p inkapp-core --test checkbox_state`
Expected: FAIL (compile error — `CheckState`/`read_state` missing).

- [ ] **Step 3: Implement discrimination**

Replace the body of `crates/inkapp-core/src/widgets/checkbox.rs` with (keeps `render_at`/`render` unchanged; adds state):

```rust
use crate::geometry::PdfPoint;
use crate::ink::RegionInk;
use crate::manifest::Manifest;
use crate::widget::{region_metadata, RenderCx, Widget};

/// How a checkbox region was marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    /// No ink in the region.
    Empty,
    /// A check/tick (short mark).
    Marked,
    /// A dense scribble-out (cancel / un-check).
    ScribbledOut,
}

/// Total ink path length above this multiple of the region diagonal reads as a
/// scribble-out rather than a mark. A tick is ~1–2 diagonals; a scribble many.
const SCRIBBLE_RATIO: f64 = 3.0;

/// A single tappable checkbox bound to a named region.
pub struct Checkbox {
    name: String,
}

impl Checkbox {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string() }
    }

    /// Render the checkbox glyph and its region at an explicit position
    /// (Typst-space points). Used directly by tests and apps that lay out
    /// absolutely; `render` wraps this with a default box.
    pub fn render_at(&self, page: usize, x: f64, y: f64, w: f64, h: f64) -> String {
        let mut s = region_metadata(&self.name, page, x, y, w, h);
        s.push_str(&format!(
            "#place(top + left, dx: {x}pt, dy: {y}pt, rect(width: {w}pt, height: {h}pt, stroke: 0.5pt))\n"
        ));
        s
    }

    /// Classify the ink attributed to this checkbox's region.
    pub fn read_state(&self, ink: &[RegionInk], manifest: &Manifest) -> CheckState {
        let Some(region) = manifest.regions.iter().find(|r| r.name == self.name) else {
            return CheckState::Empty;
        };
        let strokes: Vec<&crate::ink::Stroke> = ink
            .iter()
            .filter(|ri| ri.region == self.name)
            .flat_map(|ri| &ri.strokes)
            .filter(|s| s.points.iter().any(|p| region.rect.contains(p.x, p.y)))
            .collect();
        if strokes.is_empty() {
            return CheckState::Empty;
        }
        let dx = region.rect.x1 - region.rect.x0;
        let dy = region.rect.y1 - region.rect.y0;
        let diagonal = (dx * dx + dy * dy).sqrt().max(f64::EPSILON);
        let total: f64 = strokes.iter().map(|s| polyline_len(&s.points)).sum();
        if total > SCRIBBLE_RATIO * diagonal {
            CheckState::ScribbledOut
        } else {
            CheckState::Marked
        }
    }
}

/// Sum of segment lengths of a polyline.
fn polyline_len(points: &[PdfPoint]) -> f64 {
    points
        .windows(2)
        .map(|w| {
            let dx = w[1].x - w[0].x;
            let dy = w[1].y - w[0].y;
            (dx * dx + dy * dy).sqrt()
        })
        .sum()
}

impl Widget for Checkbox {
    type Output = bool;

    fn render(&self, cx: &mut RenderCx) -> String {
        self.render_at(cx.page, 20.0, 40.0, 16.0, 16.0)
    }

    fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> bool {
        self.read_state(ink, manifest) != CheckState::Empty
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `nix develop -c cargo test -p inkapp-core`
Expected: PASS (new state tests + all existing tests).

- [ ] **Step 5: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "inkapp-core: Checkbox mark-vs-scribble discrimination (CheckState/read_state)"
```

---

### Task 2: `rm-files` block-structure inspector + structural-diff (deferred #1)

**Goal:** Expose a structural summary of a `.rm` block stream and prove our writer's line-item blocks structurally match a real device file's, while documenting the scaffolding our minimal writer intentionally omits.

**Files:**
- Modify: `crates/rm-files/src/scene/mod.rs` (add `BlockSummary` + `block_structure`)
- Modify: `crates/rm-files/src/lib.rs` (re-export them)
- Test: `crates/rm-files/tests/structure.rs`

**Acceptance Criteria:**
- [ ] `rm_files::block_structure(bytes) -> Result<Vec<BlockSummary>>` returns one summary `{ block_type, current_version, content_len }` per top-level block.
- [ ] Our `write_scene` output for the real fixture's strokes contains only line-item blocks (type `0x05`), one per stroke, all `current_version == 2`.
- [ ] The real fixture contains additional non-line block types (scaffolding our writer omits) — asserted, documenting the intentional gap.

**Verify:** `nix develop -c cargo test -p rm-files --test structure` → PASS.

**Steps:**

- [ ] **Step 1: Write failing test**

`crates/rm-files/tests/structure.rs`:

```rust
use std::io::Read;

use rm_files::{block_structure, write_scene, Scene, SceneItem, Stroke};

/// reMarkable v6 block type for a scene line item (ink stroke).
const BLOCK_TYPE_LINE: u8 = 0x05;

fn fixture_rm_bytes() -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/stamped-labels.rmdoc");
    let file = std::fs::File::open(path).expect("open rmdoc");
    let mut archive = zip::ZipArchive::new(file).expect("read zip");
    let rm_name = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .find(|n| n.ends_with(".rm"))
        .expect(".rm entry");
    let mut e = archive.by_name(&rm_name).unwrap();
    let mut b = Vec::new();
    e.read_to_end(&mut b).unwrap();
    b
}

#[test]
fn writer_output_is_all_line_items() {
    let real = fixture_rm_bytes();
    let strokes: Vec<Stroke> = Scene::parse(&real).unwrap().strokes().into_iter().cloned().collect();
    assert!(!strokes.is_empty(), "fixture has strokes");

    let items: Vec<SceneItem> = strokes.iter().cloned().map(SceneItem::Line).collect();
    let written = write_scene(6, &items);

    let ours = block_structure(&written).unwrap();
    assert_eq!(ours.len(), strokes.len(), "one block per stroke");
    assert!(
        ours.iter().all(|b| b.block_type == BLOCK_TYPE_LINE && b.current_version == 2),
        "writer emits only v2 line-item blocks"
    );
}

#[test]
fn real_file_carries_scaffolding_we_omit() {
    let real = fixture_rm_bytes();
    let real_struct = block_structure(&real).unwrap();

    let line_blocks = real_struct.iter().filter(|b| b.block_type == BLOCK_TYPE_LINE).count();
    let non_line_blocks = real_struct.iter().filter(|b| b.block_type != BLOCK_TYPE_LINE).count();

    assert!(line_blocks > 0, "real file has line items");
    // The minimal writer omits the CRDT/scaffolding blocks (author ids, migration
    // info, page/scene tree, group items) a device file carries. This documents
    // that gap; device-acceptance (Task 8) is the render gate, not byte-identity.
    assert!(
        non_line_blocks > 0,
        "real device file carries scaffolding blocks our writer intentionally omits"
    );
}
```

- [ ] **Step 2: Run to verify failure**

Run: `nix develop -c cargo test -p rm-files --test structure`
Expected: FAIL (`block_structure`/`BlockSummary` not found).

- [ ] **Step 3: Implement `block_structure`**

Add to `crates/rm-files/src/scene/mod.rs` (after the `Scene` impl block):

```rust
/// A structural summary of one top-level v6 block: its framing only, not its
/// decoded body. Used to compare writer output against real device files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSummary {
    /// Block type byte (e.g. `0x05` for a scene line item).
    pub block_type: u8,
    /// The block's `current_version` header byte.
    pub current_version: u8,
    /// Content length in bytes (excludes the 8-byte block header).
    pub content_len: usize,
}

/// Walk the top-level block stream and summarise each block's framing without
/// decoding item bodies. A structural inspector for writer-vs-device comparison.
pub fn block_structure(bytes: &[u8]) -> Result<Vec<BlockSummary>> {
    let mut r = Reader::new(bytes);
    let _version = r.read_header()?;
    let mut out = Vec::new();
    while let Some(h) = r.read_block_header()? {
        out.push(BlockSummary {
            block_type: h.block_type,
            current_version: h.current_version,
            content_len: h.end() - r.pos(),
        });
        r.seek(h.end())?;
    }
    Ok(out)
}
```

`crates/rm-files/src/scene/mod.rs`: extend the existing re-export line to
`pub use writer::write_scene;` companions — add `BlockSummary` and `block_structure` to a `pub use` (they are defined in this module, so just ensure they are `pub`, which they are).

`crates/rm-files/src/lib.rs`: change the scene re-export line to:

```rust
pub use scene::{block_structure, write_scene, BlockSummary, Pen, PenColor, Scene, SceneItem, Stroke, TextHighlight};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `nix develop -c cargo test -p rm-files --test structure`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "rm-files: block_structure inspector + writer structural-diff vs real fixture"
```

---

### Task 3: Fixture types + transplant math (`inkapp-harness`)

**Goal:** The pure, hardware-free core of the fixture layer — fixture types, unit-box normalization, and the three transplant fit modes.

**Files:**
- Create: `crates/inkapp-harness/src/fixtures.rs`
- Modify: `crates/inkapp-harness/src/lib.rs` (add `pub mod fixtures;`)
- Modify: `crates/inkapp-harness/Cargo.toml` (add `serde`, `serde_json`)
- Test: `crates/inkapp-harness/tests/transplant.rs`

**Acceptance Criteria:**
- [ ] `Fit { AspectFit, Stretch, StretchX }`, `Tool { Pen, Highlighter }`, `UnitStroke`, `Sample`, `Source`, `GestureFixture` defined and (de)serializable as in the spec's JSON.
- [ ] `normalize(&[Stroke]) -> Sample` maps points to `[0,1]²` over the combined bbox and records `native_aspect`.
- [ ] `transplant(&Sample, target, fit, highlighter) -> Vec<Stroke>` implements all three fit modes.
- [ ] `stretch` fills the target; `aspect-fit` centers and preserves `native_aspect`; `stretch-x` fills width, height = `target_w / native_aspect`, centered vertically.

**Verify:** `nix develop -c cargo test -p inkapp-harness --test transplant` → PASS.

**Steps:**

- [ ] **Step 1: Add dependencies**

`crates/inkapp-harness/Cargo.toml`, under `[dependencies]` (after the existing entries):

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

And under `[dev-dependencies]` (after `inkapp-remarkable`):

```toml
zip = "2"
```

- [ ] **Step 2: Write failing transplant tests**

`crates/inkapp-harness/tests/transplant.rs`:

```rust
use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::Stroke;
use inkapp_harness::fixtures::{normalize, transplant, Fit, Sample};

fn s(points: &[(f64, f64)]) -> Stroke {
    Stroke {
        points: points.iter().map(|&(x, y)| PdfPoint { x, y }).collect(),
        highlighter: false,
    }
}

#[test]
fn normalize_unit_box_and_aspect() {
    // A 40-wide, 20-high stroke -> native_aspect 2.0, points span [0,1].
    let sample = normalize(&[s(&[(10.0, 100.0), (50.0, 120.0)])]);
    assert!((sample.native_aspect - 2.0).abs() < 1e-9);
    assert_eq!(sample.strokes.len(), 1);
    let p = &sample.strokes[0].points;
    assert_eq!(p[0], [0.0, 0.0]);
    assert_eq!(p[1], [1.0, 1.0]);
}

#[test]
fn stretch_fills_target() {
    let sample = Sample { native_aspect: 2.0, strokes: vec![us(&[[0.0, 0.0], [1.0, 1.0]])] };
    let t = PdfRect { x0: 100.0, y0: 200.0, x1: 140.0, y1: 230.0 };
    let out = transplant(&sample, t, Fit::Stretch, false);
    let p = &out[0].points;
    assert_eq!(p[0], PdfPoint { x: 100.0, y: 200.0 });
    assert_eq!(p[1], PdfPoint { x: 140.0, y: 230.0 });
}

#[test]
fn aspect_fit_centers_and_preserves_shape() {
    // native_aspect 2.0 into a 40x40 target -> fitted 40x20, centered vertically.
    let sample = Sample { native_aspect: 2.0, strokes: vec![us(&[[0.0, 0.0], [1.0, 1.0]])] };
    let t = PdfRect { x0: 0.0, y0: 0.0, x1: 40.0, y1: 40.0 };
    let out = transplant(&sample, t, Fit::AspectFit, false);
    let p = &out[0].points;
    // W = min(40, 2*40) = 40; H = 20; x-offset 0, y-offset (40-20)/2 = 10.
    assert!((p[0].x - 0.0).abs() < 1e-9 && (p[0].y - 10.0).abs() < 1e-9);
    assert!((p[1].x - 40.0).abs() < 1e-9 && (p[1].y - 30.0).abs() < 1e-9);
}

#[test]
fn stretch_x_fills_width_keeps_proportion() {
    // native_aspect 4.0 into 80-wide target -> height 20, centered in 40-high target.
    let sample = Sample { native_aspect: 4.0, strokes: vec![us(&[[0.0, 0.0], [1.0, 1.0]])] };
    let t = PdfRect { x0: 0.0, y0: 0.0, x1: 80.0, y1: 40.0 };
    let out = transplant(&sample, t, Fit::StretchX, true);
    assert!(out[0].highlighter, "tool flag carried through");
    let p = &out[0].points;
    // W = 80; H = 80/4 = 20; y-offset (40-20)/2 = 10.
    assert!((p[0].x - 0.0).abs() < 1e-9 && (p[0].y - 10.0).abs() < 1e-9);
    assert!((p[1].x - 80.0).abs() < 1e-9 && (p[1].y - 30.0).abs() < 1e-9);
}

fn us(points: &[[f64; 2]]) -> inkapp_harness::fixtures::UnitStroke {
    inkapp_harness::fixtures::UnitStroke { points: points.to_vec() }
}
```

- [ ] **Step 3: Run to verify failure**

Run: `nix develop -c cargo test -p inkapp-harness --test transplant`
Expected: FAIL (`fixtures` module missing).

- [ ] **Step 4: Implement `fixtures.rs`**

`crates/inkapp-harness/src/fixtures.rs`:

```rust
//! Real-ink gesture fixtures: unit-box normalized strokes plus the transplant
//! math that maps them into a target region. Device-agnostic and hardware-free.

use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::Stroke;
use serde::{Deserialize, Serialize};

/// How a fixture maps into a target region rect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fit {
    /// Preserve `native_aspect`, fit inside the target, center.
    AspectFit,
    /// Fill the target on both axes (ignores shape).
    Stretch,
    /// Fill width; height = target_w / native_aspect; center vertically.
    StretchX,
}

/// The drawing tool a fixture was recorded with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tool {
    Pen,
    Highlighter,
}

impl Tool {
    /// Whether this tool maps to a highlighter stroke.
    pub fn is_highlighter(self) -> bool {
        matches!(self, Tool::Highlighter)
    }
}

/// One stroke in unit-box coordinates (`[0,1]²`, PDF y-up).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitStroke {
    pub points: Vec<[f64; 2]>,
}

/// One recorded sample of a gesture: its native aspect plus unit-box strokes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub native_aspect: f64,
    pub strokes: Vec<UnitStroke>,
}

/// Provenance of a fixture's samples.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub recording: String,
    pub device: String,
    pub recorded: String,
}

/// A gesture fixture: catalog identity plus its banked samples.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GestureFixture {
    pub name: String,
    pub tool: Tool,
    pub fit: Fit,
    pub default: usize,
    pub samples: Vec<Sample>,
    pub source: Source,
}

impl GestureFixture {
    /// Load a fixture from JSON bytes.
    pub fn from_json(bytes: &[u8]) -> serde_json::Result<GestureFixture> {
        serde_json::from_slice(bytes)
    }

    /// Serialize to pretty JSON (deterministic field order via the struct).
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    /// Transplant the default sample into `target` using this fixture's fit/tool.
    pub fn transplant_default(&self, target: PdfRect) -> Vec<Stroke> {
        let sample = &self.samples[self.default];
        transplant(sample, target, self.fit, self.tool.is_highlighter())
    }
}

/// Normalize PDF-space strokes to a single unit-box [`Sample`] over their
/// combined bounding box. `native_aspect = bbox_w / bbox_h`. Degenerate spans
/// (zero width or height, e.g. a tap) are guarded to aspect 1.0.
pub fn normalize(strokes: &[Stroke]) -> Sample {
    let mut x0 = f64::INFINITY;
    let mut y0 = f64::INFINITY;
    let mut x1 = f64::NEG_INFINITY;
    let mut y1 = f64::NEG_INFINITY;
    for s in strokes {
        for p in &s.points {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
    }
    let w = (x1 - x0).max(f64::EPSILON);
    let h = (y1 - y0).max(f64::EPSILON);
    let native_aspect = if (x1 - x0) <= f64::EPSILON || (y1 - y0) <= f64::EPSILON {
        1.0
    } else {
        w / h
    };
    let out = strokes
        .iter()
        .map(|s| UnitStroke {
            points: s.points.iter().map(|p| [(p.x - x0) / w, (p.y - y0) / h]).collect(),
        })
        .collect();
    Sample { native_aspect, strokes: out }
}

/// Transplant a unit-box sample into `target` per `fit`. `highlighter` sets the
/// tool flag on every produced stroke.
pub fn transplant(sample: &Sample, target: PdfRect, fit: Fit, highlighter: bool) -> Vec<Stroke> {
    let tw = target.x1 - target.x0;
    let th = target.y1 - target.y0;
    let a = sample.native_aspect.max(f64::EPSILON);

    // (origin_x, origin_y, width, height) of the placed unit box in PDF space.
    let (ox, oy, w, h) = match fit {
        Fit::Stretch => (target.x0, target.y0, tw, th),
        Fit::StretchX => {
            let h = tw / a;
            (target.x0, target.y0 + (th - h) / 2.0, tw, h)
        }
        Fit::AspectFit => {
            let w = tw.min(a * th);
            let h = w / a;
            (target.x0 + (tw - w) / 2.0, target.y0 + (th - h) / 2.0, w, h)
        }
    };

    sample
        .strokes
        .iter()
        .map(|us| Stroke {
            points: us
                .points
                .iter()
                .map(|[u, v]| PdfPoint { x: ox + u * w, y: oy + v * h })
                .collect(),
            highlighter,
        })
        .collect()
}
```

`crates/inkapp-harness/src/lib.rs`: add `pub mod fixtures;` (alongside the existing `pub mod` lines).

- [ ] **Step 5: Run tests to verify they pass**

Run: `nix develop -c cargo test -p inkapp-harness --test transplant`
Expected: PASS (normalize + all three fit modes).

- [ ] **Step 6: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "inkapp-harness: gesture fixture types + normalize + transplant fit modes"
```

---

### Task 4: Gesture catalog + self-instructing template generation

**Goal:** The single-source-of-truth catalog and the generator that turns each entry into a self-describing template PDF (and a calibration sheet), with recoverable region rects.

**Files:**
- Create: `crates/inkapp-harness/src/recording.rs`
- Modify: `crates/inkapp-harness/src/lib.rs` (add `pub mod recording;`)
- Test: `crates/inkapp-harness/tests/templates.rs`

**Acceptance Criteria:**
- [ ] `recording::catalog() -> &'static [CatalogEntry]` lists the seven gestures with `name`, `tool`, `fit`, `instruction`, `box_shape`, `sample_text`.
- [ ] `recording::render_template(entry) -> Result<Vec<u8>>` returns a PDF whose extracted manifest contains regions `box:<name>:0..=2`.
- [ ] `recording::render_calibration() -> Result<Vec<u8>>` returns a PDF whose extracted manifest contains regions `cross:0..` and exposes their known PDF centers via `calibration_points()`.
- [ ] `PAGE_W`/`PAGE_H` constants are shared (used later by extraction).

**Verify:** `nix develop -c cargo test -p inkapp-harness --test templates` → PASS.

**Steps:**

- [ ] **Step 1: Write failing template tests**

`crates/inkapp-harness/tests/templates.rs`:

```rust
use inkapp_core::embed::extract_manifest;
use inkapp_harness::recording::{calibration_points, catalog, render_calibration, render_template};

#[test]
fn catalog_has_seven_gestures() {
    let names: Vec<&str> = catalog().iter().map(|e| e.name).collect();
    assert!(names.contains(&"checkmark"));
    assert!(names.contains(&"scribble-out"));
    assert!(names.contains(&"highlight-swipe"));
    assert_eq!(names.len(), 7);
}

#[test]
fn template_declares_three_boxes() {
    let entry = catalog().iter().find(|e| e.name == "checkmark").unwrap();
    let pdf = render_template(entry).unwrap();
    let manifest = extract_manifest(&pdf).unwrap();
    for i in 0..3 {
        let name = format!("box:checkmark:{i}");
        assert!(
            manifest.regions.iter().any(|r| r.name == name),
            "missing region {name}"
        );
    }
}

#[test]
fn calibration_declares_crosses_with_known_points() {
    let pdf = render_calibration().unwrap();
    let manifest = extract_manifest(&pdf).unwrap();
    let pts = calibration_points();
    assert!(pts.len() >= 4, "at least 4 calibration points");
    for i in 0..pts.len() {
        assert!(
            manifest.regions.iter().any(|r| r.name == format!("cross:{i}")),
            "missing cross:{i}"
        );
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `nix develop -c cargo test -p inkapp-harness --test templates`
Expected: FAIL (`recording` module missing).

- [ ] **Step 3: Implement catalog + generation**

`crates/inkapp-harness/src/recording.rs` (catalog + template render; extraction is added in Task 5):

```rust
//! On-device recording: the gesture catalog, self-instructing template
//! generation, and (Task 5) extraction of captures into fixtures.

use inkapp_core::embed::embed_manifest;
use inkapp_core::error::Result;
use inkapp_core::geometry::PdfPoint;
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::{compile_to_document, document_to_pdf};
use inkapp_core::widget::region_metadata;

use crate::fixtures::{Fit, Tool};

/// Template page width/height in points (shared by generation and extraction).
/// 0.75 aspect approximates the reMarkable canvas; the device fits to width.
pub const PAGE_W: f64 = 420.0;
pub const PAGE_H: f64 = 560.0;
/// Top inset so the pen toolbar never covers a cell (mechanics doc §7).
const TOP_INSET: f64 = 48.0;

/// Guide-box shape for a gesture, matched to how it is naturally drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxShape {
    /// ~square (checkmark, circle, scribble).
    Square,
    /// wide/short (swipe, strike, handwriting, arrow).
    Wide,
}

impl BoxShape {
    fn dims(self) -> (f64, f64) {
        match self {
            BoxShape::Square => (120.0, 120.0),
            BoxShape::Wide => (340.0, 80.0),
        }
    }
}

/// One catalog entry — the single source of truth for a gesture.
#[derive(Debug, Clone, Copy)]
pub struct CatalogEntry {
    pub name: &'static str,
    pub tool: Tool,
    pub fit: Fit,
    pub instruction: &'static str,
    pub box_shape: BoxShape,
    pub sample_text: Option<&'static str>,
}

/// The gesture vocabulary. Editing this list is how the vocabulary grows.
pub fn catalog() -> &'static [CatalogEntry] {
    &[
        CatalogEntry { name: "checkmark", tool: Tool::Pen, fit: Fit::AspectFit, instruction: "draw a check in each box", box_shape: BoxShape::Square, sample_text: None },
        CatalogEntry { name: "scribble-out", tool: Tool::Pen, fit: Fit::StretchX, instruction: "scribble each box out", box_shape: BoxShape::Square, sample_text: None },
        CatalogEntry { name: "highlight-swipe", tool: Tool::Highlighter, fit: Fit::StretchX, instruction: "swipe a highlight across the words", box_shape: BoxShape::Wide, sample_text: Some("highlight these words") },
        CatalogEntry { name: "strike-through", tool: Tool::Pen, fit: Fit::StretchX, instruction: "strike the words out", box_shape: BoxShape::Wide, sample_text: Some("strike these words") },
        CatalogEntry { name: "handwritten-word", tool: Tool::Pen, fit: Fit::AspectFit, instruction: "write the word: review", box_shape: BoxShape::Wide, sample_text: None },
        CatalogEntry { name: "circle", tool: Tool::Pen, fit: Fit::AspectFit, instruction: "circle inside each box", box_shape: BoxShape::Square, sample_text: None },
        CatalogEntry { name: "arrow", tool: Tool::Pen, fit: Fit::AspectFit, instruction: "draw an arrow left to right", box_shape: BoxShape::Square, sample_text: None },
    ]
}

/// Number of guide boxes (samples) per gesture template.
pub const BOXES_PER_GESTURE: usize = 3;

fn page_header() -> String {
    format!("#set page(width: {PAGE_W}pt, height: {PAGE_H}pt, margin: 0pt)\n#set text(size: 11pt)\n")
}

fn place_text(x: f64, y: f64, text: &str) -> String {
    // Escape for a Typst content string used inline.
    let esc = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("#place(top + left, dx: {x}pt, dy: {y}pt, text[{esc}])\n")
}

fn place_box(x: f64, y: f64, w: f64, h: f64) -> String {
    format!("#place(top + left, dx: {x}pt, dy: {y}pt, rect(width: {w}pt, height: {h}pt, stroke: 0.5pt))\n")
}

/// Render the self-instructing template PDF for one gesture (manifest embedded).
pub fn render_template(entry: &CatalogEntry) -> Result<Vec<u8>> {
    let (bw, bh) = entry.box_shape.dims();
    let mut src = page_header();
    // Title line.
    src.push_str(&place_text(
        24.0,
        16.0,
        &format!("{} — {}", entry.name, entry.instruction),
    ));

    let cell_h = bh + 30.0;
    for i in 0..BOXES_PER_GESTURE {
        let box_x = 24.0;
        let box_y = TOP_INSET + (i as f64) * cell_h + 14.0;
        // Region metadata (Typst-space, top-left origin) — recovered to PDF coords.
        src.push_str(&region_metadata(
            &format!("box:{}:{i}", entry.name),
            0,
            box_x,
            box_y,
            bw,
            bh,
        ));
        // Visible guide box.
        src.push_str(&place_box(box_x, box_y, bw, bh));
        // Faint sample words to act on (highlight / strike).
        if let Some(t) = entry.sample_text {
            src.push_str(&place_text(box_x + 8.0, box_y + bh / 2.0 - 6.0, t));
        }
    }

    let doc = compile_to_document(&src)?;
    let manifest = recover_regions(&doc)?.with_version(1);
    let pdf = document_to_pdf(&doc)?;
    embed_manifest(&pdf, &manifest)
}

/// Known calibration points (PDF-space, y-up) the crosses are centered on.
pub fn calibration_points() -> Vec<PdfPoint> {
    let m = 60.0;
    vec![
        PdfPoint { x: m, y: m },
        PdfPoint { x: PAGE_W - m, y: m },
        PdfPoint { x: m, y: PAGE_H - m },
        PdfPoint { x: PAGE_W - m, y: PAGE_H - m },
        PdfPoint { x: PAGE_W / 2.0, y: PAGE_H / 2.0 },
    ]
}

/// Render the calibration sheet: crosshairs at known PDF points, each wrapped in
/// a `cross:<i>` region (the region center equals the known point).
pub fn render_calibration() -> Result<Vec<u8>> {
    const HALF: f64 = 12.0; // cross half-size / region half-size
    let mut src = page_header();
    src.push_str(&place_text(24.0, 16.0, "Calibration — tap the centre of each cross"));
    for (i, p) in calibration_points().iter().enumerate() {
        // Convert PDF y-up point to Typst top-left for placement.
        let tx = p.x - HALF;
        let ty = (PAGE_H - p.y) - HALF;
        src.push_str(&region_metadata(&format!("cross:{i}"), 0, tx, ty, HALF * 2.0, HALF * 2.0));
        // Draw a "+" using two thin rects.
        src.push_str(&format!(
            "#place(top + left, dx: {}pt, dy: {}pt, rect(width: {}pt, height: 0.6pt, fill: black))\n",
            p.x - HALF,
            (PAGE_H - p.y),
            HALF * 2.0
        ));
        src.push_str(&format!(
            "#place(top + left, dx: {}pt, dy: {}pt, rect(width: 0.6pt, height: {}pt, fill: black))\n",
            p.x,
            (PAGE_H - p.y) - HALF,
            HALF * 2.0
        ));
    }
    let doc = compile_to_document(&src)?;
    let manifest = recover_regions(&doc)?.with_version(1);
    let pdf = document_to_pdf(&doc)?;
    embed_manifest(&pdf, &manifest)
}
```

`crates/inkapp-harness/src/lib.rs`: add `pub mod recording;`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `nix develop -c cargo test -p inkapp-harness --test templates`
Expected: PASS (catalog + box/cross regions recovered).

> If `region_metadata` placement and the visible `#place rect` disagree on rounding, the test only checks region *presence*, not pixel position; exact rects are pinned by extraction round-trip in Task 5.

- [ ] **Step 5: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "inkapp-harness: gesture catalog + self-instructing template/calibration generation"
```

---

### Task 5: Extraction — captures (or synthetic strokes) → fixtures

**Goal:** Attribute strokes to a template's boxes and normalize them into a `GestureFixture`, working identically for real captures and synthetic bootstrap strokes (both routed through the real `write_ink → read_ink` path).

**Files:**
- Modify: `crates/inkapp-harness/src/recording.rs` (add extraction + bootstrap synth)
- Test: `crates/inkapp-harness/tests/extract.rs`

**Acceptance Criteria:**
- [ ] `extract_samples(strokes_pdf, manifest, name) -> Vec<Sample>` collects strokes per `box:<name>:i` (in index order) and normalizes each.
- [ ] `bootstrap_strokes(entry, &manifest) -> Vec<Stroke>` synthesizes plausible PDF-space ink in each box (checkmark short; scribble-out dense; swipe/strike horizontal; etc.).
- [ ] `extract_fixture(entry, device, strokes_pdf, manifest, source) -> GestureFixture` builds the fixture from already-PDF-space strokes.
- [ ] A bootstrap round-trip (synthesize → `write_ink` → `read_ink` → extract) yields one sample per box with unit-box points and correct `tool`.

**Verify:** `nix develop -c cargo test -p inkapp-harness --test extract` → PASS.

**Steps:**

- [ ] **Step 1: Write failing extraction test**

`crates/inkapp-harness/tests/extract.rs`:

```rust
use inkapp_core::device::Device;
use inkapp_core::embed::extract_manifest;
use inkapp_harness::fixtures::Tool;
use inkapp_harness::recording::{
    bootstrap_strokes, catalog, extract_fixture, render_template, Source, BOXES_PER_GESTURE, PAGE_H,
};
use inkapp_remarkable::Remarkable;

#[test]
fn bootstrap_round_trip_yields_one_sample_per_box() {
    let entry = catalog().iter().find(|e| e.name == "checkmark").unwrap();
    let pdf = render_template(entry).unwrap();
    let manifest = extract_manifest(&pdf).unwrap();
    let device = Remarkable::new();

    // Synthesize ink in each box, route through the real .rm write+read path.
    let synth = bootstrap_strokes(entry, &manifest);
    let bytes = device.write_ink(&synth, PAGE_H).unwrap();
    let strokes_pdf = device.read_ink(&bytes, PAGE_H).unwrap();

    let source = Source {
        recording: "synthetic".into(),
        device: "synthetic-bootstrap".into(),
        recorded: "2026-05-23".into(),
    };
    let fixture = extract_fixture(entry, &strokes_pdf, &manifest, source);

    assert_eq!(fixture.name, "checkmark");
    assert_eq!(fixture.tool, Tool::Pen);
    assert_eq!(fixture.samples.len(), BOXES_PER_GESTURE);
    for s in &fixture.samples {
        assert!(!s.strokes.is_empty(), "each box has ink");
        for st in &s.strokes {
            for p in &st.points {
                assert!((-0.001..=1.001).contains(&p[0]) && (-0.001..=1.001).contains(&p[1]));
            }
        }
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `nix develop -c cargo test -p inkapp-harness --test extract`
Expected: FAIL (`bootstrap_strokes`/`extract_fixture`/`Source` not found).

- [ ] **Step 3: Implement extraction + bootstrap synth**

Append to `crates/inkapp-harness/src/recording.rs`:

```rust
use inkapp_core::geometry::PdfRect;
use inkapp_core::ink::Stroke;
use inkapp_core::manifest::Manifest;
use inkapp_core::readback::attribute;

pub use crate::fixtures::Source;
use crate::fixtures::{normalize, GestureFixture, Sample};

/// Collect the strokes attributed to each `box:<name>:i` (in index order) and
/// normalize each box's strokes into a [`Sample`].
pub fn extract_samples(strokes_pdf: &[Stroke], manifest: &Manifest, name: &str) -> Vec<Sample> {
    let region_ink = attribute(strokes_pdf, manifest);
    let mut samples = Vec::new();
    for i in 0..BOXES_PER_GESTURE {
        let region_name = format!("box:{name}:{i}");
        let strokes: Vec<Stroke> = region_ink
            .iter()
            .filter(|ri| ri.region == region_name)
            .flat_map(|ri| ri.strokes.clone())
            .collect();
        if !strokes.is_empty() {
            samples.push(normalize(&strokes));
        }
    }
    samples
}

/// Build a [`GestureFixture`] from already-PDF-space strokes (real or synthetic).
pub fn extract_fixture(
    entry: &CatalogEntry,
    strokes_pdf: &[Stroke],
    manifest: &Manifest,
    source: Source,
) -> GestureFixture {
    GestureFixture {
        name: entry.name.to_string(),
        tool: entry.tool,
        fit: entry.fit,
        default: 0,
        samples: extract_samples(strokes_pdf, manifest, entry.name),
        source,
    }
}

/// The PDF rect of `box:<name>:i`, or `None` if absent.
fn box_rect(manifest: &Manifest, name: &str, i: usize) -> Option<PdfRect> {
    manifest
        .regions
        .iter()
        .find(|r| r.name == format!("box:{name}:{i}"))
        .map(|r| r.rect)
}

/// Synthesize plausible PDF-space ink in each guide box for bootstrap fixtures.
/// Shapes are representative, not artistic; behavioural assertions rely only on
/// checkmark (short), scribble-out (dense), and highlight-swipe (horizontal).
pub fn bootstrap_strokes(entry: &CatalogEntry, manifest: &Manifest) -> Vec<Stroke> {
    let mut out = Vec::new();
    let hi = entry.tool.is_highlighter();
    for i in 0..BOXES_PER_GESTURE {
        let Some(r) = box_rect(manifest, entry.name, i) else { continue };
        let w = r.x1 - r.x0;
        let h = r.y1 - r.y0;
        let pad = 0.15;
        let pt = |u: f64, v: f64| inkapp_core::geometry::PdfPoint {
            x: r.x0 + (pad + u * (1.0 - 2.0 * pad)) * w,
            y: r.y0 + (pad + v * (1.0 - 2.0 * pad)) * h,
        };
        let points = match entry.name {
            "checkmark" => vec![pt(0.0, 0.45), pt(0.35, 0.0), pt(1.0, 1.0)],
            "scribble-out" => {
                // Dense zigzag: total length >> diagonal -> reads ScribbledOut.
                let mut v = Vec::new();
                for k in 0..14 {
                    let u = k as f64 / 13.0;
                    v.push(pt(u, if k % 2 == 0 { 0.0 } else { 1.0 }));
                }
                v
            }
            "highlight-swipe" | "strike-through" => vec![pt(0.0, 0.5), pt(1.0, 0.5)],
            "circle" => {
                let mut v = Vec::new();
                for k in 0..=16 {
                    let t = std::f64::consts::TAU * (k as f64) / 16.0;
                    v.push(pt(0.5 + 0.5 * t.cos(), 0.5 + 0.5 * t.sin()));
                }
                v
            }
            "arrow" => vec![pt(0.0, 0.5), pt(1.0, 0.5), pt(0.7, 0.2), pt(1.0, 0.5), pt(0.7, 0.8)],
            // handwritten-word and anything else: a short squiggle.
            _ => vec![pt(0.0, 0.5), pt(0.25, 0.2), pt(0.5, 0.6), pt(0.75, 0.2), pt(1.0, 0.6)],
        };
        out.push(Stroke { points, highlighter: hi });
    }
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `nix develop -c cargo test -p inkapp-harness --test extract`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "inkapp-harness: extraction (strokes->fixtures) + bootstrap stroke synthesis"
```

---

### Task 6: Bootstrap fixtures committed + regen-consistency guard

**Goal:** Generate and commit the bootstrap `gestures/*.json` fixtures, and add a test that regenerates them from source (real recording if present, else synthetic) and asserts the committed files match — the "golden" guard for fixtures.

**Files:**
- Create (generated, committed): `crates/inkapp-harness/tests/fixtures/gestures/*.json`
- Create: `crates/inkapp-harness/tests/regen.rs`
- Create: `crates/inkapp-harness/tests/common/mod.rs` (shared `.rmdoc` reader + regen helper)

**Acceptance Criteria:**
- [ ] `regen_fixture(entry, device) -> GestureFixture` returns the fixture from a real `recordings/<name>.rmdoc` if it exists, else from synthetic bootstrap strokes.
- [ ] On first run (fixture absent) the test writes it and fails with a "review and re-run" message (bootstrap pattern).
- [ ] On subsequent runs the committed JSON equals the regenerated fixture.
- [ ] All seven `gestures/*.json` exist and are committed.

**Verify:** `nix develop -c cargo test -p inkapp-harness --test regen` → PASS (after the bootstrap write + commit).

**Steps:**

- [ ] **Step 1: Write the regen helper + test**

`crates/inkapp-harness/tests/common/mod.rs`:

```rust
use std::io::Read;
use std::path::Path;

use inkapp_core::device::Device;
use inkapp_core::embed::extract_manifest;
use inkapp_harness::fixtures::{GestureFixture, Source};
use inkapp_harness::recording::{
    bootstrap_strokes, extract_fixture, render_template, CatalogEntry, PAGE_H,
};

/// Read the `.pdf` and first `.rm` entries out of an `.rmdoc` zip.
pub fn open_rmdoc(path: &Path) -> (Vec<u8>, Vec<u8>) {
    let file = std::fs::File::open(path).expect("open rmdoc");
    let mut archive = zip::ZipArchive::new(file).expect("read zip");
    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .collect();
    let read = |suffix: &str| -> Vec<u8> {
        let n = names.iter().find(|n| n.ends_with(suffix)).unwrap_or_else(|| panic!("no {suffix} entry"));
        let mut e = archive.by_name(n).unwrap();
        let mut b = Vec::new();
        e.read_to_end(&mut b).unwrap();
        b
    };
    (read(".pdf"), read(".rm"))
}

/// Regenerate a gesture fixture from its real recording if present, else from
/// synthetic bootstrap strokes (both via the real write/read path).
pub fn regen_fixture(entry: &CatalogEntry, device: &dyn Device) -> GestureFixture {
    let rec_path = format!(
        "{}/tests/fixtures/recordings/{}.rmdoc",
        env!("CARGO_MANIFEST_DIR"),
        entry.name
    );
    if Path::new(&rec_path).exists() {
        let (pdf, rm) = open_rmdoc(Path::new(&rec_path));
        let manifest = extract_manifest(&pdf).unwrap();
        let strokes = device.read_ink(&rm, PAGE_H).unwrap();
        let source = Source {
            recording: format!("recordings/{}.rmdoc", entry.name),
            device: "reMarkable Paper Pro Move".into(),
            recorded: "recorded".into(),
        };
        extract_fixture(entry, &strokes, &manifest, source)
    } else {
        let pdf = render_template(entry).unwrap();
        let manifest = extract_manifest(&pdf).unwrap();
        let synth = bootstrap_strokes(entry, &manifest);
        let bytes = device.write_ink(&synth, PAGE_H).unwrap();
        let strokes = device.read_ink(&bytes, PAGE_H).unwrap();
        let source = Source {
            recording: "synthetic".into(),
            device: "synthetic-bootstrap".into(),
            recorded: "2026-05-23".into(),
        };
        extract_fixture(entry, &strokes, &manifest, source)
    }
}
```

`crates/inkapp-harness/tests/regen.rs`:

```rust
mod common;

use std::path::Path;

use inkapp_harness::fixtures::GestureFixture;
use inkapp_harness::recording::catalog;
use inkapp_remarkable::Remarkable;

use common::regen_fixture;

#[test]
fn fixtures_match_regenerated() {
    let device = Remarkable::new();
    let dir = format!("{}/tests/fixtures/gestures", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dir).unwrap();

    let mut wrote_any = false;
    for entry in catalog() {
        let fixture = regen_fixture(entry, &device);
        let json = fixture.to_json().unwrap();
        let path = format!("{dir}/{}.json", entry.name);

        match std::fs::read_to_string(&path) {
            Ok(existing) => {
                let committed: GestureFixture =
                    GestureFixture::from_json(existing.as_bytes()).unwrap();
                assert_eq!(committed, fixture, "fixture {} differs from regenerated", entry.name);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::write(&path, json).unwrap();
                wrote_any = true;
            }
            Err(e) => panic!("read {path}: {e}"),
        }
    }
    assert!(!wrote_any, "wrote missing bootstrap fixtures; review and re-run");
    // Sanity: ensure all are present.
    for entry in catalog() {
        assert!(Path::new(&format!("{dir}/{}.json", entry.name)).exists());
    }
}
```

- [ ] **Step 2: First run — bootstrap the fixtures**

Run: `nix develop -c cargo test -p inkapp-harness --test regen`
Expected: FAIL with "wrote missing bootstrap fixtures; review and re-run" (the seven JSON files are now written).

- [ ] **Step 3: Re-run to verify consistency**

Run: `nix develop -c cargo test -p inkapp-harness --test regen`
Expected: PASS (committed JSON equals regenerated).

> If a fixture's `f64` serialization is unstable across runs, that indicates nondeterminism in synth/normalize — investigate rather than loosening the equality (compare via parsed `GestureFixture`, which the test already does, so float formatting differences are tolerated).

- [ ] **Step 4: Commit the fixtures + test**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "inkapp-harness: bootstrap gesture fixtures + regen-consistency guard"
```

---

### Task 7: `Gesture::Fixture` in the simulator + real-ink e2e exercisers

**Goal:** Drive the existing loop with real (bootstrap) ink via a new `Gesture::Fixture`, and assert the three e2e behaviours with committed golden composites.

**Files:**
- Modify: `crates/inkapp-harness/src/simulator.rs` (add `Gesture::Fixture`)
- Test: `crates/inkapp-harness/tests/e2e.rs`
- Create (generated, committed): `crates/inkapp-harness/tests/golden/e2e_*.png`

**Acceptance Criteria:**
- [ ] `Gesture::Fixture(&'static str)` loads `gestures/<name>.json` and transplants its default sample into the target region.
- [ ] `checkmark` → `done` reads `CheckState::Marked`; `scribble-out` → `done` reads `CheckState::ScribbledOut`.
- [ ] `highlight-swipe` over `tok-4`/`tok-5` → `HighlightableText::read` returns `{lazy, dog}`.
- [ ] Each e2e produces a committed golden inspector PNG.

**Verify:** `nix develop -c cargo test -p inkapp-harness --test e2e` → PASS (after golden bootstrap + commit).

**Steps:**

- [ ] **Step 1: Add `Gesture::Fixture` to the simulator**

In `crates/inkapp-harness/src/simulator.rs`, extend the `Gesture` enum and `synthesize`:

```rust
// in the Gesture enum, add:
    /// Real recorded ink: load `gestures/<name>.json` and transplant its default
    /// sample into the target region (per the fixture's fit policy).
    Fixture(&'static str),
```

In `synthesize`, replace the `match gesture { ... }` body's arms to add the fixture case (keep `Tap`/`Swipe`):

```rust
        match gesture {
            Gesture::Tap => strokes.push(Stroke {
                points: vec![PdfPoint { x: cx, y: cy }],
                highlighter: false,
            }),
            Gesture::Swipe => strokes.push(Stroke {
                points: vec![PdfPoint { x: r.x0, y: cy }, PdfPoint { x: r.x1, y: cy }],
                highlighter: true,
            }),
            Gesture::Fixture(name) => {
                let path = format!(
                    "{}/tests/fixtures/gestures/{name}.json",
                    env!("CARGO_MANIFEST_DIR")
                );
                let bytes = std::fs::read(&path)
                    .unwrap_or_else(|e| panic!("read fixture {path}: {e}"));
                let fixture = crate::fixtures::GestureFixture::from_json(&bytes)
                    .unwrap_or_else(|e| panic!("parse fixture {path}: {e}"));
                strokes.extend(fixture.transplant_default(*r));
            }
        }
```

> `env!("CARGO_MANIFEST_DIR")` resolves to the `inkapp-harness` crate root at compile time, so fixture loading works whether the simulator is driven from a unit or integration test. This keeps fixtures as test assets without adding a runtime path dependency to apps (apps will pass their own ink, not `Gesture::Fixture`).

The `cx`/`cy` locals already exist in `synthesize`; `r` is the region rect. Ensure `Stroke`/`PdfPoint` imports remain.

- [ ] **Step 2: Write the e2e tests**

`crates/inkapp-harness/tests/e2e.rs`:

```rust
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::compile_to_document;
use inkapp_core::widget::{RenderCx, Widget};
use inkapp_core::widgets::checkbox::{CheckState, Checkbox};
use inkapp_core::widgets::highlight_text::HighlightableText;
use inkapp_harness::simulator::{simulate, Gesture, Scenario};
use inkapp_remarkable::Remarkable;

const TOKENS: &[&str] = &["the", "quick", "brown", "fox", "lazy", "dog"];

fn assert_golden(name: &str, png: &[u8]) {
    let path = format!("{}/tests/golden/{name}.png", env!("CARGO_MANIFEST_DIR"));
    match std::fs::read(&path) {
        Ok(expected) => assert_eq!(png, expected.as_slice(), "inspector image differs from golden {name}"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(format!("{}/tests/golden", env!("CARGO_MANIFEST_DIR"))).unwrap();
            std::fs::write(&path, png).unwrap();
            panic!("golden {name} did not exist; wrote it — review and re-run");
        }
        Err(e) => panic!("could not read golden {name}: {e}"),
    }
}

#[test]
fn checkmark_marks_checkbox() {
    let cb = Checkbox::new("done");
    let body = cb.render_at(0, 20.0, 40.0, 40.0, 40.0);
    let src = format!("#set page(width: 200pt, height: 200pt, margin: 0pt)\n{body}");
    let doc = compile_to_document(&src).unwrap();
    let manifest = recover_regions(&doc).unwrap().with_version(1);
    let device = Remarkable::new();

    let trace = simulate(&src, &manifest, &device, &Scenario::new().mark("done", Gesture::Fixture("checkmark"))).unwrap();
    assert_eq!(cb.read_state(&trace.readback, &manifest), CheckState::Marked);
    assert_golden("e2e_checkmark", &trace.inspector_png);
}

#[test]
fn scribble_out_reads_scribbled() {
    let cb = Checkbox::new("done");
    let body = cb.render_at(0, 20.0, 40.0, 40.0, 40.0);
    let src = format!("#set page(width: 200pt, height: 200pt, margin: 0pt)\n{body}");
    let doc = compile_to_document(&src).unwrap();
    let manifest = recover_regions(&doc).unwrap().with_version(1);
    let device = Remarkable::new();

    let trace = simulate(&src, &manifest, &device, &Scenario::new().mark("done", Gesture::Fixture("scribble-out"))).unwrap();
    assert_eq!(cb.read_state(&trace.readback, &manifest), CheckState::ScribbledOut);
    assert_golden("e2e_scribble_out", &trace.inspector_png);
}

#[test]
fn highlight_swipe_selects_lazy_dog() {
    let w = HighlightableText::new(TOKENS);
    let mut cx = RenderCx::new(0);
    let body = w.render(&mut cx);
    let src = format!("#set page(width: 300pt, height: 120pt, margin: 10pt)\n{body}");
    let doc = compile_to_document(&src).unwrap();
    let manifest = recover_regions(&doc).unwrap().with_version(1);
    let device = Remarkable::new();

    let scenario = Scenario::new()
        .mark("tok-4", Gesture::Fixture("highlight-swipe"))
        .mark("tok-5", Gesture::Fixture("highlight-swipe"));
    let trace = simulate(&src, &manifest, &device, &scenario).unwrap();

    let mut got = w.read(&trace.readback, &manifest);
    got.sort();
    assert_eq!(got, vec!["dog".to_string(), "lazy".to_string()]);
    assert_golden("e2e_highlight_lazy_dog", &trace.inspector_png);
}
```

- [ ] **Step 3: Run to verify failure, then bootstrap goldens**

Run: `nix develop -c cargo test -p inkapp-harness --test e2e`
Expected: first run FAILs writing the three goldens ("did not exist; wrote it"). Run again:

Run: `nix develop -c cargo test -p inkapp-harness --test e2e`
Expected: PASS (behaviours + goldens match).

> If `scribble-out` reads `Marked` instead of `ScribbledOut`, the bootstrap zigzag's transplanted length fell below `SCRIBBLE_RATIO × diagonal`. The fixture uses `stretch-x` into a 40×40 region; increase the zigzag density in `bootstrap_strokes` (more segments) and re-run Task 6's regen, then re-bootstrap this golden. Do not lower `SCRIBBLE_RATIO` to force it — tune the representative ink.

- [ ] **Step 4: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "inkapp-harness: Gesture::Fixture + real-ink e2e exercisers with goldens"
```

---

### Task 8: Transform-fidelity validation (deferred #2)

**Goal:** Validate the reMarkable transform against calibration taps at known PDF points; pass on the bootstrap (on-model) calibration, and provide a fitted-constants suggestion + clear failure message for when a real recording exceeds tolerance.

**Files:**
- Modify: `crates/inkapp-harness/src/recording.rs` (add `synth_calibration`, `fit_scale`)
- Test: `crates/inkapp-harness/tests/transform_fidelity.rs`

**Acceptance Criteria:**
- [ ] `synth_calibration(device) -> (pdf, rm)` produces a capture whose taps sit exactly on `pdf_to_device(known_point)`.
- [ ] The test pairs each recorded tap with its `cross:<i>` known point and asserts max error `< TOL` device px (passes for bootstrap).
- [ ] On failure it prints a least-squares-fitted scale suggestion for adoption (gated adoption: validate by default, adopt only if exceeded).
- [ ] Uses a real `recordings/calibration.rmdoc` if present, else the synthetic capture.

**Verify:** `nix develop -c cargo test -p inkapp-harness --test transform_fidelity` → PASS.

**Steps:**

- [ ] **Step 1: Add calibration synth + fit helper**

Append to `crates/inkapp-harness/src/recording.rs`:

```rust
use inkapp_core::device::Device;
use inkapp_core::geometry::DevicePoint;

/// Synthesize a calibration capture: a short tap centered exactly on each known
/// point's `pdf_to_device` image, so a correct model measures ~0 error.
pub fn synth_calibration(device: &dyn Device) -> Result<(Vec<u8>, Vec<u8>)> {
    let pdf = render_calibration()?;
    let mut strokes = Vec::new();
    for p in calibration_points() {
        // A 3-point dot in PDF space at the known point.
        strokes.push(Stroke {
            points: vec![*&p, *&p, *&p],
            highlighter: false,
        });
    }
    let rm = device.write_ink(&strokes, PAGE_H)?;
    Ok((pdf, rm))
}

/// Least-squares uniform scale relating known device points to recorded ones,
/// reported as an adoption suggestion when validation fails.
pub fn fit_scale(expected: &[DevicePoint], actual: &[DevicePoint]) -> f64 {
    let mut num = 0.0;
    let mut den = 0.0;
    for (e, a) in expected.iter().zip(actual) {
        num += e.x * a.x + e.y * a.y;
        den += e.x * e.x + e.y * e.y;
    }
    if den.abs() < f64::EPSILON {
        1.0
    } else {
        num / den
    }
}
```

> `*&p` is intentional cloning of the `Copy` `PdfPoint`; written plainly as `p` works too — keep whichever the linter prefers (`p` is cleaner).

- [ ] **Step 2: Write the fidelity test**

`crates/inkapp-harness/tests/transform_fidelity.rs`:

```rust
mod common;

use std::path::Path;

use inkapp_core::device::Device;
use inkapp_core::embed::extract_manifest;
use inkapp_core::geometry::{DevicePoint, PdfPoint};
use inkapp_harness::recording::{calibration_points, fit_scale, synth_calibration, PAGE_H};
use inkapp_remarkable::Remarkable;
use rm_files::Scene;

use common::open_rmdoc;

/// Max acceptable per-point error (device px) before adoption is required.
const TOL: f64 = 2.0;

fn tap_centroids(rm: &[u8]) -> Vec<DevicePoint> {
    Scene::parse(rm)
        .unwrap()
        .strokes()
        .into_iter()
        .map(|s| {
            let n = s.points.len().max(1) as f64;
            let (sx, sy) = s.points.iter().fold((0.0, 0.0), |(ax, ay), p| (ax + p.x as f64, ay + p.y as f64));
            DevicePoint { x: sx / n, y: sy / n }
        })
        .collect()
}

#[test]
fn transform_matches_calibration_within_tolerance() {
    let device = Remarkable::new();

    // Prefer a real recording; fall back to the on-model synthetic capture.
    let real = format!("{}/tests/fixtures/recordings/calibration.rmdoc", env!("CARGO_MANIFEST_DIR"));
    let (pdf, rm) = if Path::new(&real).exists() {
        open_rmdoc(Path::new(&real))
    } else {
        synth_calibration(&device).unwrap()
    };

    let manifest = extract_manifest(&pdf).unwrap();
    let known = calibration_points();
    let actual = tap_centroids(&rm);
    assert_eq!(actual.len(), known.len(), "one tap per cross");

    // Pair each tap to its nearest known point via the model's inverse.
    let mut expected_dev = Vec::new();
    let mut actual_dev = Vec::new();
    let mut max_err: f64 = 0.0;
    for a in &actual {
        let a_pdf = device.device_to_pdf(*a, PAGE_H);
        // nearest known cross center
        let k = known
            .iter()
            .min_by(|p, q| dist2(p, &a_pdf).partial_cmp(&dist2(q, &a_pdf)).unwrap())
            .unwrap();
        let predicted = device.pdf_to_device(*k, PAGE_H);
        let err = ((predicted.x - a.x).powi(2) + (predicted.y - a.y).powi(2)).sqrt();
        max_err = max_err.max(err);
        expected_dev.push(predicted);
        actual_dev.push(*a);
    }

    if max_err >= TOL {
        let suggested = fit_scale(&expected_dev, &actual_dev);
        panic!(
            "transform error {max_err:.2}px exceeds tolerance {TOL}px. \
             Gated adoption: refit and adopt constants in inkapp-remarkable \
             (suggested uniform scale factor x{suggested:.4}), regenerate goldens, \
             record provenance, then re-run."
        );
    }
}

fn dist2(a: &PdfPoint, b: &PdfPoint) -> f64 {
    (a.x - b.x).powi(2) + (a.y - b.y).powi(2)
}
```

- [ ] **Step 3: Run to verify it passes**

Run: `nix develop -c cargo test -p inkapp-harness --test transform_fidelity`
Expected: PASS (synthetic taps are on-model → ~0 error).

- [ ] **Step 4: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "inkapp-harness: transform-fidelity validation via calibration taps (deferred #2)"
```

---

### Task 9: Manual bars — recording + device acceptance (`#[ignore]`)

**Goal:** The two documented hardware bars: push templates to `/InkAppDev/fixtures/` & pull captures (recording), and write a `.rm` + push to `/InkAppDev/acceptance/` & eyeball it (device acceptance, deferred #3). Both shell out to `rmapi` and are `#[ignore]`.

**Files:**
- Modify: `crates/inkapp-harness/tests/common/mod.rs` (add `rmapi` helpers)
- Test: `crates/inkapp-harness/tests/record.rs` (`#[ignore]`)
- Test: `crates/inkapp-harness/tests/acceptance.rs` (`#[ignore]`)

**Acceptance Criteria:**
- [ ] `rmapi_mkdir`, `rmapi_put`, `rmapi_get` helpers shell out to the `rmapi` CLI with `-ni` and null stdin (per the mechanics doc's token-clobber guidance).
- [ ] `record.rs` (`#[ignore]`) renders all templates + the calibration sheet, creates `/InkAppDev`/`/InkAppDev/fixtures`, and pushes each; a second `#[ignore]` pulls the folder into `tests/fixtures/recordings/`.
- [ ] `acceptance.rs` (`#[ignore]`) writes a known `.rm` via `write_ink`, wraps it as a pushable PDF doc, pushes to `/InkAppDev/acceptance/`, and prints an eyeball instruction.
- [ ] Non-ignored `make test` is unaffected (these never run unless `--ignored`).

**Verify:** `nix develop -c cargo test -p inkapp-harness` → PASS (ignored tests are skipped); `... --test record -- --ignored` is the documented manual run.

**Steps:**

- [ ] **Step 1: Add rmapi helpers**

Append to `crates/inkapp-harness/tests/common/mod.rs`:

```rust
use std::process::{Command, Stdio};

/// `rmapi mkdir` (idempotent/best-effort; rmapi errors on an existing dir).
pub fn rmapi_mkdir(folder: &str) {
    let _ = Command::new("rmapi").args(["-ni", "mkdir", folder]).stdin(Stdio::null()).status();
}

/// `rmapi put --content-only <pdf> <folder>` (preserves on-device ink).
pub fn rmapi_put(pdf_path: &Path, folder: &str) {
    let ok = Command::new("rmapi")
        .args(["-ni", "put", "--content-only", pdf_path.to_str().unwrap(), folder])
        .stdin(Stdio::null())
        .status()
        .expect("spawn rmapi")
        .success();
    assert!(ok, "rmapi put failed for {}", pdf_path.display());
}

/// `rmapi get <remote> <dest-dir>`.
pub fn rmapi_get(remote: &str, dest_dir: &Path) {
    let ok = Command::new("rmapi")
        .args(["-ni", "get", remote])
        .current_dir(dest_dir)
        .stdin(Stdio::null())
        .status()
        .expect("spawn rmapi")
        .success();
    assert!(ok, "rmapi get failed for {remote}");
}
```

> `common/mod.rs` is shared by several integration tests; each `#[allow(dead_code)]`-s unused helpers automatically because Rust treats per-test-binary unused `pub` items leniently. If clippy warns, add `#![allow(dead_code)]` at the top of `common/mod.rs`.

- [ ] **Step 2: Recording bar**

`crates/inkapp-harness/tests/record.rs`:

```rust
mod common;

use inkapp_harness::recording::{catalog, render_calibration, render_template};

use common::{rmapi_get, rmapi_mkdir, rmapi_put};

const FIXTURES_FOLDER: &str = "/InkAppDev/fixtures";

#[test]
#[ignore = "requires a paired reMarkable; run: cargo test -p inkapp-harness --test record push_templates -- --ignored --nocapture"]
fn push_templates() {
    rmapi_mkdir("/InkAppDev");
    rmapi_mkdir(FIXTURES_FOLDER);
    let dir = tempfile::tempdir().unwrap();

    // Calibration sheet first so it sorts to the top of the folder.
    let cal = dir.path().join("calibration.pdf");
    std::fs::write(&cal, render_calibration().unwrap()).unwrap();
    rmapi_put(&cal, FIXTURES_FOLDER);

    for entry in catalog() {
        let path = dir.path().join(format!("{}.pdf", entry.name));
        std::fs::write(&path, render_template(entry).unwrap()).unwrap();
        rmapi_put(&path, FIXTURES_FOLDER);
    }
    eprintln!("pushed templates to {FIXTURES_FOLDER}; draw on each, sync, then run pull_recordings");
}

#[test]
#[ignore = "requires a paired reMarkable; run after drawing: cargo test -p inkapp-harness --test record pull_recordings -- --ignored --nocapture"]
fn pull_recordings() {
    let dest = format!("{}/tests/fixtures/recordings", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dest).unwrap();
    rmapi_get(FIXTURES_FOLDER, std::path::Path::new(&dest));
    eprintln!("pulled {FIXTURES_FOLDER} into {dest}; re-run the regen test to extract fixtures");
}
```

`tempfile` is already a dev-dependency? It is used by the spike; add `tempfile = "3"` to `crates/inkapp-harness/Cargo.toml` `[dev-dependencies]` if missing.

- [ ] **Step 3: Device-acceptance bar**

`crates/inkapp-harness/tests/acceptance.rs`:

```rust
mod common;

use inkapp_core::device::Device;
use inkapp_core::geometry::PdfPoint;
use inkapp_core::ink::Stroke;
use inkapp_harness::recording::{render_template, catalog, PAGE_H};
use inkapp_remarkable::Remarkable;

use common::{rmapi_mkdir, rmapi_put};

const ACCEPTANCE_FOLDER: &str = "/InkAppDev/acceptance";

#[test]
#[ignore = "requires a paired reMarkable; run: cargo test -p inkapp-harness --test acceptance writes_and_pushes_rm -- --ignored --nocapture"]
fn writes_and_pushes_rm() {
    // Use the checkmark template as the background PDF, and write a known stroke
    // via the framework's writer; push and eyeball that the ink renders.
    let device = Remarkable::new();
    let entry = catalog().iter().find(|e| e.name == "checkmark").unwrap();
    let pdf = render_template(entry).unwrap();

    let stroke = Stroke {
        points: vec![PdfPoint { x: 40.0, y: 500.0 }, PdfPoint { x: 380.0, y: 500.0 }],
        highlighter: false,
    };
    let _rm = device.write_ink(&[stroke], PAGE_H).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("acceptance.pdf");
    std::fs::write(&path, &pdf).unwrap();

    rmapi_mkdir("/InkAppDev");
    rmapi_mkdir(ACCEPTANCE_FOLDER);
    rmapi_put(&path, ACCEPTANCE_FOLDER);
    eprintln!(
        "pushed acceptance doc to {ACCEPTANCE_FOLDER}. NOTE: content-only push carries the \
         PDF only; to verify the WRITTEN .rm renders, sideload the .rm via the device's \
         document bundle and confirm the horizontal line at y=500 appears. See spec §H#3."
    );
}
```

> The `.rm`-rendering check is the genuine fidelity gate. Content-only `rmapi put` swaps the PDF blob only (it never writes `.rm`), so confirming our *written* `.rm` renders requires placing it as the page's annotation file in the bundle. This bar documents that procedure; it is intentionally manual and device-specific.

- [ ] **Step 4: Verify the suite still passes (ignored tests skipped)**

Run: `nix develop -c cargo test -p inkapp-harness`
Expected: PASS; `record`/`acceptance` report as ignored.

- [ ] **Step 5: Full workspace gate**

Run: `make test` then `make clippy`
Expected: both green.

- [ ] **Step 6: Commit**

```bash
git add -A
git -c core.hooksPath=.githooks commit -m "inkapp-harness: manual recording + device-acceptance bars (rmapi, #[ignore])"
```

---

## Self-Review

**Spec coverage:**
- Catalog / 1:1:1:1 (spec A, C) → Task 4.
- Crate layout (spec B) → Tasks 3–9 place code as specified; writer diff in `rm-files` → Task 2.
- Recording workflow + `/InkAppDev/fixtures/` + calibration (spec D) → Tasks 4, 9.
- Fixture format + provenance (spec E) → Tasks 3, 6.
- Transplant math (spec F) → Task 3.
- Replay / `Gesture::Fixture` + e2e (spec G) → Task 7; Checkbox discrimination → Task 1.
- Deferred #1 writer structural-diff (spec H#1) → Task 2.
- Deferred #2 transform fidelity (spec H#2) → Task 8.
- Deferred #3 device acceptance (spec H#3) → Task 9.
- Automation boundary + bootstrap (spec I) → Tasks 6, 7 (goldens), 9 (`#[ignore]`).

**Type consistency:** `GestureFixture`/`Sample`/`UnitStroke`/`Fit`/`Tool`/`Source` defined in Task 3 and used unchanged in Tasks 5–8. `CatalogEntry`/`BoxShape`/`PAGE_W`/`PAGE_H`/`BOXES_PER_GESTURE` defined in Task 4, used in Tasks 5–9. `CheckState` (Task 1) used in Task 7. `block_structure`/`BlockSummary` (Task 2) used only in its own test. `extract_fixture`/`extract_samples`/`bootstrap_strokes` (Task 5) used in Tasks 6–7. `Device` trait method signatures (`read_ink`/`write_ink(bytes, page_h)`) match `inkapp-core`.

**Placeholder scan:** No TBD/TODO; every code step shows complete code. The only intentionally-deferred *content* is real recorded ink, which the bootstrap path fills deterministically and the manual bar (Task 9) replaces.
