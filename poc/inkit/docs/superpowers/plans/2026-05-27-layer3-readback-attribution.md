# Layer 3 — Readback / attribution: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development to implement task-by-task. Steps use `- [ ]` checkbox syntax. **Do NOT call `TaskCreate` / `TaskUpdate` for the tasks in this plan** — the plan-file checkboxes are the source of truth. The session-scoped pre-commit hook counts native tasks as "open work" and blocks per-task commits if any are pending; using only checkboxes avoids that fight.

**Goal:** Prove `inkapp-core::readback::attribute` / `attribute_page` / `diff_new` / `guard_version` with committed Rust tests and verify `inkctl`'s by-region lens (`inkctl ink list --by-region`, backed by `inkapp_harness::observe::stroke_region`) agrees with the library on the same inputs. Fix the known midpoint-vs-all-points divergence surfaced by the parity test.

**Architecture:** Library tests live in `crates/inkapp-core/tests/readback.rs` (extend the existing file). Lens-parity tests live in `crates/inkctl/tests/lens_parity_layer3.rs`. The lens fix replaces `observe::stroke_region`'s ad-hoc containment with a call into the library's `attribute()`, eliminating the divergence at its root.

**Tech Stack:** Rust, `cargo test`, `inkapp_core::readback`, `inkapp_harness::observe`, `inkctl`.

**Spec:** [docs/superpowers/specs/2026-05-27-reader-thorough-test-design.md](../specs/2026-05-27-reader-thorough-test-design.md). This is the Layer-3 plan; Layer 2 is shipped (commits 4fc25a8, d83e6f7); Layer 4–6 plans follow this one.

---

## Pre-flight: what already exists

**Existing tests** in `crates/inkapp-core/tests/readback.rs`:

- `attributes_strokes_to_regions` — single-point strokes wholly inside / outside.
- `stroke_in_overlap_is_attributed_to_both_regions` — point inside overlapping rects.
- `diff_returns_only_new_strokes` — exact-equality set diff.
- `stale_version_is_rejected` — guard_version round-trip.
- `split_region_stitches_across_pages` — same-name regions on different pages.
- `no_cross_page_attribution` — region on page N doesn't match strokes from page M.
- `split_region_with_ink_on_only_one_page_still_stitches`.
- `attribute_page_is_single_page_wrapper`.

**Gaps vs. Layer-3 spec inventory:**

1. No test for a **multi-point stroke whose first point is inside region A and last point is outside any region** — does the stroke attribute (since any-point-in-rect matches) or not? Pinned behavior unclear.
2. No test for a **multi-point stroke straddling two non-overlapping regions** (one point in A, another in B) — does it attribute to both?
3. The existing `attributes_strokes_to_regions` notes "the (100,100) stroke matches no region and is dropped" but no dedicated test names that drop as the contract. Layer 3 spec said "unattributed bucket, not silently dropped." Current behavior is **drop**. We pin the drop and flag the spec discrepancy in `docs/appdx.md`.
4. **No lens-parity test.** And there's a known divergence (see "The Bug" below).

**The Bug** (`crates/inkapp-harness/src/observe.rs`, `stroke_region`):

```rust
fn stroke_region(s: &Stroke, manifest: &Manifest, page: usize) -> Option<String> {
    let mid = s.points.get(s.points.len() / 2)?;        // midpoint only
    for r in &manifest.regions {
        if r.page != page { continue; }
        if mid.x >= r.rect.x0 && ... return Some(r.name);  // first match wins
    }
    None
}
```

Compare with the library (`crates/inkapp-core/src/readback.rs`, `attribute`):

```rust
for s in strokes {
    if s.points.iter().any(|p| region.rect.contains(p.x, p.y)) {  // any point
        // ... appends to EVERY matching region
    }
}
```

Divergence:
- **Containment:** midpoint-only vs. any-point.
- **Multi-region:** first-match-wins vs. attributed-to-all-matching.

A stroke whose first/last point is in a region but whose midpoint is outside attributes via the library and is invisible to the lens. A stroke landing across two overlapping regions attributes to both via the library and to only one (whichever appears first in the manifest order) via the lens.

The root-cause fix is to have `observe::ink_list`'s by-region grouping call `attribute()` directly instead of using its own helper.

---

## Task 1 — Strengthen library readback tests

- [x] **Goal:** Pin the library's behavior on three previously-untested cases. All edits live in `crates/inkapp-core/tests/readback.rs`.

