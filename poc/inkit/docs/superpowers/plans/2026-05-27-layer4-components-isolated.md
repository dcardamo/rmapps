# Layer 4 — Components in isolation: implementation plan

> **For agentic workers:** Use superpowers-extended-cc:subagent-driven-development. **Do NOT call `TaskCreate` / `TaskUpdate`** — the plan-file checkboxes are the only tracker. The session-scoped pre-commit hook blocks per-task commits when native tasks are pending, so we use plain checkboxes instead. One combined commit at end of layer.

**Goal:** Close the remaining gaps in `inkapp-core` component coverage (per the spec's Layer-4 inventory), restricted to components NOT currently under user WIP. Verify the agent-facing lens (`inkctl page describe`) reports the same regions the library recovers for a freshly-compiled component.

**Architecture:** Tests stay in the natural homes — `crates/inkapp-core/src/components/stack.rs` inline tests (extend) and `crates/inkctl/tests/lens_parity_layer4.rs` (new). The fix-as-you-go discipline from Layer 3 carries over: any inkctl/harness divergence found in the lens test gets fixed in the same change-set as the test that exposed it.

**Tech Stack:** Rust, `cargo test`, `inkapp-core::components`, `inkctl`, `inkapp-harness`.

**Spec:** [docs/superpowers/specs/2026-05-27-reader-thorough-test-design.md](../specs/2026-05-27-reader-thorough-test-design.md). Layers 2 + 3 are shipped; this is Layer 4. Layers 5 + 6 follow.

---

## Scope and constraint

**In scope (no WIP):** `Stack`, `GestureAction` (audit only — already 8 tests), `HighlightText` (audit only — already 6 tests), `Index` (audit only — already 13 inline tests), plus the inkctl `page describe` lens parity.

**Out of scope (user has uncommitted edits):** `ActionBand`, `NavBand`, `heading.rs` / `HeadingComponent`, `section.rs` / `Section`. These get a Layer-4 follow-up plan once user WIP lands.

**Spec inventory mapping for Layer 4:**

| Spec inventory item                                                     | Status            |
|-------------------------------------------------------------------------|-------------------|
| `GestureAction` — tap inside / outside / edge → correct `Msg`           | ✅ existing 8 tests |
| `ActionBand` — region tappable; consistent across pages                 | ⏸ deferred (WIP)  |
| `HighlightText` — highlight stroke yields `Msg`; region shape correct   | ✅ existing 6 tests |
| `NavBand` — Prev/Home/Next; disabled states                             | ⏸ deferred (WIP)  |
| `Stack` composer — children's regions don't collide; decode forwarded   | ⚠ gap → Task 1     |
| `Index` (compact-row) — row taps map to entry; masthead non-interactive | ✅ existing 13 tests |
| inkctl lens: `page inspect`/`describe` overlay matches component regions| ⚠ gap → Task 2     |

---

## Task 1 — Strengthen `Stack` tests

- [x] **Goal:** Close the three gaps in `Stack`'s inline tests: (a) decode forwards messages from a child that *actually* emits messages, (b) `typst_sources` aggregates across children, (c) `image_urls` aggregates across children, (d) two children's regions are both recoverable from a Stack-rendered doc (no collisions).

**Files:** `crates/inkapp-core/src/components/stack.rs` (extend the inline `mod tests`).

**Acceptance:**
- Test `decode_forwards_messages_from_children` exists; uses `GestureAction` (which actually emits messages) as a Stack child; a region-spanning pen strike on the gesture's region produces the gesture's message via `Stack::decode`.
- Test `typst_sources_aggregates_across_children` exists; passes.
- Test `image_urls_aggregates_across_children` exists; passes.
- Test `children_regions_both_recover_under_stack` exists; uses real Typst compilation + `recover_regions` to confirm two children stacked into one render each contribute their own region.
- `nix develop -c cargo test -p inkapp-core --test stack` (or the inline test runner) passes; existing 2 tests untouched.

**Verify:** `nix develop -c cargo test -p inkapp-core stack::tests`

**Steps:**

- [x] **Step 1: Inspect `Notice` and `GestureAction`'s `image_urls`** to find the simplest seed for a "has image_urls" child:

```bash
grep -nE "fn image_urls|impl Component for" /home/dan/git/inkapp/.worktrees/layer-4/crates/inkapp-core/src/components/*.rs | head -30
```

Pick a component whose `image_urls` returns at least one entry. If `Image` / `image_component` exists, use it; otherwise, construct one Stack child whose `image_urls()` returns a known string (e.g. directly via a mock `Component` impl in the test module).

- [x] **Step 2: Append to the existing `mod tests` in `crates/inkapp-core/src/components/stack.rs`.** The exact code below uses a mock `Component` for the `typst_sources` / `image_urls` aggregation tests (sturdy against future component changes) and uses `GestureAction` for the decode-forwarding test:

```rust
    use crate::components::gesture::GestureAction;
    use crate::document::Document;
    use crate::flow;
    use crate::geometry::{PageGeom, PdfPoint};
    use crate::ink::Stroke;
    use crate::manifest::{recover_regions, Region};
    use crate::readback::attribute_page;
    use crate::runtime::compile_document_in;
    use crate::Theme;

    // A minimal mock Component used for aggregation tests: lets each child
    // declare its own typst_sources / image_urls so we can assert the Stack
    // wires them through cleanly without being coupled to a real component's
    // current behaviour.
    struct Mock {
        sources: Vec<(String, String)>,
        images: Vec<String>,
    }
    impl Component for Mock {
        type Msg = ();
        fn render(&self, _cx: &mut RenderCx) -> String {
            String::new()
        }
        fn typst_sources(&self) -> Vec<(String, String)> {
            self.sources.clone()
        }
        fn decode(&self, _ink: &[RegionInk], _m: &Manifest) -> Vec<()> {
            Vec::new()
        }
        fn image_urls(&self) -> Vec<String> {
            self.images.clone()
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    enum M {
        Hit,
    }

    #[test]
    fn decode_forwards_messages_from_children() {
        // Stack a GestureAction child; render real Typst, recover the title
        // region, build a region-spanning pen strike, and feed it through the
        // Stack's decode. The expected output is the gesture's message.
        let doc: Document<M> = Document::keyed(
            "d",
            flow![Stack::new(vec![Box::new(GestureAction::with_msg(
                "title",
                "How CGI changed the web",
                M::Hit
            ))])],
        );
        let compiled =
            compile_document_in(&doc, PageGeom::default(), &Theme::reader()).unwrap();
        let manifest = recover_regions(&compiled).unwrap();
        let r: &Region = manifest
            .regions
            .iter()
            .find(|r| r.name == "title")
            .expect("title region recovered through Stack");
        let cy = (r.rect.y0 + r.rect.y1) / 2.0;
        let stroke = Stroke {
            points: vec![
                PdfPoint { x: r.rect.x0, y: cy },
                PdfPoint { x: r.rect.x1, y: cy },
            ],
            highlighter: false,
        };
        let ink = attribute_page(&[stroke], &manifest);
        let decoded = doc.flow[0].decode(&ink, &manifest);
        assert_eq!(decoded, vec![M::Hit], "Stack forwards child's Msg");
    }

    #[test]
    fn typst_sources_aggregates_across_children() {
        let s: Stack<()> = Stack::new(vec![
            Box::new(Mock {
                sources: vec![("a.typ".into(), "let a = 1".into())],
                images: vec![],
            }),
            Box::new(Mock {
                sources: vec![
                    ("b.typ".into(), "let b = 2".into()),
                    ("c.typ".into(), "let c = 3".into()),
                ],
                images: vec![],
            }),
        ]);
        let out = s.typst_sources();
        assert_eq!(out.len(), 3, "all three sources collected");
        assert_eq!(out[0].0, "a.typ");
        assert_eq!(out[1].0, "b.typ");
        assert_eq!(out[2].0, "c.typ");
    }

    #[test]
    fn image_urls_aggregates_across_children() {
        let s: Stack<()> = Stack::new(vec![
            Box::new(Mock {
                sources: vec![],
                images: vec!["https://example.com/1.png".into()],
            }),
            Box::new(Mock {
                sources: vec![],
                images: vec![
                    "https://example.com/2.png".into(),
                    "https://example.com/3.png".into(),
                ],
            }),
        ]);
        let urls = s.image_urls();
        assert_eq!(urls.len(), 3);
        assert!(urls.iter().any(|u| u.ends_with("/1.png")));
        assert!(urls.iter().any(|u| u.ends_with("/3.png")));
    }

    #[test]
    fn children_regions_both_recover_under_stack() {
        // Two GestureAction children with DIFFERENT region names, stacked.
        // Both regions must come back from recover_regions — proving Stack
        // does not collapse, deduplicate, or otherwise lose a child's region.
        let doc: Document<M> = Document::keyed(
            "d",
            flow![Stack::new(vec![
                Box::new(GestureAction::with_msg(
                    "alpha",
                    "first action",
                    M::Hit
                )),
                Box::new(GestureAction::with_msg(
                    "beta",
                    "second action",
                    M::Hit
                )),
            ])],
        );
        let compiled =
            compile_document_in(&doc, PageGeom::default(), &Theme::reader()).unwrap();
        let manifest = recover_regions(&compiled).unwrap();
        assert!(
            manifest.regions.iter().any(|r| r.name == "alpha"),
            "alpha region recovered"
        );
        assert!(
            manifest.regions.iter().any(|r| r.name == "beta"),
            "beta region recovered"
        );
    }
```

**Note on the `Document::keyed` / `flow!` macros and `Theme::reader()`:** these names are used by the existing `gesture_action_decodes_strike_end_to_end` test (`crates/inkapp-core/tests/gesture_action.rs`). Mirror that test's imports. If `Stack` is not currently importable from `crate::components::stack::Stack` inside `flow![...]`, the `flow!` macro may need its child types wrapped — match exactly what the existing test does.

- [x] **Step 3: Run.**

```bash
nix develop -c cargo fmt -p inkapp-core
nix develop -c cargo test -p inkapp-core stack
```

All Stack tests pass (existing 2 + new 4 = 6).

- [x] **Step 4: Update this plan file** — flip Task 1's checkboxes to `[x]`.

**DO NOT git add / git commit.**

---

## Task 2 — inkctl `page describe` lens parity

- [x] **Goal:** Drive a fresh smoke document via `inkctl document publish`, then assert that `inkctl page describe` returns the same region set (names + rects) that `recover_regions(compile_document_in(...))` produces in-library for the same component tree.

**Files:**
- Create: `crates/inkctl/tests/lens_parity_layer4.rs`

**Acceptance:**
- Test `layer4_page_describe_matches_recovered_regions` passes.
- It publishes the "smoke" app (whatever component(s) it uses today; the test inspects the live result, doesn't hard-code names).
- It asserts: for every region in `inkctl page describe data.regions`, there is a matching region (same name, same rect within 0.01pt) in the library-side `recover_regions` output for the same compiled doc. And vice versa — set equality.
- Existing `inkctl` tests still pass.

**Verify:** `nix develop -c cargo test -p inkctl layer4_page_describe_matches_recovered_regions`

**Steps:**

- [x] **Step 1: Discover what the smoke app actually publishes.** Run:

```bash
grep -rn "\"smoke\"" /home/dan/git/inkapp/.worktrees/layer-4/crates/inkctl/src/ /home/dan/git/inkapp/.worktrees/layer-4/crates/inkapp-harness/src/ 2>/dev/null
```

Find where the smoke app's `Document` is constructed in the harness/CLI. The library-side path in the test must construct **the same** `Document` — same components, same content, same `PageGeom`. If the smoke app lives in an `apps::*` module of the harness or inkctl, re-construct it in-test by calling the same builder.

If the smoke app is too tangled to reconstruct, pivot: define the test's own minimal `Document<()>` with one or two `GestureAction`s, publish it via the harness `Session::document_publish` directly (bypass the CLI's app-registry shortcut), then compare. This keeps the test self-contained.

- [x] **Step 2: Create `crates/inkctl/tests/lens_parity_layer4.rs`.** The sketch below assumes you go the "minimal in-test Document" route (Step 1 pivot). Adjust to whichever route Step 1 chose:

```rust
//! Layer-4 lens parity: `inkctl page describe` must return the same region
//! set that the library's `recover_regions` produces on the compiled doc.

use std::process::Command;

fn run_inkctl(inkctl_home: &std::path::Path, args: &[&str]) -> serde_json::Value {
    let out = Command::new(env!("CARGO_BIN_EXE_inkctl"))
        .args(args)
        .env("INKCTL_HOME", inkctl_home)
        .output()
        .expect("spawn inkctl");
    assert!(
        out.status.success(),
        "inkctl {args:?} failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("inkctl stdout JSON")
}

#[test]
fn layer4_page_describe_matches_recovered_regions() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let inkctl_home = tmp.path().to_path_buf();

    // ROUTE A — use the smoke app via the CLI's registry, then re-construct
    // the same Document in-test for the library comparison.
    //
    // (If Step 1 found the smoke app is complex, use ROUTE B below instead.)
    let s = run_inkctl(&inkctl_home, &["session", "new"]);
    let session_id = s["data"]["session_id"]
        .as_str()
        .or_else(|| s["data"]["id"].as_str())
        .unwrap()
        .to_string();
    let d = run_inkctl(
        &inkctl_home,
        &["--session", &session_id, "device", "new", "--backend", "fake"],
    );
    let device_id = d["data"]["device_id"]
        .as_str()
        .or_else(|| d["data"]["id"].as_str())
        .unwrap()
        .to_string();
    let doc = run_inkctl(
        &inkctl_home,
        &[
            "--session", &session_id, "--device", &device_id,
            "document", "publish", &device_id, "smoke",
        ],
    );
    let doc_id = doc["data"]["doc_id"].as_str().unwrap().to_string();

    let described = run_inkctl(
        &inkctl_home,
        &["--session", &session_id, "page", "describe", &doc_id, "0"],
    );
    let cli_regions = described["data"]["regions"].as_array().expect("regions");

    // Library side: re-build the same smoke document and recover_regions on
    // it. Replace this block with the actual smoke-doc builder discovered in
    // Step 1.
    use inkapp_core::document::Document;
    use inkapp_core::geometry::PageGeom;
    use inkapp_core::manifest::recover_regions;
    use inkapp_core::runtime::compile_document_in;
    use inkapp_core::Theme;

    let lib_doc = build_same_smoke_document_in_test();
    let compiled = compile_document_in(&lib_doc, PageGeom::default(), &Theme::reader()).unwrap();
    let lib_manifest = recover_regions(&compiled).unwrap();

    // Filter library regions to page 0 to match `page describe 0`.
    let lib_page0: Vec<&inkapp_core::manifest::Region> =
        lib_manifest.regions.iter().filter(|r| r.page == 0).collect();

    // Same count.
    assert_eq!(
        cli_regions.len(),
        lib_page0.len(),
        "region count mismatch CLI={} lib={}",
        cli_regions.len(),
        lib_page0.len()
    );

    // Same set (by name + rect within 0.01).
    for c in cli_regions {
        let name = c["name"].as_str().unwrap();
        let l = lib_page0
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("CLI region '{name}' has no library counterpart"));
        // Adjust the rect-field indexing to match inkctl's actual JSON shape
        // (Layer 3 found it was a `[x0, y0, x1, y1]` array).
        let arr = c["rect"].as_array().expect("rect as array");
        let cx0 = arr[0].as_f64().unwrap();
        let cy0 = arr[1].as_f64().unwrap();
        let cx1 = arr[2].as_f64().unwrap();
        let cy1 = arr[3].as_f64().unwrap();
        let tol = 0.01;
        assert!((cx0 - l.rect.x0).abs() < tol, "{name} x0 {cx0} vs {}", l.rect.x0);
        assert!((cy0 - l.rect.y0).abs() < tol, "{name} y0 {cy0} vs {}", l.rect.y0);
        assert!((cx1 - l.rect.x1).abs() < tol, "{name} x1 {cx1} vs {}", l.rect.x1);
        assert!((cy1 - l.rect.y1).abs() < tol, "{name} y1 {cy1} vs {}", l.rect.y1);
    }
}

/// Re-construct the same document inkctl's smoke registry publishes. Fill
/// this in based on Step 1's findings; this stub will not compile until you
/// provide the real builder.
fn build_same_smoke_document_in_test() -> inkapp_core::document::Document<()> {
    todo!("see Step 1: this must mirror the smoke app's Document exactly");
}
```

**Important caveats baked into the sketch:**
- The `data.session_id` / `data.id` and `data.device_id` / `data.id` aliasing was needed in Layer 3 — keep both fallback paths.
- The rect JSON shape `[x0, y0, x1, y1]` was confirmed in Layer 3 — assume the same.
- `document publish` argument order in Layer 3 was `<device_id> <app_name>` positional (not `--app`); use that.

- [x] **Step 3: Run the test.** If it FAILS because the CLI and library disagree, that is a real divergence — fix it at the source. The most likely places to find a divergence: inkctl's `page describe` reads the persisted manifest (from `session.state_dir().join("docs").join(doc_id).join("manifest.json")`), so a difference would mean the manifest written at publish time isn't the same one `recover_regions` produced in-process. That's a different class of bug than Layer 3's; investigate before patching.

- [x] **Step 4: Run broader suites.**

```bash
nix develop -c cargo test -p inkapp-harness
nix develop -c cargo test -p inkctl
nix develop -c cargo fmt -p inkapp-harness -p inkctl
```

All green.

- [x] **Step 5: Update this plan** — flip Task 2 checkboxes to `[x]`.

**DO NOT git add / git commit.**

---

## Task 3 — appdx update + workspace verify + commit

- [x] **Goal:** Extend `docs/appdx.md`'s "Test coverage by layer" subsection with Layer-4 status; run workspace-wide verify; commit everything in one shot.

**Files:**
- Modify: `docs/appdx.md`
- Modify: this plan file (flip Task 3 checkboxes)

**Verify:** all three:
- `nix develop -c cargo fmt --check`
- `nix develop -c cargo test --workspace`
- `nix develop -c cargo clippy --all-targets -- -D warnings`

**Steps:**

- [x] **Step 1: Append to `docs/appdx.md`'s "Test coverage by layer" subsection** (after Layer 3's bullet):

