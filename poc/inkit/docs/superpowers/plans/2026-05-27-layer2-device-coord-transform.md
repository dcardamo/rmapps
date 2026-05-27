# Layer 2 — Device coord transform: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove `rm-device`'s PDF↔device coordinate transform and `.rm` ink read/write — the bottom layer the reader app sits on — with committed Rust tests, and verify `inkctl`'s lens at this layer matches what the library sees. Fix any `inkctl` / `inkapp-harness` bugs encountered.

**Architecture:** Tests live in `crates/rm-device/tests/` (transform + read/write contracts) and `crates/inkctl/tests/` (CLI-vs-library parity). One real-recording fixture (`crates/inkapp-harness/tests/fixtures/recordings/calibration.rmdoc`) drives the fixture-decode test. Out-of-page behavior is *pinned* by the test (we document what the impl currently does, then assert it stays that way) rather than redesigned.

**Tech Stack:** Rust, `cargo test`, `inkapp-core` `Device` trait, `rm-device::Remarkable`, `rm-files` Scene parser, `inkctl` CLI, real `.rmdoc` fixtures already in the tree.

**Spec:** [docs/superpowers/specs/2026-05-27-reader-thorough-test-design.md](../specs/2026-05-27-reader-thorough-test-design.md). This plan covers Layer 2 only; subsequent layer plans get written after this one is green.

---

## Pre-flight: inventory of what already exists

**Existing Layer-2 tests** in `crates/rm-device/tests/device.rs`:

- `transform_is_invertible` — one point, A4 height (841.89pt), 1e-6 tolerance.
- `ink_round_trips_through_rm` — one highlighter stroke, A4, 0.5pt tolerance.
- `non_highlighter_ink_round_trips` — one pen stroke, A4, 0.5pt tolerance.

**Gaps vs. the spec inventory for Layer 2:**

1. Only one geometry (A4); the inkapp default is 420×560pt and never tested.
2. No fixture-based decode — all round-trips are synthetic in-memory data.
3. Out-of-page (off-edge) strokes: no test, no documented contract.
4. Text-highlight (`GlyphRange`) synthesis path in `Remarkable::read_ink` (lib.rs:113–134) is unverified at the `rm-device` boundary.
5. `inkctl` lens parity: nothing asserts `inkctl ink list` agrees with `Remarkable::read_ink` on the same bytes; in fact, no `inkctl` verb today accepts a raw `.rm` byte stream — that gap is the expected first inkctl bug fix.
6. Stale doc-comment in `rm-device/src/lib.rs:28` references a `transform_fidelity` test that does not exist.

The seven tasks below close these gaps in order.

---

## Task 1: Multi-geometry round-trip tests

**Goal:** Prove the PDF↔device transform inverts at the inkapp-default page (420×560pt) and at a tall page (A4), at multiple points across the page rather than just one.

**Files:**
- Modify: `crates/rm-device/tests/device.rs`

**Acceptance Criteria:**
- [ ] A parameterized test asserts `device_to_pdf(pdf_to_device(p, h), h) ≈ p` (1e-6 absolute) for each (geometry, point) combination.
- [ ] Geometries tested: `(page_h_pt = 560.0)` and `(page_h_pt = 841.89)`.
- [ ] Points tested per geometry: page center, all four corners (0,0)/(page_w,0)/(0,page_h)/(page_w,page_h), and one mid-edge.
- [ ] Test name is `transform_is_invertible_across_geometries`; the existing single-point `transform_is_invertible` is removed (subsumed).
- [ ] `cargo test -p rm-device transform_is_invertible_across_geometries` passes.

**Verify:** `nix develop -c cargo test -p rm-device transform_is_invertible_across_geometries -- --nocapture` → PASS.

**Steps:**

- [ ] **Step 1: Write the new test, delete the old one**

Replace the body of `crates/rm-device/tests/device.rs` lines 11–19 (the `transform_is_invertible` test) with:

```rust
// reMarkable's canvas aspect is fixed; given page_h_pt the page_w_pt is
// derived inside the impl. The samples below use the impl's own derivation
// implicitly — by going pdf -> device -> pdf — so they don't need to repeat
// the math.
fn page_w_for(rm: &Remarkable, page_h: f64) -> f64 {
    // Round-trip the point (page_h, 0) where x=page_h would land off-page;
    // instead derive width by inverting the centred-x model: pdf x=0 maps
    // to device x = -page_w/2 * scale. We don't need the value here — every
    // sample we pick is expressed as a fraction of page_w determined by an
    // out-and-back trip, so just pick PDF points inside a known box.
    let _ = rm;
    let _ = page_h;
    // Conservative width: 0.5 * page_h gives ample interior room at any aspect.
    page_h * 0.5
}

#[test]
fn transform_is_invertible_across_geometries() {
    let rm = Remarkable::new();
    // (page_h_pt, label) — inkapp default (560) and A4 (841.89).
    for (page_h, label) in &[(560.0_f64, "inkapp-default"), (841.89_f64, "a4")] {
        let page_w = page_w_for(&rm, *page_h);
        let samples: &[(f64, f64, &str)] = &[
            (page_w / 2.0, *page_h / 2.0, "center"),
            (0.0, 0.0, "bottom-left"),
            (page_w, 0.0, "bottom-right"),
            (0.0, *page_h, "top-left"),
            (page_w, *page_h, "top-right"),
            (page_w / 2.0, 0.0, "mid-bottom"),
        ];
        for (x, y, name) in samples {
            let p = PdfPoint { x: *x, y: *y };
            let d = rm.pdf_to_device(p, *page_h);
            let back = rm.device_to_pdf(d, *page_h);
            assert!(
                (back.x - p.x).abs() < 1e-6,
                "[{label}/{name}] x inverts: {} vs {}",
                p.x,
                back.x
            );
            assert!(
                (back.y - p.y).abs() < 1e-6,
                "[{label}/{name}] y inverts: {} vs {}",
                p.y,
                back.y
            );
        }
    }
}
```

