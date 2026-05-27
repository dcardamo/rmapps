# Layer 4 follow-up — deferred components: implementation plan

> **For agentic workers:** Use superpowers-extended-cc:subagent-driven-development. **Do NOT call `TaskCreate` / `TaskUpdate`** — the plan-file checkboxes are the only tracker. The session-scoped pre-commit hook blocks per-task commits when native tasks are pending, so we use plain checkboxes instead. One combined commit at end of layer; FF-merge to `main`; remove worktree.

**Goal:** Cover the four Layer-4 components that the original Layer-4 plan deferred because they had user WIP at the time: `ActionBand`, `NavBand`, `HeadingComponent` (`heading.rs`), `Section` (`section.rs`). The WIP has now landed (commit `1ab6729`), so the component surface is settled.

**Architecture:** Tests stay in the natural homes already established for each component — `crates/inkapp-core/tests/action_band.rs`, the inline `mod tests` in `crates/inkapp-core/src/components/nav_band.rs`, `crates/inkapp-core/tests/heading_component.rs`, `crates/inkapp-core/tests/section_component.rs`. The inkctl lens parity gets a richer multi-component fixture (`crates/inkapp-harness/src/tests_common.rs` + `crates/inkctl/src/apps.rs` registry entry) and a new parity test in `crates/inkctl/tests/lens_parity_layer4_followup.rs`. Same fix-as-you-go discipline as Layers 3 and 4: any inkctl/harness divergence found in the parity test gets fixed in the same change-set.

**Tech Stack:** Rust, `cargo test`, `inkapp-core::components`, `inkctl`, `inkapp-harness`.

**Spec:** [docs/superpowers/specs/2026-05-27-reader-thorough-test-design.md](../specs/2026-05-27-reader-thorough-test-design.md). Layers 2 + 3 + 4-partial are shipped; this closes Layer 4. Layers 5 + 6 follow in their own plans.

---

## Scope and constraint

**In scope:** `ActionBand`, `NavBand`, `HeadingComponent`, `Section` — fill the gaps that the Layer-4 partial plan called out as deferred. Plus a multi-component inkctl `page describe` lens-parity test, since the existing Layer-4 parity only exercises a single-region smoke doc and the spec demands the lens be honest for the regions these four components produce.