**Files:** `crates/inkapp-core/tests/readback.rs`

**Acceptance:**
- New test `multi_point_stroke_attributes_if_any_point_in_region` passes.
- New test `multi_point_stroke_in_two_regions_attributes_to_both` passes.
- New test `stroke_outside_all_regions_is_dropped` passes and documents the drop contract.
- `nix develop -c cargo test -p inkapp-core readback` passes (all readback tests).

**Verify:** `nix develop -c cargo test -p inkapp-core --test readback`

**Steps:**

- [x] **Step 1:** Append to `crates/inkapp-core/tests/readback.rs`:

```rust
fn multi_point(points: &[(f64, f64)]) -> Stroke {
    Stroke {
        points: points
            .iter()
            .map(|(x, y)| PdfPoint { x: *x, y: *y })
            .collect(),
        highlighter: false,
    }
}

#[test]
fn multi_point_stroke_attributes_if_any_point_in_region() {
    // Region "a" = [0,0]-[10,10]. A 3-point stroke starts outside, dips into
    // the region at the midpoint, then exits. Library contract: ANY point in
    // the region → attributed.
    let m = Manifest {
        version: 1,
        regions: vec![Region {
            name: "a".into(),
            page: 0,
            rect: rect(0.0, 0.0, 10.0, 10.0),
        }],
        ..Default::default()
    };
    let s = multi_point(&[(-5.0, 5.0), (5.0, 5.0), (15.0, 5.0)]);
    let ink = attribute_page(&[s], &m);
    let a = ink.iter().find(|ri| ri.region == "a").expect("attributed to a");
    assert_eq!(a.strokes.len(), 1, "single stroke attributed once");
}

#[test]
fn multi_point_stroke_in_two_regions_attributes_to_both() {
    // Two NON-overlapping regions; a single stroke has one point in each.
    // Library contract: stroke appears in both region buckets.
    let m = Manifest {
        version: 1,
        regions: vec![
            Region { name: "a".into(), page: 0, rect: rect(0.0, 0.0, 10.0, 10.0) },
            Region { name: "b".into(), page: 0, rect: rect(50.0, 50.0, 60.0, 60.0) },
        ],
        ..Default::default()
    };
    let s = multi_point(&[(5.0, 5.0), (30.0, 30.0), (55.0, 55.0)]);
    let ink = attribute_page(&[s], &m);
    let a = ink.iter().find(|ri| ri.region == "a").expect("attributed to a");
    let b = ink.iter().find(|ri| ri.region == "b").expect("attributed to b");
    assert_eq!(a.strokes.len(), 1);
    assert_eq!(b.strokes.len(), 1);
    assert_eq!(
        ink.iter().map(|ri| ri.strokes.len()).sum::<usize>(),
        2,
        "stroke attributed to both regions (one in each bucket)"
    );
}

#[test]
fn stroke_outside_all_regions_is_dropped() {
    // Contract: a stroke that matches no region is dropped from the output —
    // there is no "unattributed" bucket in the current library design. Tests
    // pin this so any future change (e.g. spec said "unattributed bucket")
    // is a deliberate behavior change, not a silent regression.
    let m = manifest(); // version 3, regions "a"=[0,0]-[10,10], "b"=[20,20]-[30,30]
    let strokes = vec![stroke(100.0, 100.0), stroke(200.0, 200.0)];
    let ink = attribute_page(&strokes, &m);
    assert_eq!(
        ink.iter().map(|ri| ri.strokes.len()).sum::<usize>(),
        0,
        "all strokes outside every region — dropped"
    );
    // Note: the output may still contain region entries with empty stroke
    // vectors for regions named in the manifest; that's fine. We assert on
    // total stroke count, not on `ink.len()`.
}
```

- [x] **Step 2:** Run `nix develop -c cargo fmt -p inkapp-core`.

- [x] **Step 3:** Run `nix develop -c cargo test -p inkapp-core --test readback`. All readback tests (existing 8 + new 3) should pass.

- [x] **Step 4:** Update the checkbox markers in this plan file to `[x]` for Task 1 (steps + the task heading).

**DO NOT COMMIT.** Layer-3 work piles into the working tree; one combined commit at the end.

---

## Task 2 — Lens-parity test + root-cause fix

- [x] **Goal:** Write a parity test that drives ink via the harness library, observes via `inkctl ink list --by-region`, and asserts the by-region grouping equals what `attribute()` would produce on the same input. The test will fail against current `observe::stroke_region` because of midpoint/first-match divergence. Then fix `observe::ink_list` to use `attribute()` directly, eliminating the divergence at its root. Test passes.