- [ ] **Step 2: Run and confirm**

Run: `nix develop -c cargo test -p rm-device transform_is_invertible_across_geometries -- --nocapture`
Expected: PASS, prints nothing on success.

- [ ] **Step 3: Run the rest of the rm-device suite to confirm no regression**

Run: `nix develop -c cargo test -p rm-device`
Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/rm-device/tests/device.rs
git commit -m "tests(layer-2): transform invertibility across geometries + multiple points"
```

---

## Task 2: Fix stale `transform_fidelity` doc reference

**Goal:** Remove an outdated comment in `rm-device/src/lib.rs` that references a `transform_fidelity` test that does not exist in the tree. Tiny but tracked because the rule says inkctl/harness/core inconsistencies get fixed in flight.

**Files:**
- Modify: `crates/rm-device/src/lib.rs:28`

**Acceptance Criteria:**
- [ ] No source file references a test named `transform_fidelity`.
- [ ] Replacement comment points the reader at the existing `transform_is_invertible_across_geometries` test instead.
- [ ] `nix develop -c cargo build -p rm-device` succeeds.

**Verify:** `grep -r transform_fidelity crates/` → no matches.

**Steps:**

- [ ] **Step 1: Locate the comment**

Read `crates/rm-device/src/lib.rs` lines 22–30. The relevant sentence ends with "Re-derive any time via the `transform_fidelity` test."

- [ ] **Step 2: Replace**

Change line 28 from:

```rust
/// residuals ≤4px. Re-derive any time via the `transform_fidelity` test.
```

to:

```rust
/// residuals ≤4px. Re-derive any time by re-running the calibration capture
/// against `crates/rm-device/tests/device.rs::transform_is_invertible_across_geometries`.
```

- [ ] **Step 3: Verify**

Run: `grep -r transform_fidelity crates/`
Expected: no output.

Run: `nix develop -c cargo build -p rm-device`
Expected: clean build.

- [ ] **Step 4: Commit**

```bash
git add crates/rm-device/src/lib.rs
git commit -m "fix(rm-device): drop stale transform_fidelity comment, point at real test"
```

---

## Task 3: Pin the out-of-page-stroke contract

**Goal:** Document what `Remarkable::pdf_to_device` / `device_to_pdf` / `read_ink` do when given points outside the page rectangle (currently: pure math, no clamping, no dropping — coordinates simply go negative or exceed the canvas), and write a test that locks the contract in.

**Files:**
- Modify: `crates/inkapp-core/src/device.rs` (trait doc-comment)
- Modify: `crates/rm-device/src/lib.rs` (impl doc-comment)
- Modify: `crates/rm-device/tests/device.rs`

**Acceptance Criteria:**
- [ ] `Device::pdf_to_device` doc-comment names the off-page behavior policy ("pure linear map; off-page inputs produce off-canvas outputs, never an error").
- [ ] `Remarkable::pdf_to_device` impl doc-comment confirms it honors the policy.
- [ ] A test `off_page_strokes_round_trip_without_clamping` asserts that a point with x = -100 and a point with y = page_h + 100 both transform out-and-back to themselves within 1e-6.
- [ ] A test `read_ink_does_not_drop_off_canvas_points` constructs a stroke whose first point is well off-page (PDF x = -50, y = -50), writes it via `write_ink`, reads it back, and asserts both points are present (length unchanged).
- [ ] `nix develop -c cargo test -p rm-device` passes.

**Verify:** `nix develop -c cargo test -p rm-device off_page` → 2 passed.

**Steps:**

- [ ] **Step 1: Add the trait doc-comment**

In `crates/inkapp-core/src/device.rs`, replace the `pdf_to_device` doc-comment (line 8) with:

```rust
    /// Map a PDF-space point into this device's ink space.
    ///
    /// **Off-page contract.** Implementations MUST treat off-page inputs as a
    /// pure linear extrapolation of the in-page transform. They MUST NOT clamp,
    /// drop, or error on off-page points. Off-page points produce off-canvas
    /// outputs and round-trip back to themselves (within numerical tolerance).
    /// Rationale: the harness substitutes synthetic ink for components that
    /// extend off the laid-out page rect (e.g. action bands flush to an edge),
    /// and clamping would silently lose tap locations.
    fn pdf_to_device(&self, p: PdfPoint, page_h_pt: f64) -> DevicePoint;