```markdown
- **Layer 4 — components in isolation (partial).** Covered 2026-05-27.
  - `GestureAction` already pinned by 8 tests in
    `crates/inkapp-core/tests/gesture_action.rs` (wide pen strike fires,
    tap doesn't, narrow doesn't, highlighter doesn't, empty doesn't,
    presence-only constructor, render declares region + content,
    end-to-end render→recover→attribute→decode).
  - `HighlightText` already pinned by 6 tests in
    `crates/inkapp-core/tests/highlight_text.rs` (token-per-region,
    multi-token swipe, real-attribution discrimination, non-highlighter
    ignored, markup-char safety, `highlighted_token_indices` correctness).
  - `Index` already pinned by 13 inline tests in
    `crates/inkapp-core/src/components/index.rs` (compact-row layout,
    masthead, title→link emission, `decode_is_always_empty`).
  - `Stack` extended to 6 tests in `crates/inkapp-core/src/components/stack.rs`:
    decode forwards a child's `Msg`, `typst_sources` aggregates,
    `image_urls` aggregates, two children's regions both recover under
    one Stack-rendered doc (no collisions).
  - `inkctl page describe` returns the same region set as the library's
    `recover_regions` on the same compiled doc
    (`crates/inkctl/tests/lens_parity_layer4.rs::layer4_page_describe_matches_recovered_regions`).
  - **Deferred to a Layer-4 follow-up plan** (current user WIP touches
    these components): `ActionBand`, `NavBand`, `HeadingComponent`,
    `Section`. They get coverage after the WIP lands so the tests target
    the final shape, not a moving target.
```