**Files:**
- Create: `crates/inkctl/tests/lens_parity_layer3.rs`
- Modify: `crates/inkapp-harness/src/observe.rs` (`ink_list`'s by-region path + delete `stroke_region`)

**Acceptance:**
- `lens_parity_layer3::layer3_by_region_matches_attribute` passes.
- `observe::stroke_region` is removed (dead code after the fix); `observe::ink_list` calls `inkapp_core::readback::attribute_page` for the by-region bucket.
- All existing `inkapp-harness` tests still pass.
- All existing `inkctl` tests still pass.

**Verify:**
- `nix develop -c cargo test -p inkctl layer3_by_region_matches_attribute`
- `nix develop -c cargo test -p inkapp-harness`
- `nix develop -c cargo test -p inkctl`

**Steps:**

- [x] **Step 1: Write the test (it will fail at first).** Create `crates/inkctl/tests/lens_parity_layer3.rs`:

```rust
//! Layer-3 lens parity: `inkctl ink list --by-region` must group strokes
//! the same way the library's `readback::attribute_page` does. Specifically:
//!   - any-point-in-rect (not midpoint-only)
//!   - multi-region attribution (a stroke can appear in two region buckets)

use std::collections::BTreeMap;
use std::process::Command;

#[test]
fn layer3_by_region_matches_attribute() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let inkctl_home = tmp.path().to_path_buf();

    let run = |args: &[&str]| -> serde_json::Value {
        let out = Command::new(env!("CARGO_BIN_EXE_inkctl"))
            .args(args)
            .env("INKCTL_HOME", &inkctl_home)
            .output()
            .expect("spawn inkctl");
        assert!(
            out.status.success(),
            "inkctl {args:?} failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).expect("inkctl stdout is JSON")
    };

    // Spin up session + device + smoke doc.
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

    // The "smoke" app exposes at least one named region per page. We don't
    // need to know its name in advance — query it.
    let described = run(&[
        "--session", &session_id,
        "page", "describe", &doc_id, "0",
    ]);
    let regions = described["data"]["regions"].as_array().expect("regions");
    assert!(!regions.is_empty(), "smoke app should publish at least one region");

    // Take the first region and synthesize a multi-point stroke that has its
    // MIDPOINT outside the region but FIRST/LAST points inside. This is the
    // pathological case that the buggy lens (midpoint-only) misses.
    let r = &regions[0];
    let r_name = r["name"].as_str().unwrap().to_string();
    let x0 = r["rect"]["x0"].as_f64().unwrap();
    let y0 = r["rect"]["y0"].as_f64().unwrap();
    let x1 = r["rect"]["x1"].as_f64().unwrap();
    let y1 = r["rect"]["y1"].as_f64().unwrap();
    let cx_in = (x0 + x1) / 2.0;
    let cy_in = (y0 + y1) / 2.0;
    // A 3-point stroke: in -> way outside -> in.
    let stroke_json = serde_json::json!([
        { "x": cx_in, "y": cy_in },
        { "x": x1 + 1000.0, "y": y1 + 1000.0 },
        { "x": cx_in, "y": cy_in + 0.5 },
    ]);

    // Apply via inkctl ink draw (PDF-space stroke).
    run(&[
        "--session", &session_id, "--device", &device_id,
        "ink", "draw", &doc_id, "0",
        "--path", &stroke_json.to_string(),
    ]);

    // Observe via inkctl ink list --by-region.
    let listed = run(&[
        "--session", &session_id,
        "ink", "list", &doc_id, "0",
        "--by-region",
    ]);
    let by_region = listed["data"]["by_region"]
        .as_object()
        .expect("by_region object");
    let cli_strokes_in_r: usize = by_region
        .get(&r_name)
        .map(|v| v.as_array().map(|a| a.len()).unwrap_or(0))
        .unwrap_or(0);

    // Library answer on the same stroke: since the first and last points are
    // inside region r, `attribute()` puts the stroke in r's bucket. Expected
    // count = 1.
    assert_eq!(
        cli_strokes_in_r, 1,
        "inkctl by-region missed a stroke whose endpoints land inside the region \
         (likely midpoint-only containment in observe::stroke_region)"
    );

    // Also verify the order doesn't matter: drop another stroke whose midpoint
    // is inside but endpoints are outside (the OPPOSITE pathology).
    let stroke2 = serde_json::json!([
        { "x": x1 + 1000.0, "y": y1 + 1000.0 },
        { "x": cx_in, "y": cy_in },
        { "x": x1 + 1001.0, "y": y1 + 1001.0 },
    ]);
    run(&[
        "--session", &session_id, "--device", &device_id,
        "ink", "draw", &doc_id, "0",
        "--path", &stroke2.to_string(),
    ]);
    let listed2 = run(&[
        "--session", &session_id,
        "ink", "list", &doc_id, "0",
        "--by-region",
    ]);
    let by_region2 = listed2["data"]["by_region"].as_object().expect("by_region");
    let cli_strokes2: usize = by_region2
        .get(&r_name)
        .map(|v| v.as_array().map(|a| a.len()).unwrap_or(0))
        .unwrap_or(0);
    assert_eq!(
        cli_strokes2, 2,
        "second stroke (midpoint-inside) should also attribute"
    );

    // Silence unused warning on BTreeMap import in some toolchains.
    let _ = std::mem::size_of::<BTreeMap<String, ()>>();
}
```

If `crates/inkctl/Cargo.toml` doesn't already have `tempfile` / `serde_json` under `[dev-dependencies]`, add them (Layer 2 added these — they should be present).

If `inkctl ink draw`'s `--path` argument shape is different from what's used above (e.g. expects a file path with JSON inside, not a JSON literal), adjust to match. Look at `crates/inkctl/src/cmd/ink.rs::Cmd::Draw` to confirm.

- [x] **Step 2: Run the test — it MUST fail.** `nix develop -c cargo test -p inkctl layer3_by_region_matches_attribute`. Confirm failure mode is one of: `cli_strokes_in_r != 1` (midpoint-outside missed) OR `cli_strokes2 != 2`. This proves the test exercises the bug.

- [x] **Step 3: Fix the lens.** In `crates/inkapp-harness/src/observe.rs`:

```rust
        ObserveGroup::ByRegion => {
            let (_, manifest) = load_doc(session.state_dir(), doc_id)?;
            // Build the same per-page strokes view the library uses, with
            // `all` placed at index `page` and empty vectors elsewhere up to
            // the maximum region page in the manifest.
            let max_page = manifest
                .regions
                .iter()
                .map(|r| r.page)
                .max()
                .unwrap_or(page);
            let mut pages: Vec<Vec<inkapp_core::ink::Stroke>> =
                vec![Vec::new(); max_page.max(page) + 1];
            pages[page] = all.clone();
            let region_inks = inkapp_core::readback::attribute(&pages, &manifest);
            let mut m: std::collections::BTreeMap<String, Vec<Stroke>> = Default::default();
            for ri in region_inks {
                if !ri.strokes.is_empty() {
                    m.insert(ri.region, ri.strokes);
                }
            }
            (None, Some(m))
        }
```

And **delete** the `fn stroke_region(...)` helper — it has no other callers (`grep` to confirm). If it does, replace those calls with `attribute_page` too.

The dep `inkapp-core` is already in `inkapp-harness`'s `Cargo.toml` — no Cargo edit needed.

- [x] **Step 4: Run the test — it MUST now pass.** `nix develop -c cargo test -p inkctl layer3_by_region_matches_attribute`.

- [x] **Step 5: Run the broader suites.** `nix develop -c cargo test -p inkapp-harness` and `nix develop -c cargo test -p inkctl` — all green.

- [x] **Step 6:** `nix develop -c cargo fmt -p inkapp-harness -p inkctl`.

- [x] **Step 7:** Update this plan's checkboxes for Task 2 to `[x]`.

**DO NOT COMMIT yet** — combined with Task 3 below.

---

## Task 3 — appdx update + workspace verify + commit

- [x] **Goal:** Document Layer 3 coverage in `docs/appdx.md`, run workspace-wide verification, then commit everything in one shot.

**Files:**
- Modify: `docs/appdx.md` (extend the "Test coverage by layer" subsection added in Layer 2)
- Modify: this plan file (mark all task headings `[x]`)

**Verify:** all three must pass:
- `nix develop -c cargo fmt --check`
- `nix develop -c cargo test --workspace`
- `nix develop -c cargo clippy --all-targets -- -D warnings`

**Steps:**

- [x] **Step 1:** Append to the "Test coverage by layer" subsection in `docs/appdx.md` (find it under the inkctl section, where Layer 2 was added):

```markdown
- **Layer 3 — readback / attribution.** Covered 2026-05-27.
  - Library `attribute_page` / `attribute` already pinned by 8 existing
    tests in `crates/inkapp-core/tests/readback.rs`; this layer adds three
    more:
    - `multi_point_stroke_attributes_if_any_point_in_region` — any-point
      containment, not midpoint-only.
    - `multi_point_stroke_in_two_regions_attributes_to_both` — a stroke
      whose points span two non-overlapping regions attributes to both.
    - `stroke_outside_all_regions_is_dropped` — pins drop behavior (no
      "unattributed bucket" in the current library design; revisit if a
      higher layer needs visibility into unattributed ink).
  - `inkctl ink list --by-region` matches `readback::attribute` on the
    same inputs (`crates/inkctl/tests/lens_parity_layer3.rs::layer3_by_region_matches_attribute`).
  - Harness lens gap closed during this layer: `observe::ink_list`'s
    by-region path now calls `inkapp_core::readback::attribute` directly
    instead of a midpoint/first-match helper. Previously, strokes whose
    midpoint was outside a region but whose endpoints landed inside it
    were invisible to the lens; strokes landing in overlapping regions
    appeared only in the first one. Both are now correct.
```

- [x] **Step 2:** Mark all `[ ]` task headings in this plan file as `[x]`.

- [x] **Step 3: Verify workspace.**

```bash
nix develop -c cargo fmt --check
nix develop -c cargo test --workspace
nix develop -c cargo clippy --all-targets -- -D warnings
```

If `fmt --check` fails: `nix develop -c cargo fmt` and re-check. If clippy fails on Layer-3-introduced code: fix it. If it fails on pre-existing drift: STOP and report.

- [x] **Step 4: Inspect staged set.** `git status` should show:

```
M crates/inkapp-core/tests/readback.rs
M crates/inkapp-harness/src/observe.rs
A crates/inkctl/tests/lens_parity_layer3.rs
M docs/appdx.md
A docs/superpowers/plans/2026-05-27-layer3-readback-attribution.md
```

Anything else (apps/reader/*, inkapp-content/*, components/*) is pre-existing main-branch drift that bled into the worktree and MUST NOT be staged.

- [x] **Step 5: Commit.**

```bash
git add crates/inkapp-core/tests/readback.rs crates/inkapp-harness/src/observe.rs crates/inkctl/tests/lens_parity_layer3.rs docs/appdx.md docs/superpowers/plans/2026-05-27-layer3-readback-attribution.md

git commit -m "$(cat <<'EOF'
tests(layer-3): readback attribution + inkctl by-region lens parity

Closes Layer-3 coverage per docs/superpowers/specs/2026-05-27-reader-thorough-test-design.md
(plan: docs/superpowers/plans/2026-05-27-layer3-readback-attribution.md).

- inkapp-core readback tests: pin any-point containment for multi-point
  strokes, multi-region attribution across non-overlapping regions, and
  drop-when-no-region-matches.
- inkapp-harness fix: observe::ink_list's by-region grouping now calls
  inkapp_core::readback::attribute directly. The old stroke_region
  helper used midpoint-only containment with first-match-wins, which
  silently dropped strokes whose endpoints (not midpoint) landed inside
  a region and never showed a stroke in two overlapping regions. Both
  divergences from the library are now eliminated.
- inkctl lens_parity_layer3 test: drives ink whose midpoint sits outside
  a region but whose endpoints land inside, then drives the inverse;
  asserts both attribute to the region via inkctl ink list --by-region.
- docs/appdx.md: Test coverage by layer subsection, Layer 3 covered.
EOF
)"
```

- [x] **Step 6:** Verify the commit landed: `git log -1 --stat`.

---

## Self-review checklist

- No `TBD` / `TODO` markers committed.
- The test added in Task 2 *must* fail before the fix in the same task. If it passes immediately, the test does not actually exercise the bug — fix the test.
- `observe::stroke_region` is removed (no callers remain) — verify with `grep -rn stroke_region crates/`.
- The commit set is exactly the 5 files listed in Task 3 Step 4; nothing else.
- `make clippy` is clean.

## Out of scope

- Layer 4 (components in isolation) — separate plan after this lands.
- Any redesign of the "unattributed bucket" question — current behavior is drop; if a higher layer needs visibility into dropped strokes, that's a deliberate spec change for that layer.