```

- [ ] **Step 2: Add the impl note**

In `crates/rm-device/src/lib.rs`, just above `impl Device for Remarkable` (around line 66), add:

```rust
// Off-page contract: the four `Device` methods on `Remarkable` are pure linear
// maps. No clamping, no dropping, no error on off-page inputs. See the trait
// doc-comment in `inkapp_core::device::Device::pdf_to_device` and the tests
// `off_page_strokes_round_trip_without_clamping` /
// `read_ink_does_not_drop_off_canvas_points` in `tests/device.rs`.
```

- [ ] **Step 3: Add the tests**

Append to `crates/rm-device/tests/device.rs`:

```rust
#[test]
fn off_page_strokes_round_trip_without_clamping() {
    let rm = Remarkable::new();
    let page_h = 560.0_f64;
    let cases = [
        ("left-of-page", PdfPoint { x: -100.0, y: 200.0 }),
        ("above-page", PdfPoint { x: 100.0, y: page_h + 100.0 }),
        ("below-page", PdfPoint { x: 100.0, y: -50.0 }),
    ];
    for (label, p) in cases {
        let d = rm.pdf_to_device(p, page_h);
        let back = rm.device_to_pdf(d, page_h);
        assert!(
            (back.x - p.x).abs() < 1e-6 && (back.y - p.y).abs() < 1e-6,
            "[{label}] off-page point did not round-trip: {p:?} -> {d:?} -> {back:?}"
        );
    }
}

#[test]
fn read_ink_does_not_drop_off_canvas_points() {
    let rm = Remarkable::new();
    let page_h = 560.0_f64;
    let original = vec![Stroke {
        points: vec![
            PdfPoint { x: -50.0, y: -50.0 },   // off-page
            PdfPoint { x: 100.0, y: 100.0 },   // on-page
            PdfPoint { x: 9999.0, y: 9999.0 }, // far off-page
        ],
        highlighter: false,
    }];
    let bytes = rm.write_ink(&original, page_h).unwrap();
    let got = rm.read_ink(&bytes, page_h).unwrap();
    assert_eq!(got.len(), 1, "stroke count preserved");
    assert_eq!(
        got[0].points.len(),
        original[0].points.len(),
        "point count preserved — off-page points not dropped"
    );
}
```

- [ ] **Step 4: Verify**

Run: `nix develop -c cargo test -p rm-device off_page`
Expected: `off_page_strokes_round_trip_without_clamping` and `read_ink_does_not_drop_off_canvas_points` both PASS.

Run: `nix develop -c cargo build -p inkapp-core -p rm-device` (the trait doc-comment is in core).
Expected: clean build.

- [ ] **Step 5: Commit**

```bash
git add crates/inkapp-core/src/device.rs crates/rm-device/src/lib.rs crates/rm-device/tests/device.rs
git commit -m "tests(layer-2): pin off-page-stroke contract; document on trait + impl"
```

---

## Task 4: Text-highlight (`GlyphRange`) synthesis test

**Goal:** Verify that `Remarkable::read_ink` correctly synthesizes 17-point horizontal swipes through the vertical midpoint of each text-highlight rectangle (the code path at `rm-device/src/lib.rs:113–134`). Uses the existing real fixture in `rm-files/tests/fixtures/rmtest-glyph.rmdoc` to avoid needing a writer for `GlyphRange` items.

**Files:**
- Modify: `crates/rm-device/Cargo.toml` (add `rm-files` to `[dev-dependencies]` if not already; it is via `pub use` so check).
- Create: `crates/rm-device/tests/fixtures/` (symlink or copy of one `.rm` extracted from the rm-files fixture — or load via zip at test time; see Step 1 for the decision).
- Modify: `crates/rm-device/tests/device.rs`

**Acceptance Criteria:**
- [ ] A test `text_highlight_rect_synthesizes_swipe` loads `.rm` bytes containing at least one `GlyphRange` item, calls `Remarkable::read_ink`, and asserts:
  - Resulting stroke vector has at least one stroke with `highlighter == true`.
  - At least one synthesized swipe has exactly 17 points (matches `SAMPLES = 16` → `0..=SAMPLES` = 17 inclusive).
  - All points in one synthesized swipe share the same `y` (within 1e-6 — same midpoint), and `x` is monotonically non-decreasing.
- [ ] The fixture used is identified by path in a comment in the test (so future readers can find the source).
- [ ] `nix develop -c cargo test -p rm-device text_highlight_rect_synthesizes_swipe` passes.

**Verify:** `nix develop -c cargo test -p rm-device text_highlight_rect_synthesizes_swipe -- --nocapture` → PASS.

**Steps:**

- [ ] **Step 1: Decide how to access the `.rm` bytes**

The fixture lives in `crates/rm-files/tests/fixtures/rmtest-glyph.rmdoc` (a zip archive). The `.rm` files inside it are at a path determined by the bundle layout. Pick the approach:

- **(a)** Use `rm_files::Bundle` to open the `.rmdoc` and pull out the first page's scene bytes in code. This is the right answer because it survives bundle-layout changes.
- **(b)** Pre-extract one `.rm` and check it into `crates/rm-device/tests/fixtures/`. Simpler but duplicates a fixture.

Use **(a)**. The bundle API is:

```rust
let bundle = rm_files::Bundle::open(std::path::Path::new(path))?;
let pages = bundle.pages();          // Vec<Page<'_>>
let bytes = pages[i].scene_bytes();  // Option<&[u8]>
```

- [ ] **Step 2: Add `rm-files` to dev-dependencies if needed**

Check:

```bash
grep -A5 "\[dev-dependencies\]" crates/rm-device/Cargo.toml
```

If `rm-files` is not present under `[dev-dependencies]`, add:

```toml
[dev-dependencies]
rm-files = { path = "../rm-files" }
```

(If `rm-files` already appears under `[dependencies]` it is also accessible from tests; no change needed.)

- [ ] **Step 3: Add the test**

Append to `crates/rm-device/tests/device.rs`:

```rust
// Fixture: crates/rm-files/tests/fixtures/rmtest-glyph.rmdoc
// Page index 1 carries a `GlyphRange text="ARCHIVE"` per
// crates/rm-files/tests/highlights.rs.
const GLYPH_FIXTURE_BUNDLE: &str = "../rm-files/tests/fixtures/rmtest-glyph.rmdoc";