- [x] **Step 2: Flip Task 3 checkboxes** in this plan file to `[x]`.

- [x] **Step 3: Workspace verify** (run all three). If fmt fails, `cargo fmt` and retry. If clippy fails on Layer-4 code, fix it. If it fails on pre-existing drift, STOP and report.

- [x] **Step 4: Check staged set.** Expected files:

- `M crates/inkapp-core/src/components/stack.rs`   (Task 1)
- `A crates/inkctl/tests/lens_parity_layer4.rs`    (Task 2, new file)
- `M docs/appdx.md`                                 (Task 3)
- `A docs/superpowers/plans/2026-05-27-layer4-components-isolated.md` (new file)

Possibly:
- `M crates/inkctl/Cargo.toml` if Task 2 required new dev-deps (unlikely — Layer 2/3 already added `tempfile` + `serde_json`).
- `M crates/inkapp-harness/src/observe.rs` or similar if Task 2 surfaced a lens divergence requiring a fix.

Anything else is pre-existing drift; do not stage.

- [x] **Step 5: Commit.**

```bash
git add crates/inkapp-core/src/components/stack.rs crates/inkctl/tests/lens_parity_layer4.rs docs/appdx.md docs/superpowers/plans/2026-05-27-layer4-components-isolated.md
# Add any extra Task-2 fix files identified in Step 4.

git commit -m "$(cat <<'EOF'
tests(layer-4): Stack composer + inkctl page describe lens parity

Closes the non-WIP portion of Layer-4 coverage per
docs/superpowers/specs/2026-05-27-reader-thorough-test-design.md (plan:
docs/superpowers/plans/2026-05-27-layer4-components-isolated.md).

- Stack: four new tests — decode forwards child Msg, typst_sources and
  image_urls aggregate across children, two children's regions both
  recover under one Stack-rendered document.
- inkctl lens_parity_layer4: inkctl page describe matches the library's
  recover_regions on the same compiled doc (set equality on name + rect
  within 0.01pt).
- Existing component coverage audited: GestureAction (8 tests),
  HighlightText (6), Index (13) all already meet the spec's Layer-4 bar.
- Deferred to a follow-up: ActionBand, NavBand, Heading, Section
  (currently under user WIP).
- docs/appdx.md: Test coverage by layer subsection extended for Layer 4.
EOF
)"
```

- [x] **Step 6:** `git log -1 --stat`.

## Self-review checklist

- No `TBD` / `TODO` / `todo!()` markers committed (in particular: the `build_same_smoke_document_in_test` `todo!()` stub in Task 2 Step 2 MUST be replaced with the real builder before Task 2 lands).
- Stack test count is 6 (existing 2 + new 4); existing 2 untouched.
- `cargo test --workspace` clean.
- `cargo clippy --all-targets -- -D warnings` clean.
- The commit set matches Task 3 Step 4 exactly.

## Out of scope

- ActionBand, NavBand, Heading, Section components — deferred until user WIP lands.
- Layer 5 (reader composition) — separate plan after this lands.
- Any visual / PNG-diff assertion on `page inspect` rendering — the lens check uses the JSON region list (`page describe`), not the rendered overlay image.