**Out of scope:** any reader-side composition work (that's Layer 5); any new component features (this is a testing pass); refactoring or renaming existing tests.

**Spec inventory mapping for Layer 4 (follow-up rows):**

| Spec inventory item                                                       | Current state                                                                | Gap → task |
|---------------------------------------------------------------------------|------------------------------------------------------------------------------|------------|
| `ActionBand` — region tappable; consistent across pages                   | 7 tests; pen-strike fires `art-1` only; no cross-page assertion; no sub-threshold assertion | Task 1     |
| `NavBand` — Prev/Home/Next; disabled states                               | 4 inline tests; render+escape+typst_sources+empty-decode covered             | Task 2     |
| `HeadingComponent` (`heading.rs`)                                         | 5 tests; render+optional fields+typst-compile+decode-empty covered           | Task 3     |
| `Section` (`section.rs`)                                                  | 3 tests; only Notice body (no Msg-emitting child); no `art-{id}` anchor pin  | Task 4     |
| `inkctl page describe` honest for ActionBand/NavBand/Heading/Section regions | Existing parity test uses a 1-region smoke only                            | Task 5     |

**Implementation context the implementer needs:**
- The Layer-4 plan's `multi_stroke_scribble_fires_action` test already proves the bbox-union threshold uses `STRIKE_WIDTH_RATIO`; reuse the `strike_across` and `make_short_stroke` patterns. Don't reinvent.
- `Section::new(id, body)` body components must impl `Component<Msg = M>`. For Msg-emitting bodies, use `GestureAction::with_msg("title", "...", M::SomeMsg)` (see `crates/inkapp-core/tests/gesture_action.rs` for the exact incantation).
- The `art-{id}` anchor in `Section::render` is emitted as a Typst `<art-{id}>` label on a zero-size `#metadata` element. It does **not** create a `#region(...)`, so it won't show up in `manifest.regions`. Don't try to assert against `manifest.regions` for the anchor — assert against the rendered Typst source string, or against `link` annotations in the compiled doc if practical.
- `nav_band.rs` already has inline tests — extend that `mod tests`, don't create a new file.
- `heading_component.rs` and `section_component.rs` are integration tests under `tests/`. Extend those files; don't create new `layerN_*.rs` files.

---

## Task 1 — `ActionBand` follow-up coverage

- [x] **Goal:** Pin three behaviors the existing tests leave open: (a) a non-highlighter strike on `action-Inbox-art-2` fires the **Inbox** closure with section id `art-2` (proves the second section's regions are also tappable and the per-section dispatch works), (b) cross-page consistency — every section's full action set (`Inbox` + `Archive`) appears on its own page in the recovered manifest, (c) a sub-threshold strike (well below `STRIKE_WIDTH_RATIO` * cell width) on a real cell produces no message.

**Files:**
- Modify: `crates/inkapp-core/tests/action_band.rs` (append new `#[test]`s; leave existing 7 tests untouched).

**Acceptance:**
- New test `pen_strike_on_inbox_art2_fires_the_inbox_closure` passes.
- New test `each_section_has_full_action_set_on_its_own_page` passes; it asserts each section id ∈ {`art-1`, `art-2`} has both `action-Inbox-<id>` and `action-Archive-<id>` regions, and that the two sections live on different pages (`region.page` differs).
- New test `sub_threshold_strike_does_not_fire` passes; uses a single stroke spanning ~20% of cell width (well under 60% threshold).
- Existing 7 tests still pass.

**Verify:** `nix develop -c cargo test -p inkapp-core --test action_band`

**Steps:**

- [x] **Step 1: Append to `crates/inkapp-core/tests/action_band.rs`.** Keep using the existing `band_with_recorder` and `compile_doc_with_band` helpers and the `strike_across` helper at the bottom of that file.

```rust
#[test]
fn pen_strike_on_inbox_art2_fires_the_inbox_closure() {
    // Symmetric to pen_strike_on_archive_art1_fires_the_archive_closure but
    // proves the second section's regions are also tappable and the per-section
    // dispatch carries the right section id through to the closure.
    let (band, _log) = band_with_recorder();
    let (doc, manifest, _) = compile_doc_with_band(band);
    let header = doc.page_header.as_ref().unwrap();

    let target = manifest
        .regions
        .iter()
        .find(|r| r.name == "action-Inbox-art-2")
        .expect("Inbox/art-2 region present in recovered manifest");
    let region_ink = vec![RegionInk {
        region: "action-Inbox-art-2".into(),
        strokes: vec![strike_across(&target.rect)],
    }];
    let msgs = header.decode(&region_ink, &manifest);
    assert_eq!(msgs, vec![TestMsg::Inbox("art-2".into())]);
}

#[test]
fn each_section_has_full_action_set_on_its_own_page() {
    // For both sections, both action cells must be present, and the two
    // sections must paginate to distinct pages — that is the "renders
    // consistently across pages" half of the Layer-4 spec inventory.
    let (band, _log) = band_with_recorder();
    let (_doc, manifest, _) = compile_doc_with_band(band);

    let by_name: std::collections::HashMap<&str, u32> = manifest
        .regions
        .iter()
        .map(|r| (r.name.as_str(), r.page))
        .collect();

    for section in ["art-1", "art-2"] {
        for label in ["Inbox", "Archive"] {
            let name = format!("action-{label}-{section}");
            assert!(
                by_name.contains_key(name.as_str()),
                "missing region {name}; saw: {:?}",
                by_name.keys().collect::<Vec<_>>()
            );
        }
    }

    let page_art1 = by_name["action-Inbox-art-1"];
    let page_art2 = by_name["action-Inbox-art-2"];
    assert_ne!(
        page_art1, page_art2,
        "art-1 and art-2 must land on different pages; both on {page_art1}"
    );
}

#[test]
fn sub_threshold_strike_does_not_fire() {
    // A single stroke covering ~20% of the cell width (well under the 60%
    // STRIKE_WIDTH_RATIO threshold) must NOT fire. Complements
    // multi_stroke_scribble_fires_action which proves the union-of-bboxes
    // path is honored.
    let (band, _log) = band_with_recorder();
    let (doc, manifest, _) = compile_doc_with_band(band);
    let header = doc.page_header.as_ref().unwrap();

    let target = manifest
        .regions
        .iter()
        .find(|r| r.name == "action-Archive-art-1")
        .unwrap();
    let w = target.rect.x1 - target.rect.x0;
    let y_mid = (target.rect.y0 + target.rect.y1) / 2.0;
    let stroke = Stroke {
        points: (0..=5)
            .map(|i| inkapp_core::geometry::PdfPoint {
                x: target.rect.x0 + w * 0.4 + w * 0.2 * (i as f64 / 5.0),
                y: y_mid,
            })
            .collect(),
        highlighter: false,
    };

    let region_ink = vec![RegionInk {
        region: "action-Archive-art-1".into(),
        strokes: vec![stroke],
    }];
    let msgs = header.decode(&region_ink, &manifest);
    assert!(
        msgs.is_empty(),
        "20%-width strike must not fire; got: {msgs:?}"
    );
}
```

- [x] **Step 2: Run.**

```bash
nix develop -c cargo fmt -p inkapp-core
nix develop -c cargo test -p inkapp-core --test action_band
```

All ActionBand tests pass (existing 7 + new 3 = 10).

- [x] **Step 3: Update this plan file** — flip Task 1's checkboxes to `[x]`.

**DO NOT git add / git commit.**

---

## Task 2 — `NavBand` follow-up coverage

- [x] **Goal:** Pin three more contracts on top of the existing 4 inline tests: (a) decode is still a no-op even when fed non-empty ink whose regions match the order ids (proves the "navigation never produces messages" contract isn't accidentally violated when ink happens to be present), (b) render works for first / middle / last positions in a meaningful way — i.e. `NavBand::new(vec![])` still emits a valid `#nav-band((,))` call (edge case: empty order), and (c) typst_sources is deterministic — calling it twice returns equal vectors (the implementation appears stateless; pin it).

**Files:**
- Modify: `crates/inkapp-core/src/components/nav_band.rs` (extend the inline `mod tests`).

**Acceptance:**
- New test `decode_is_noop_even_with_matching_ink` passes — builds a `RegionInk` with `region = "a"` and at least one stroke, fed to `decode`, must return empty.
- New test `empty_order_still_renders_valid_call` passes — `NavBand::<()>::new(vec![])` renders an output containing `#nav-band((, ))` (the trailing-comma single-element guard still applies as the empty-element guard).
- New test `typst_sources_is_deterministic` passes — two consecutive calls to `typst_sources()` return equal `Vec`s.
- Existing 4 tests still pass.

**Verify:** `nix develop -c cargo test -p inkapp-core nav_band`

**Steps:**

- [x] **Step 1: Append the three tests to `crates/inkapp-core/src/components/nav_band.rs`'s `mod tests`.** The imports at the top of that module (`use super::*;`) already cover `NavBand`, `Component`, `RenderCx`, and `Manifest`; you'll need to additionally bring `Stroke`, `RegionInk`, and `PdfPoint` into scope inside the new tests.

```rust
    #[test]
    fn decode_is_noop_even_with_matching_ink() {
        use crate::geometry::PdfPoint;
        use crate::ink::{RegionInk, Stroke};

        let n: NavBand<()> = NavBand::new(vec!["a".into(), "b".into()]);
        let ink = vec![RegionInk {
            region: "a".into(),
            strokes: vec![Stroke {
                points: vec![PdfPoint { x: 0.0, y: 0.0 }, PdfPoint { x: 10.0, y: 0.0 }],
                highlighter: false,
            }],
        }];
        let msgs = n.decode(&ink, &Manifest::default());
        assert!(
            msgs.is_empty(),
            "NavBand::decode must remain a no-op even with ink on its regions; got: {msgs:?}"
        );
    }

    #[test]
    fn empty_order_still_renders_valid_call() {
        let n: NavBand<()> = NavBand::new(vec![]);
        let out = n.render(&mut RenderCx::new(0));
        // Trailing-comma guard preserved even on empty arrays: the call
        // contains `(, )` (i.e. an array literal, never a parenthesized expr).
        assert!(out.contains("#nav-band("), "expected nav-band call: {out}");
        assert!(out.contains(", )"), "expected trailing-comma guard: {out}");
    }

    #[test]
    fn typst_sources_is_deterministic() {
        let n: NavBand<()> = NavBand::new(vec!["a".into()]);
        let a = n.typst_sources();
        let b = n.typst_sources();
        assert_eq!(a, b, "typst_sources must be deterministic across calls");
    }
```

- [x] **Step 2: Run.**

```bash
nix develop -c cargo fmt -p inkapp-core
nix develop -c cargo test -p inkapp-core nav_band
```

All NavBand tests pass (existing 4 + new 3 = 7).

- [x] **Step 3: Update this plan** — flip Task 2 checkboxes to `[x]`.

**DO NOT git add / git commit.**

---

## Task 3 — `HeadingComponent` follow-up coverage

- [x] **Goal:** Close three gaps the existing 5 tests leave open: (a) titles containing Typst-meaningful characters (`"`, `\`, newline) are escaped via `esc_typst_str` so the call still compiles, (b) optional fields when absent do NOT appear as empty named args in the rendered output (no stray `byline: ""`), (c) `typst_sources` returns the single heading.typ entry (pins the dependency contract).

**Files:**
- Modify: `crates/inkapp-core/tests/heading_component.rs` (append).

**Acceptance:**
- New test `title_with_special_chars_escapes_and_compiles` passes; uses a title containing `"` and compiles the rendered output via `compile_to_document_with_sources`.
- New test `absent_optionals_do_not_pollute_output` passes; constructs `Heading::new("just title")` and asserts the rendered call contains neither `byline:` nor `meta:` nor `subtitle:`.
- New test `typst_sources_contract` passes; asserts the returned vec is exactly one entry whose path is `/inkapp/heading.typ`.
- Existing 5 tests still pass.

**Verify:** `nix develop -c cargo test -p inkapp-core --test heading_component`

**Steps:**

- [x] **Step 1: Append to `crates/inkapp-core/tests/heading_component.rs`.**

```rust
#[test]
fn title_with_special_chars_escapes_and_compiles() {
    // A title with a quote and a backslash must survive escaping and still
    // produce a Typst source that compiles end-to-end.
    let title = r#"Say "hello" \ world"#;
    let h = Heading::<()>::new(title);
    let theme = Theme::reader();
    let mut cx = RenderCx::new(0).with_theme(theme.clone());
    let body = h.render(&mut cx);
    // The raw quote and raw backslash must not appear unescaped in the call
    // string (otherwise the Typst parser will choke).
    assert!(
        body.contains(r#"\""#),
        "quote not escaped in heading call: {body}"
    );
    let src = format!(
        "#import \"/inkapp/heading.typ\": *\n#set page(width: 200pt, height: 200pt, margin: 8pt)\n{}\n{body}",
        theme.prelude()
    );
    let sources = vec![
        (
            "/inkapp/region.typ".into(),
            include_str!("../typst/region.typ").into(),
        ),
        (
            "/inkapp/heading.typ".into(),
            include_str!("../typst/heading.typ").into(),
        ),
    ];
    compile_to_document_with_sources(&src, &sources)
        .expect("Heading with escaped specials compiles");
}

#[test]
fn absent_optionals_do_not_pollute_output() {
    // Heading::new("…") with no further builder calls must NOT emit empty
    // named args like `byline: ""` — the implementation appends each named arg
    // only when its Option is Some. Pin that.
    let out = render(&Heading::new("just title"));
    assert!(!out.contains("byline:"), "stray byline: {out}");
    assert!(!out.contains("meta:"), "stray meta: {out}");
    assert!(!out.contains("subtitle:"), "stray subtitle: {out}");
}

#[test]
fn typst_sources_contract() {
    let h = Heading::<()>::new("x");
    let srcs = <Heading<()> as Component>::typst_sources(&h);
    assert_eq!(srcs.len(), 1, "exactly one source registered");
    assert_eq!(srcs[0].0, "/inkapp/heading.typ");
    assert!(!srcs[0].1.is_empty(), "source text must be non-empty");
}
```

- [x] **Step 2: Run.**

```bash
nix develop -c cargo fmt -p inkapp-core
nix develop -c cargo test -p inkapp-core --test heading_component
```

All Heading tests pass (existing 5 + new 3 = 8).

- [x] **Step 3: Update this plan** — flip Task 3 checkboxes to `[x]`.

**DO NOT git add / git commit.**

---

## Task 4 — `Section` follow-up coverage

- [x] **Goal:** Close four gaps the existing 3 tests leave open: (a) `decode` actually forwards a Msg-emitting child's messages (currently only tested with `Notice` which emits nothing — Section's decode could be silently swallowing and we'd never know), (b) `typst_sources` aggregates section.typ **plus** each body component's sources, (c) `image_urls` forwards from body components, (d) the `art-{id}` anchor label appears in the rendered Typst source (pins the Index-link target shape).

**Files:**
- Modify: `crates/inkapp-core/tests/section_component.rs` (append).

**Acceptance:**
- New test `decode_forwards_child_msg` passes; uses a `GestureAction::with_msg` as the Section body, builds a real recovered manifest via the full render pipeline, attributes a region-spanning strike, calls `Section::decode`, asserts the child's Msg is returned.
- New test `typst_sources_aggregates_section_and_body` passes; uses a body containing a `Heading` and asserts the returned vec includes both `/inkapp/section.typ` and `/inkapp/heading.typ`.
- New test `image_urls_forwards_from_body` passes; uses a stub `Mock` body component (defined inline) whose `image_urls` returns a known URL; asserts `Section::image_urls()` contains it.
- New test `render_emits_art_anchor_label` passes; asserts the rendered Typst source contains `<art-art-1>` for `Section::new("art-1", ...)`.
- Existing 3 tests still pass.

**Verify:** `nix develop -c cargo test -p inkapp-core --test section_component`

**Steps:**

- [x] **Step 1: Append to `crates/inkapp-core/tests/section_component.rs`.** The needed extra imports are listed inline in the test code; mirror them at the top of the file alongside the existing imports.

```rust
// Add to the imports at the top of the file:
//   use inkapp_core::components::gesture::GestureAction;
//   use inkapp_core::components::heading::Heading;
//   use inkapp_core::document::Document;
//   use inkapp_core::flow;
//   use inkapp_core::geometry::{PageGeom, PdfPoint};
//   use inkapp_core::ink::Stroke;
//   use inkapp_core::manifest::{recover_regions, Manifest, Region};
//   use inkapp_core::readback::attribute_page;
//   use inkapp_core::runtime::compile_document_in;

#[derive(Debug, Clone, PartialEq, Eq)]
enum SecMsg {
    Hit,
}

#[test]
fn decode_forwards_child_msg() {
    // A Section wrapping a GestureAction must forward the child's Msg when
    // ink hits the child's region. Compiles a real doc through the full
    // pipeline, recovers regions, attributes a region-spanning strike, then
    // calls Section::decode directly.
    let s: Section<SecMsg> = Section::new(
        "art-1",
        vec![Box::new(GestureAction::with_msg("title", "Read me", SecMsg::Hit))],
    );
    let doc: Document<SecMsg> = Document::keyed("d", flow![s]);
    let compiled =
        compile_document_in(&doc, PageGeom::default(), &Theme::reader()).unwrap();
    let manifest = recover_regions(&compiled).unwrap();
    let r: &Region = manifest
        .regions
        .iter()
        .find(|r| r.name == "title")
        .expect("title region recovered through Section");
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
    assert_eq!(decoded, vec![SecMsg::Hit]);
}

#[test]
fn typst_sources_aggregates_section_and_body() {
    let s: Section<()> = Section::new(
        "x",
        vec![Box::new(Heading::<()>::new("hi"))],
    );
    let paths: Vec<String> = s.typst_sources().into_iter().map(|(p, _)| p).collect();
    assert!(
        paths.iter().any(|p| p == "/inkapp/section.typ"),
        "section.typ missing: {paths:?}"
    );
    assert!(
        paths.iter().any(|p| p == "/inkapp/heading.typ"),
        "heading.typ not forwarded: {paths:?}"
    );
}

#[test]
fn image_urls_forwards_from_body() {
    use inkapp_core::component::{Component as _, RenderCx};
    use inkapp_core::ink::RegionInk;

    struct Img;
    impl Component for Img {
        type Msg = ();
        fn render(&self, _cx: &mut RenderCx) -> String {
            String::new()
        }
        fn decode(&self, _ink: &[RegionInk], _m: &Manifest) -> Vec<()> {
            Vec::new()
        }
        fn image_urls(&self) -> Vec<String> {
            vec!["https://example.com/picture.png".into()]
        }
    }

    let s: Section<()> = Section::new("x", vec![Box::new(Img)]);
    let urls = s.image_urls();
    assert_eq!(urls, vec!["https://example.com/picture.png".to_string()]);
}

#[test]
fn render_emits_art_anchor_label() {
    // The `art-{id}` label is what Index entries link to. Pin its presence in
    // the rendered Typst source.
    let s: Section<()> = Section::new("art-1", vec![Box::new(Notice::line("body"))]);
    let mut cx = RenderCx::new(0).with_theme(Theme::reader());
    let out = s.render(&mut cx);
    assert!(out.contains("<art-art-1>"), "art anchor missing: {out}");
}
```

- [x] **Step 2: Run.**

```bash
nix develop -c cargo fmt -p inkapp-core
nix develop -c cargo test -p inkapp-core --test section_component
```

All Section tests pass (existing 3 + new 4 = 7).

- [x] **Step 3: Update this plan** — flip Task 4 checkboxes to `[x]`.

**DO NOT git add / git commit.**

---

## Task 5 — inkctl `page describe` lens parity over a multi-component doc

- [x] **Goal:** The existing Layer-4 lens parity test only exercises the single-region `smoke` doc. Add a richer fixture (`ActionBand` page-header + two `Section`s containing `Heading` + a `GestureAction`) and an inkctl parity test that asserts `inkctl page describe` returns the same region set as the library's in-process compile for **each page** of the multi-page doc (not just page 0). If any CLI/library divergence emerges, fix it at the source.

**Files:**
- Modify: `crates/inkapp-harness/src/tests_common.rs` (add a `multi_component_app(name)` builder).
- Modify: `crates/inkctl/src/apps.rs` (register `"multi"` → `multi_component_app("multi")`).
- Create: `crates/inkctl/tests/lens_parity_layer4_followup.rs`.

**Acceptance:**
- `tests_common::multi_component_app("multi")` returns a `PublishedApp` whose manifest spans **two pages** and contains regions for an `ActionBand` (`action-Inbox-art-1`, `action-Archive-art-1`, etc.) and the `GestureAction` regions inside each Section.
- `inkctl document publish <did> multi` succeeds end-to-end.
- New test `layer4_followup_page_describe_matches_recovered_regions_all_pages` passes; iterates `inkctl page describe <doc_id> <page_idx>` for every page in the manifest and asserts set-equality of region names + rects (within 0.01pt) with the library-side manifest filtered to that same `page` index.
- Existing inkctl tests all still pass.
- If a divergence is found, the fix lands in the same change-set and the test message references the cause.

**Verify:** `nix develop -c cargo test -p inkctl layer4_followup_page_describe_matches_recovered_regions_all_pages`

**Steps:**

- [x] **Step 1: Add the multi-component builder to `crates/inkapp-harness/src/tests_common.rs`.** This builder must use the framework's real `runtime::compile_document_in` (the same path the framework's app loop uses), not a raw Typst source — that way the regions match what `Document`-authoring code actually produces.

```rust
// Add to the imports at the top of tests_common.rs:
//   use inkapp_core::components::action_band::ActionBand;
//   use inkapp_core::components::gesture::GestureAction;
//   use inkapp_core::components::heading::Heading;
//   use inkapp_core::components::section::Section;
//   use inkapp_core::document::Document;
//   use inkapp_core::flow;
//   use inkapp_core::geometry::PageGeom;
//   use inkapp_core::render::document_to_pdf;
//   use inkapp_core::runtime::compile_document_in;
//   use inkapp_core::theme::Theme;

/// Build a multi-component, multi-page `PublishedApp` for Layer-4 lens-parity
/// testing. Two `Section`s, each containing a `Heading` and a `GestureAction`;
/// the page-header is an `ActionBand` with `Inbox` + `Archive` cells. The
/// resulting PDF is at least two pages; the manifest carries action-band
/// regions per section and gesture-action regions per section.
pub fn multi_component_app(name: &str) -> PublishedApp {
    // Boxed-handler ActionBand cells that ignore the section id and return ();
    // the parity test only inspects the manifest, not the decoded messages.
    let cells: Vec<(String, Box<dyn Fn(&str) -> () + Send + Sync>)> = vec![
        ("Inbox".into(), Box::new(|_| ())),
        ("Archive".into(), Box::new(|_| ())),
    ];
    let band: ActionBand<()> = ActionBand::new(cells);

    let s1: Section<()> = Section::new(
        "art-1",
        vec![
            Box::new(Heading::<()>::new("First article")),
            Box::new(GestureAction::with_msg("title-1", "tap me", ())),
        ],
    );
    let s2: Section<()> = Section::new(
        "art-2",
        vec![
            Box::new(Heading::<()>::new("Second article")),
            Box::new(GestureAction::with_msg("title-2", "tap me too", ())),
        ],
    );

    let doc: Document<()> = Document::keyed("multi", flow![s1, s2]).page_header(band);
    let geom = PageGeom {
        w: 240.0,
        h: 140.0,
        margin: 6.0,
    };
    let compiled = compile_document_in(&doc, geom, &Theme::reader()).expect("compile multi doc");
    let pdf_bytes = document_to_pdf(&compiled).expect("render pdf");
    let manifest = recover_regions(&compiled)
        .expect("recover manifest")
        .with_version(1);
    PublishedApp {
        app_name: name.to_string(),
        pdf_bytes,
        manifest,
        source_typ: None,
    }
}
```

If `compile_document_in` is not currently re-exported from `inkapp_core::runtime`, find its actual path (check `crates/inkapp-core/src/lib.rs` re-exports and `crates/inkapp-core/src/runtime.rs`) and adjust. If the page geometry above produces only one page, bump `h` down or pad the heading content until two pages are guaranteed.

- [x] **Step 2: Register the new app in `crates/inkctl/src/apps.rs`.**

```rust
//! Registry of in-tree harness fixtures publishable from `inkctl document publish`.

use inkapp_harness::session::PublishedApp;
use inkapp_harness::tests_common;

pub fn build(app_name: &str) -> Result<PublishedApp, String> {
    match app_name {
        "smoke" => Ok(tests_common::single_region_app("smoke")),
        "uri-link" => Ok(tests_common::app_with_uri_link(
            "uri-link",
            "r1",
            "https://example.org",
        )),
        "multi" => Ok(tests_common::multi_component_app("multi")),
        other => Err(format!("unknown_app: {other}")),
    }
}
```

- [x] **Step 3: Create `crates/inkctl/tests/lens_parity_layer4_followup.rs`.** Model on the existing `lens_parity_layer4.rs`; loop across pages instead of asserting page 0 only.

```rust
//! Layer-4 follow-up lens parity: `inkctl page describe` must agree with the
//! library's recovered manifest on EVERY page of a multi-component doc whose
//! regions come from `ActionBand`, `Section`, `Heading`, and `GestureAction`.

use assert_cmd::Command;
use serde_json::Value;

fn run(home: &std::path::Path, sess: Option<&str>, args: &[&str]) -> Value {
    let mut cmd = Command::cargo_bin("inkctl").unwrap();
    cmd.env("INKCTL_HOME", home);
    if let Some(s) = sess {
        cmd.env("INKCTL_SESSION", s);
    }
    let out = cmd.args(args).output().unwrap();
    serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
        panic!(
            "not JSON from inkctl {args:?}:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

#[test]
fn layer4_followup_page_describe_matches_recovered_regions_all_pages() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();

    let session = run(home, None, &["session", "new"]);
    let sid = session["data"]["session_id"].as_str().unwrap().to_string();

    let device = run(home, Some(&sid), &["device", "new"]);
    let did = device["data"]["device_id"].as_str().unwrap().to_string();

    let doc = run(home, Some(&sid), &["document", "publish", &did, "multi"]);
    assert_eq!(doc["ok"], true, "publish multi failed: {doc}");
    let doc_id = doc["data"]["doc_id"].as_str().unwrap().to_string();

    // Library side: the same builder the CLI registry calls. Note: `published`
    // contains the canonical manifest the CLI side wrote to disk at publish time.
    let published = inkapp_harness::tests_common::multi_component_app("multi");
    let pages: std::collections::BTreeSet<u32> =
        published.manifest.regions.iter().map(|r| r.page).collect();
    assert!(
        pages.len() >= 2,
        "expected multi-page fixture; got pages: {pages:?}"
    );

    for page in &pages {
        let described = run(
            home,
            Some(&sid),
            &["page", "describe", &doc_id, &page.to_string()],
        );
        assert_eq!(
            described["ok"], true,
            "page describe failed for page {page}: {described}"
        );
        let cli_regions = described["data"]["regions"].as_array().unwrap_or_else(|| {
            panic!("data.regions not array for page {page}: {described}")
        });
        let lib_page: Vec<&inkapp_core::manifest::Region> = published
            .manifest
            .regions
            .iter()
            .filter(|r| r.page == *page)
            .collect();

        assert_eq!(
            cli_regions.len(),
            lib_page.len(),
            "page {page} region count mismatch — CLI={} lib={};\n  CLI names: {:?}\n  lib names: {:?}",
            cli_regions.len(),
            lib_page.len(),
            cli_regions.iter().map(|r| r["name"].as_str()).collect::<Vec<_>>(),
            lib_page.iter().map(|r| &r.name).collect::<Vec<_>>(),
        );

        for c in cli_regions {
            let name = c["name"].as_str().unwrap();
            let l = lib_page
                .iter()
                .find(|r| r.name == name)
                .unwrap_or_else(|| {
                    panic!(
                        "page {page}: CLI region '{name}' has no library counterpart;\n  lib names: {:?}",
                        lib_page.iter().map(|r| &r.name).collect::<Vec<_>>()
                    )
                });

            let arr = c["rect"].as_array().expect("rect must be 4-elem array");
            let cx0 = arr[0].as_f64().unwrap();
            let cy0 = arr[1].as_f64().unwrap();
            let cx1 = arr[2].as_f64().unwrap();
            let cy1 = arr[3].as_f64().unwrap();
            let tol = 0.01;
            assert!((cx0 - l.rect.x0).abs() < tol, "{name} x0");
            assert!((cy0 - l.rect.y0).abs() < tol, "{name} y0");
            assert!((cx1 - l.rect.x1).abs() < tol, "{name} x1");
            assert!((cy1 - l.rect.y1).abs() < tol, "{name} y1");
        }

        // Reverse direction.
        for l in &lib_page {
            let name = &l.name;
            assert!(
                cli_regions.iter().any(|c| c["name"].as_str() == Some(name)),
                "page {page}: lib region '{name}' has no CLI counterpart;\n  CLI names: {:?}",
                cli_regions.iter().map(|c| c["name"].as_str()).collect::<Vec<_>>(),
            );
        }
    }
}
```

- [x] **Step 4: Run.**

```bash
nix develop -c cargo fmt -p inkapp-harness -p inkctl
nix develop -c cargo test -p inkctl layer4_followup_page_describe_matches_recovered_regions_all_pages
nix develop -c cargo test -p inkapp-harness
nix develop -c cargo test -p inkctl
```

All green. If `cargo test -p inkctl` exposes a divergence (CLI says one set of regions, library says another), investigate before patching — likely culprits: (a) `multi_component_app`'s in-process manifest is being compared against a different doc than what `inkctl document publish multi` actually persisted (e.g. nondeterministic ordering, or `Document::keyed("multi", …)` building a different doc when called twice), (b) inkctl's `page describe` reads a stale manifest, (c) a rect-rounding difference on multi-page boundaries. Fix at the source, not in the test tolerance.

- [x] **Step 5: Update this plan** — flip Task 5 checkboxes to `[x]`.

**DO NOT git add / git commit.**

---

## Task 6 — appdx update + workspace verify + one combined commit + merge

- [x] **Goal:** Extend `docs/appdx.md`'s "Test coverage by layer" Layer-4 subsection with the follow-up coverage, run workspace-wide verify, commit everything in one shot, FF-merge to `main`, remove the worktree.

**Files:**
- Modify: `docs/appdx.md`
- Modify: this plan file (flip Task 6 checkboxes)

**Verify (all three before commit):**
- `nix develop -c cargo fmt --check`
- `nix develop -c cargo test --workspace`
- `nix develop -c cargo clippy --all-targets -- -D warnings`

**Steps:**

- [x] **Step 1: Edit the Layer-4 subsection in `docs/appdx.md`.** Find the bullet line `- **Layer 4 — components in isolation (partial).** Covered 2026-05-27.` and replace the trailing "Deferred to a Layer-4 follow-up plan…" sub-bullet with the new follow-up coverage. The replacement bullet block (paste verbatim, preserving 2-space indent of sub-bullets):

```markdown
- **Layer 4 — components in isolation.** Covered 2026-05-27.
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
  - `ActionBand` extended to 10 tests in
    `crates/inkapp-core/tests/action_band.rs`: pen-strike on art-2
    fires Inbox closure, each section has full action set on its own
    page (cross-page consistency), sub-threshold strike does not fire.
  - `NavBand` extended to 7 inline tests in
    `crates/inkapp-core/src/components/nav_band.rs`: decode is a no-op
    even with matching ink, empty order still renders a valid call,
    `typst_sources` is deterministic.
  - `HeadingComponent` extended to 8 tests in
    `crates/inkapp-core/tests/heading_component.rs`: titles with quotes
    and backslashes escape and compile, absent optional fields do not
    pollute output, `typst_sources` contract pinned.
  - `Section` extended to 7 tests in
    `crates/inkapp-core/tests/section_component.rs`: decode forwards a
    Msg-emitting child's messages (via `GestureAction`),
    `typst_sources` aggregates section.typ + body deps,
    `image_urls` forwards from body, `<art-{id}>` anchor label present
    in render output.
  - `inkctl page describe` returns the same region set as the library
    manifest persisted at publish time, for two fixtures: a single-
    region smoke
    (`crates/inkctl/tests/lens_parity_layer4.rs::layer4_page_describe_matches_recovered_regions`)
    and a multi-page, multi-component fixture spanning ActionBand,
    Section, Heading, and GestureAction regions
    (`crates/inkctl/tests/lens_parity_layer4_followup.rs::layer4_followup_page_describe_matches_recovered_regions_all_pages`).
    No divergence found — the publish→disk→describe round-trip is
    lossless within 0.01pt on every page.
```

- [x] **Step 2: Flip Task 6 checkboxes** in this plan file to `[x]`.

- [x] **Step 3: Workspace verify** — run all three:

```bash
nix develop -c cargo fmt --check
nix develop -c cargo test --workspace
nix develop -c cargo clippy --all-targets -- -D warnings
```

If fmt fails, run `cargo fmt` then re-check. If clippy fails on follow-up code, fix it. If clippy fails on pre-existing drift unrelated to Layer-4-followup, STOP and report — do not paper over.

- [x] **Step 4: Check the staged set.** Expected files:

- `M crates/inkapp-core/tests/action_band.rs`            (Task 1)
- `M crates/inkapp-core/src/components/nav_band.rs`      (Task 2)
- `M crates/inkapp-core/tests/heading_component.rs`      (Task 3)
- `M crates/inkapp-core/tests/section_component.rs`      (Task 4)
- `M crates/inkapp-harness/src/tests_common.rs`          (Task 5)
- `M crates/inkctl/src/apps.rs`                          (Task 5)
- `A crates/inkctl/tests/lens_parity_layer4_followup.rs` (Task 5)
- `M docs/appdx.md`                                       (Task 6)
- `A docs/superpowers/plans/2026-05-27-layer4-followup-deferred-components.md` (this plan, new file)

Possibly:
- `M crates/inkapp-harness/Cargo.toml` if Task 5 needed any new dep (unlikely — `recover_regions` / `document_to_pdf` / `compile_document_in` / `Theme` should all be reachable through existing deps; if not, prefer adding the path through `inkapp_core` re-exports instead of new deps).
- Any inkctl/harness fix-file if Task 5 surfaced a real divergence.

Anything else is pre-existing drift; do not stage.

- [x] **Step 5: Commit.**

```bash
git add \
  crates/inkapp-core/tests/action_band.rs \
  crates/inkapp-core/src/components/nav_band.rs \
  crates/inkapp-core/tests/heading_component.rs \
  crates/inkapp-core/tests/section_component.rs \
  crates/inkapp-harness/src/tests_common.rs \
  crates/inkctl/src/apps.rs \
  crates/inkctl/tests/lens_parity_layer4_followup.rs \
  docs/appdx.md \
  docs/superpowers/plans/2026-05-27-layer4-followup-deferred-components.md
# Add any extra fix files identified in Step 4.

git commit -m "$(cat <<'EOF'
tests(layer-4): ActionBand, NavBand, Heading, Section + multi-component lens parity

Closes the deferred Layer-4 components per
docs/superpowers/specs/2026-05-27-reader-thorough-test-design.md (plan:
docs/superpowers/plans/2026-05-27-layer4-followup-deferred-components.md).

- ActionBand: three new tests — art-2 pen-strike fires Inbox closure;
  each section has full action set on its own page (cross-page
  consistency); sub-threshold strike does not fire.
- NavBand: three new inline tests — decode is a no-op even with
  matching ink; empty order still renders a valid call;
  typst_sources is deterministic.
- HeadingComponent: three new tests — titles with quotes/backslashes
  escape and compile; absent optionals do not pollute output;
  typst_sources contract pinned (single /inkapp/heading.typ entry).
- Section: four new tests — decode forwards a Msg-emitting child's
  messages (via GestureAction); typst_sources aggregates section.typ
  plus body deps; image_urls forwards from body; <art-{id}> anchor
  label present in render output.
- inkctl lens_parity_layer4_followup: a multi-page, multi-component
  fixture (ActionBand + 2x Section/Heading/GestureAction) registered
  as the "multi" smoke app, then inkctl page describe is matched
  against the library's recover_regions on every page. Set-equality
  on name + rect within 0.01pt across all pages.
- docs/appdx.md: Layer-4 subsection promoted from "(partial)" to
  fully covered, with per-component coverage counts updated.
EOF
)"
```

- [x] **Step 6: FF-merge to main and clean up.**

```bash
# From the worktree:
git log -1 --stat

# Switch to main and FF-merge:
cd /home/dan/git/inkapp
git checkout main
git merge --ff-only layer-4-followup
git log -1 --stat

# Remove the worktree and branch:
git worktree remove .worktrees/layer-4-followup
git branch -d layer-4-followup
```

If `git merge --ff-only` rejects, main has moved during the work — rebase the layer branch onto main, re-verify (fmt/test/clippy), then re-attempt the FF-merge. Never force-push or non-FF merge.

## Self-review checklist

- No `TBD` / `TODO` / `todo!()` markers committed.
- ActionBand test count is 10 (existing 7 + new 3); NavBand inline 7 (4 + 3); Heading 8 (5 + 3); Section 7 (3 + 4). Existing tests untouched.
- inkctl `multi` app registry entry present; `multi_component_app` builder exists.
- `cargo test --workspace` clean.
- `cargo clippy --all-targets -- -D warnings` clean.
- `cargo fmt --check` clean.
- The commit set matches Task 6 Step 4 exactly.
- FF-merge to main succeeded; worktree and branch removed.

## Out of scope

- Layer 5 (reader composition, `apps/reader/src/lib.rs` update/view exhaustive coverage) — separate plan.
- Layer 6 (full agent-driven loop sequences) — separate plan, depends on Layer 5.
- Any visual / PNG-diff assertion — region set + rect parity is the lens contract; rendered pixels are out of scope here.
- Reader app feature work — this is a testing pass; any reader bug found gets filed, not fixed here.