#[test]
fn text_highlight_rect_synthesizes_swipe() {
    // Replace `read_first_rm_from_bundle` with the actual rm-files bundle API
    // discovered in Step 1. The call must return the bytes of ONE .rm file
    // from the bundle that contains a GlyphRange item.
    let bytes = read_first_rm_with_glyph(GLYPH_FIXTURE_BUNDLE);

    let rm = Remarkable::new();
    let page_h = 841.89_f64; // rmdoc bundles declare A4-ish; this test only
                             // needs the height to be the one the fixture
                             // was captured against — see rm-files highlights.rs
                             // which uses the bundle's own declared height.
    let strokes = rm.read_ink(&bytes, page_h).expect("read_ink");

    let synthesized: Vec<&Stroke> = strokes
        .iter()
        .filter(|s| s.highlighter && s.points.len() == 17)
        .collect();
    assert!(
        !synthesized.is_empty(),
        "expected at least one 17-point highlighter swipe synthesized from a GlyphRange"
    );

    let swipe = synthesized[0];
    let y0 = swipe.points[0].y;
    for (i, pt) in swipe.points.iter().enumerate() {
        assert!(
            (pt.y - y0).abs() < 1e-6,
            "swipe point {i} y drifted: {} vs {}",
            pt.y,
            y0
        );
    }
    let xs: Vec<f64> = swipe.points.iter().map(|p| p.x).collect();
    for w in xs.windows(2) {
        assert!(w[1] >= w[0] - 1e-9, "swipe x not monotonic: {:?}", xs);
    }
}

// Helper. Implementation depends on rm-files bundle API discovered in Step 1.
// Read the first .rm entry in the bundle that yields a non-empty
// `Scene::text_highlights()` list, and return its raw bytes.
fn read_first_rm_with_glyph(bundle_path: &str) -> Vec<u8> {
    use rm_files::{Bundle, Scene};
    let bundle = Bundle::open(std::path::Path::new(bundle_path))
        .expect("open bundle");
    for page in bundle.pages() {
        let Some(bytes) = page.scene_bytes() else { continue };
        let scene = Scene::parse(bytes).expect("parse scene");
        if !scene.text_highlights().is_empty() {
            return bytes.to_vec();
        }
    }
    panic!("no page in bundle had a GlyphRange item");
}
```

**Note.** The exact `rm_files::bundle::*` API may differ from what's sketched above. Step 1 told you the real names. Substitute them in `read_first_rm_with_glyph` before running.

- [ ] **Step 4: Iterate until it compiles, then run**

Run: `nix develop -c cargo test -p rm-device text_highlight_rect_synthesizes_swipe -- --nocapture`
Expected: PASS. If the rm-files bundle API names don't match, adjust the helper.

- [ ] **Step 5: Commit**

```bash
git add crates/rm-device/Cargo.toml crates/rm-device/tests/device.rs
git commit -m "tests(layer-2): text-highlight rects synthesize 17-point highlighter swipes"
```

---

## Task 5: Fixture-decode test against the calibration recording

**Goal:** Load a real on-device-captured recording (`calibration.rmdoc`) and assert that strokes come out of `Remarkable::read_ink` at the expected PDF-space locations. This is the spec inventory line item "Stroke decoded from a fixture `.rm` lands in the expected PDF-point region."

**Files:**
- Modify: `crates/rm-device/tests/device.rs`

**Acceptance Criteria:**
- [ ] Test `calibration_fixture_decodes_to_expected_pdf_region` loads the calibration bundle, picks one stroke from one known page, asserts the stroke's bounding box overlaps a documented PDF-space region (the region is recorded in the test as a comment with provenance: which page, which cross of the 5-cross sheet).
- [ ] The assertion uses a 4pt tolerance (matches the residuals quoted in `rm-device/src/lib.rs:27`).
- [ ] If the fixture is moved or replaced, the failure message names the fixture path and what to re-derive.

**Verify:** `nix develop -c cargo test -p rm-device calibration_fixture_decodes_to_expected_pdf_region -- --nocapture` → PASS.

**Steps:**

- [ ] **Step 1: Discover the fixture's expected content**

The bundle was captured against a 5-cross calibration sheet (rm-device/src/lib.rs:25–30). Before writing the assertions, run an exploratory script to print stroke bounds per page:

```bash
cat > /tmp/dump_calibration.rs <<'EOF'
// Run with: cd crates/rm-device && cargo run --example dump_calibration --quiet
// (or paste into a temporary test that prints and panics)
EOF
```

The cheapest path: write a temporary `#[ignore]` test in the same file that prints bounds, run it with `--ignored --nocapture`, copy the output into the real test as the expected box, then delete the ignored test. Example:

```rust
#[test]
#[ignore]
fn _dump_calibration_strokes() {
    let bytes = read_first_rm_from(
        "../inkapp-harness/tests/fixtures/recordings/calibration.rmdoc",
        0, // page index 0
    );
    let rm = Remarkable::new();
    let strokes = rm.read_ink(&bytes, 841.89).unwrap();
    for (i, s) in strokes.iter().enumerate() {
        let (mut x0, mut y0, mut x1, mut y1) =
            (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in &s.points {
            x0 = x0.min(p.x); y0 = y0.min(p.y);
            x1 = x1.max(p.x); y1 = y1.max(p.y);
        }
        println!("stroke {i}: bbox=({x0:.1},{y0:.1})-({x1:.1},{y1:.1}) n={}", s.points.len());
    }
    panic!("dump complete");
}
```

Run: `nix develop -c cargo test -p rm-device _dump_calibration_strokes -- --ignored --nocapture`
Note the bbox of the first stroke on page 0. Record it in Step 2.

- [ ] **Step 2: Write the real test using the discovered bbox**

Replace the `#[ignore]` test with the asserting one. Substitute `<RECORDED_BBOX>` with what Step 1 printed:

```rust
#[test]
fn calibration_fixture_decodes_to_expected_pdf_region() {
    // Fixture: crates/inkapp-harness/tests/fixtures/recordings/calibration.rmdoc
    // (5-cross tap sheet, reMarkable Paper Pro Move, captured 2026-05-23).
    // The first stroke on page 0 hits the top-left cross.
    //
    // Recorded PDF-space bbox via _dump_calibration_strokes (deleted after
    // initial capture). 4pt tolerance per rm-device/src/lib.rs:27.
    let bytes = read_first_rm_from(
        "../inkapp-harness/tests/fixtures/recordings/calibration.rmdoc",
        0,
    );
    let rm = Remarkable::new();
    let strokes = rm.read_ink(&bytes, 841.89).expect("read_ink");
    assert!(!strokes.is_empty(), "calibration fixture decoded to zero strokes");

    let s = &strokes[0];
    let (mut x0, mut y0, mut x1, mut y1) =
        (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in &s.points {
        x0 = x0.min(p.x); y0 = y0.min(p.y);
        x1 = x1.max(p.x); y1 = y1.max(p.y);
    }

    // <RECORDED_BBOX> — substitute the actual numbers from Step 1.
    let (ex0, ey0, ex1, ey1) = (/* TBD from Step 1 */ 0.0, 0.0, 0.0, 0.0);
    let tol = 4.0;
    assert!((x0 - ex0).abs() < tol, "x0 drift {x0} vs {ex0}");
    assert!((y0 - ey0).abs() < tol, "y0 drift {y0} vs {ey0}");
    assert!((x1 - ex1).abs() < tol, "x1 drift {x1} vs {ex1}");
    assert!((y1 - ey1).abs() < tol, "y1 drift {y1} vs {ey1}");
}

fn read_first_rm_from(bundle_path: &str, page_index: usize) -> Vec<u8> {
    use rm_files::Bundle;
    let bundle = Bundle::open(std::path::Path::new(bundle_path))
        .expect("open bundle");
    let pages = bundle.pages();
    assert!(
        page_index < pages.len(),
        "fixture {bundle_path} has only {} pages, page {page_index} requested",
        pages.len()
    );
    pages[page_index]
        .scene_bytes()
        .expect("page has scene bytes")
        .to_vec()
}
```

**Note.** The `TBD from Step 1` placeholder is the *one* placeholder allowed in this plan because the values are observational — they cannot be predicted without running. Step 1 produces them and Step 2 substitutes them before the commit. Do not commit with `0.0` placeholders.

- [ ] **Step 3: Substitute the recorded values and remove the ignored helper**

Edit the test to use the bbox from Step 1's output. Delete the `#[ignore]`'d `_dump_calibration_strokes` test if you kept it as a separate function.

- [ ] **Step 4: Verify**

Run: `nix develop -c cargo test -p rm-device calibration_fixture_decodes_to_expected_pdf_region -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rm-device/tests/device.rs
git commit -m "tests(layer-2): calibration fixture decodes to expected PDF-space bbox"
```

---

## Task 6: `inkctl` lens parity at Layer 2

**Goal:** Verify the agent-facing lens (`inkctl`) agrees with the library at this layer. Concretely: there must be a way to feed raw `.rm` bytes (or an `.rmdoc` bundle path) into a session via `inkctl` and observe the strokes via `inkctl ink list`, and the result must equal what `Remarkable::read_ink` produces on the same bytes.

This task is the most likely place to hit the first `inkctl` bug — there is currently no verb that accepts raw `.rm` bytes (`ink fixture` takes a gesture-JSON name, `ink draw` takes PDF-space paths). The likely fix is to extend `ink fixture` to also accept a `.rmdoc` file path, or to add `ink load-rm`.

**Files:**
- Modify: `crates/inkctl/src/cmd/ink.rs` (extend `Fixture` or add a new subcommand)
- Modify: `crates/inkapp-harness/src/session.rs` (if `Session::ink_fixture` does not already accept a raw `.rmdoc` path)
- Create: `crates/inkctl/tests/lens_parity_layer2.rs`

**Acceptance Criteria:**
- [ ] `inkctl ink fixture <doc> <page> <region> <path.rmdoc>` (or `inkctl ink load-rm <doc> <page> <path.rm>` — implementer's choice; whichever fits the existing surface with the smallest delta) accepts a real `.rmdoc` / `.rm` fixture path and applies the strokes the library would.
- [ ] New test `crates/inkctl/tests/lens_parity_layer2.rs::layer2_lens_matches_library` does:
  1. Spawns a fresh session via the CLI.
  2. Publishes a smoke document.
  3. Applies the calibration fixture (page 0) via the new/extended verb.
  4. Calls `inkctl ink list --by-region` (or library-equivalent for the expected set) and parses the JSON output.
  5. Independently calls `Remarkable::read_ink` on the same fixture bytes via library.
  6. Asserts the stroke sets match: same length, point-wise equality within 1e-6.
- [ ] Test passes via `nix develop -c cargo test -p inkctl layer2_lens_matches_library`.
- [ ] Existing inkctl smoke tests still pass (`cargo test -p inkctl`).

**Verify:** `nix develop -c cargo test -p inkctl layer2_lens_matches_library -- --nocapture` → PASS.

**Steps:**

- [ ] **Step 1: Read the existing surface**

Read these files end-to-end before writing anything:

- `crates/inkctl/src/cmd/ink.rs` (the CLI subcommand definitions)
- `crates/inkapp-harness/src/session.rs` (look at `ink_fixture`, `ink_draw`, `pending_ink`)
- `crates/inkctl/tests/smoke_ink.rs` (the pattern for inkctl integration tests)

Decide whether to (a) extend `Cmd::Fixture` to switch on file extension, or (b) add a new `Cmd::LoadRm { doc_id, page, path }`. Pick the option with the smaller blast radius — typically (b), because changing `Cmd::Fixture` semantics risks breaking emitted-test backwards compatibility.

- [ ] **Step 2: Add the CLI subcommand**

In `crates/inkctl/src/cmd/ink.rs`, add to the `Cmd` enum (after `Draw`):

```rust
    /// Apply raw .rm bytes from a file (or a .rmdoc bundle's first .rm) directly.
    /// Bypasses gesture synthesis — used to feed real device recordings into a
    /// session for Layer-2 lens-parity testing.
    LoadRm {
        doc_id: String,
        page: usize,
        #[arg(long)]
        path: String,
    },
```

Add a `LoadRm { doc_id, page, path } => { ... }` arm in `run()` that:

1. Reads the bytes at `path` (if extension is `.rmdoc`, open the bundle and grab the first `.rm`; otherwise treat as raw `.rm`).
2. Resolves the doc's page height (via `session.document_describe(&doc_id).pages[page].height_pt` or equivalent).
3. Calls `session.ink_apply_rm_bytes(&doc_id, page, &bytes)` — or whatever name the harness gets in Step 3.
4. Prints `{ "ok": true, "data": { "applied": <stroke_count> } }`.

The exact code lives in this file but depends on what Step 3 produces. Write Step 3 first.

- [ ] **Step 3: Add the harness surface**

In `crates/inkapp-harness/src/session.rs`, add:

```rust
impl Session {
    /// Decode raw `.rm` bytes through the active device and stage the resulting
    /// strokes as pending ink on `(doc_id, page)`. Layer-2 surface — bypasses
    /// gesture synthesis.
    pub fn ink_apply_rm_bytes(
        &mut self,
        device: &DeviceId,
        doc_id: &str,
        page: usize,
        bytes: &[u8],
    ) -> std::io::Result<usize> {
        let page_h = self.page_height_pt(doc_id, page)?;
        let dev = self.device_impl(device)?; // existing accessor returning &dyn Device
        let strokes = dev
            .read_ink(bytes, page_h)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        self.append_pending_strokes(doc_id, page, strokes.clone())?;
        Ok(strokes.len())
    }
}
```

If `device_impl`, `page_height_pt`, or `append_pending_strokes` don't exist with those exact names, look in `session.rs` for the equivalents (`Session` already does this work for `ink_fixture` and `ink_draw` — copy the same path).

- [ ] **Step 4: Wire the CLI arm**

Back in `crates/inkctl/src/cmd/ink.rs`, complete the `LoadRm` arm:

```rust
        Cmd::LoadRm { doc_id, page, path } => {
            let bytes: Vec<u8> = if path.ends_with(".rmdoc") {
                let bundle = match rm_files::Bundle::open(std::path::Path::new(&path)) {
                    Ok(b) => b,
                    Err(e) => output::print_err("io_error", format!("open bundle: {e}")),
                };
                let pages = bundle.pages();
                let Some(scene) = pages.first().and_then(|p| p.scene_bytes()) else {
                    output::print_err("invalid_fixture", "bundle has no scene pages");
                };
                scene.to_vec()
            } else {
                match std::fs::read(&path) {
                    Ok(b) => b,
                    Err(e) => output::print_err("io_error", format!("read file: {e}")),
                }
            };
            let device_id = device_id.expect("LoadRm requires --device");
            let applied = session
                .ink_apply_rm_bytes(&device_id, &doc_id, page, &bytes)
                .unwrap_or_else(|e| output::print_err("apply_failed", e.to_string()));
            output::print_ok(json!({ "applied": applied }));
        }
```

Note: the exact `output::print_err` signature and `device_id` resolution must match how the other arms in this file work. Match the surrounding style, don't invent.

- [ ] **Step 5: Add the parity test**

Create `crates/inkctl/tests/lens_parity_layer2.rs`:

```rust
//! Layer-2 lens parity: `inkctl ink load-rm` followed by `inkctl ink list`
//! must report the same strokes that `Remarkable::read_ink` produces on the
//! same fixture bytes.

use std::process::Command;

const CALIBRATION: &str =
    "../inkapp-harness/tests/fixtures/recordings/calibration.rmdoc";

#[test]
fn layer2_lens_matches_library() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let inkctl_home = tmp.path().to_path_buf();

    // Helper to run inkctl with the temp home set.
    let run = |args: &[&str]| -> serde_json::Value {
        let out = Command::new(env!("CARGO_BIN_EXE_inkctl"))
            .args(args)
            .env("INKCTL_HOME", &inkctl_home)
            .output()
            .expect("spawn inkctl");
        assert!(
            out.status.success(),
            "inkctl {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout)
            .expect("inkctl stdout is JSON")
    };

    // Spin up a session + device + smoke doc.
    let session = run(&["session", "new"]);
    let session_id = session["data"]["id"].as_str().unwrap().to_string();
    let device = run(&[
        "--session", &session_id,
        "device", "new", "--backend", "fake",
    ]);
    let device_id = device["data"]["id"].as_str().unwrap().to_string();
    let doc = run(&[
        "--session", &session_id, "--device", &device_id,
        "document", "publish", "--app", "smoke",
    ]);
    let doc_id = doc["data"]["doc_id"].as_str().unwrap().to_string();

    // Apply the calibration fixture (page 0) via inkctl.
    let applied = run(&[
        "--session", &session_id, "--device", &device_id,
        "ink", "load-rm", &doc_id, "0", "--path", CALIBRATION,
    ]);
    let n_cli = applied["data"]["applied"].as_u64().unwrap() as usize;

    // Read the same fixture via library.
    let bundle = rm_files::Bundle::open(std::path::Path::new(CALIBRATION))
        .expect("open bundle");
    let pages = bundle.pages();
    let bytes = pages
        .first()
        .and_then(|p| p.scene_bytes())
        .expect("first page scene bytes")
        .to_vec();
    let rm = rm_device::Remarkable::new();
    let lib_strokes = rm
        .read_ink(&bytes, 841.89)
        .expect("library read_ink");

    assert_eq!(
        n_cli,
        lib_strokes.len(),
        "stroke count mismatch CLI={n_cli} vs library={}",
        lib_strokes.len()
    );

    // Fetch the strokes back via inkctl ink list and compare point-by-point.
    let listed = run(&[
        "--session", &session_id,
        "ink", "list", &doc_id, "0",
    ]);
    let cli_strokes = listed["data"]["strokes"]
        .as_array()
        .expect("strokes is array");
    assert_eq!(cli_strokes.len(), lib_strokes.len(), "list count");

    for (i, (cli, lib)) in cli_strokes.iter().zip(&lib_strokes).enumerate() {
        let cli_pts = cli["points"].as_array().expect("points array");
        assert_eq!(cli_pts.len(), lib.points.len(), "stroke {i} point count");
        for (j, (cp, lp)) in cli_pts.iter().zip(&lib.points).enumerate() {
            let cx = cp["x"].as_f64().unwrap();
            let cy = cp["y"].as_f64().unwrap();
            assert!(
                (cx - lp.x).abs() < 1e-6 && (cy - lp.y).abs() < 1e-6,
                "stroke {i} point {j}: CLI=({cx},{cy}) lib=({},{})",
                lp.x, lp.y
            );
        }
    }
}
```

If `inkctl ink list`'s JSON shape differs from `data.strokes[].points[].{x,y}`, adjust the field names to match the actual output. Run the CLI by hand once to confirm:

```bash
INKCTL_HOME=/tmp/test inkctl ink list <doc_id> 0
```

- [ ] **Step 6: Add `tempfile`, `serde_json`, and `rm-files` / `rm-device` as test deps**

Check `crates/inkctl/Cargo.toml`'s `[dev-dependencies]`. Add anything missing:

```toml
[dev-dependencies]
tempfile = "3"
serde_json = "1"
rm-files = { path = "../rm-files" }
rm-device = { path = "../rm-device" }
```

- [ ] **Step 7: Verify**

Run: `nix develop -c cargo test -p inkctl layer2_lens_matches_library -- --nocapture`
Expected: PASS.

Run: `nix develop -c cargo test -p inkctl`
Expected: all tests pass (no regression).

- [ ] **Step 8: Commit (fix + test, separately if the inkctl change was a real bug fix)**

```bash
git add crates/inkctl/src/cmd/ink.rs crates/inkapp-harness/src/session.rs crates/inkctl/Cargo.toml
git commit -m "fix(inkctl): add ink load-rm verb for raw .rm / .rmdoc fixtures (layer-2 lens gap)"

git add crates/inkctl/tests/lens_parity_layer2.rs
git commit -m "tests(layer-2): inkctl ink load-rm matches Remarkable::read_ink on calibration fixture

Exposes the gap closed by the preceding commit (inkctl ink load-rm)."
```

---

## Task 7: Mark Layer 2 covered in `docs/appdx.md`

**Goal:** The spec's "definition of done" requires updating `docs/appdx.md` to reflect what's now built/trusted. This task lands that update and signals layer-promotion.

**Files:**
- Modify: `docs/appdx.md` (the section that lists test coverage / inkctl status — locate via grep)

**Acceptance Criteria:**
- [ ] `docs/appdx.md` names Layer 2 as covered with a short bullet list of what's tested: transform invertibility (multi-geometry), off-page contract, text-highlight synthesis, fixture decode, and inkctl lens parity via `ink load-rm`.
- [ ] No "TBD"/"TODO" markers introduced.
- [ ] Workspace builds and tests pass: `make test` and `make clippy`.

**Verify:**
1. `grep -i "layer 2" docs/appdx.md` shows the new section.
2. `nix develop -c cargo test --workspace`
3. `nix develop -c cargo clippy --all-targets -- -D warnings`

**Steps:**

- [ ] **Step 1: Find the right section in appdx.md**

Run:

```bash
grep -nE "^#+ |Layer|inkctl|test harness" docs/appdx.md | head -40
```

Pick the section that documents the testing layers (the inkctl harness section the spec mentions). If no per-layer section exists, add one immediately after the "Agent-drivable test harness (inkctl)" section.

- [ ] **Step 2: Write the entry**

Add (or extend) a subsection:

```markdown
### Test coverage by layer

Tracks the layered testing program from
[spec 2026-05-27](superpowers/specs/2026-05-27-reader-thorough-test-design.md).

- **Layer 2 — device coord transform.** Covered 2026-05-27.
  - `rm-device` PDF↔device transform inverts across the inkapp-default
    (560pt) and A4 (841.89pt) geometries at multiple sample points
    (`crates/rm-device/tests/device.rs::transform_is_invertible_across_geometries`).
  - Off-page strokes round-trip without clamping or dropping; contract
    pinned on the `Device` trait
    (`off_page_strokes_round_trip_without_clamping`,
    `read_ink_does_not_drop_off_canvas_points`).
  - `GlyphRange` text-highlight rects synthesize 17-point horizontal
    swipes through the rect's vertical midpoint
    (`text_highlight_rect_synthesizes_swipe`).
  - Real-recording fixture decodes to expected PDF-space bbox within
    the 4pt residual budget
    (`calibration_fixture_decodes_to_expected_pdf_region`).
  - `inkctl ink load-rm` matches `Remarkable::read_ink` on the same
    fixture bytes
    (`crates/inkctl/tests/lens_parity_layer2.rs::layer2_lens_matches_library`).
  - inkctl gap closed during this layer: `ink load-rm` verb added
    (previously no inkctl path accepted raw `.rm` / `.rmdoc` bytes).
```

- [ ] **Step 3: Workspace-wide verify**

Run: `nix develop -c cargo test --workspace`
Expected: all tests across all crates pass.

Run: `nix develop -c cargo clippy --all-targets -- -D warnings`
Expected: clean.

Run: `nix develop -c cargo fmt --check`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add docs/appdx.md
git commit -m "docs(appdx): layer 2 (device coord transform) covered"
```

---

## Self-review checklist (run after writing each task)

- No `TBD` / `TODO` markers in committed files. (Step 3 of Task 5 explicitly substitutes its observational placeholder.)
- Every test in the plan has a concrete `Verify` command with expected outcome.
- Cross-task type consistency: `read_first_rm_with_glyph` (Task 4) and `read_first_rm_from` (Task 5) and the `lens_parity_layer2.rs` helper (Task 6) all rely on the same `rm_files::bundle::*` API discovered in Task 4 Step 1; if names differ, fix them everywhere in one pass.
- `inkctl` JSON output field names assumed in Task 6 (`data.strokes[].points[].{x,y}`) are verified against actual CLI output before the test is committed.
- Spec coverage: every inventory bullet in the spec's Layer 2 section maps to a task here (1 → Task 1, fixture decode → Task 5, off-page → Task 3, text-highlight → Task 4, lens parity → Task 6).

## Out of scope for this plan

- Layer 3+ work (separate plans, written after this layer goes green).
- Adding a writer for `GlyphRange` items in `rm-files` (the round-trip is via a real fixture instead).
- Any change to the reMarkable canvas scale constants (would require a new on-device calibration capture — flagged via the comment fix in Task 2).
