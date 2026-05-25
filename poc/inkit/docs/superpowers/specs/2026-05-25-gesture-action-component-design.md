# GestureAction component — design

**Date:** 2026-05-25
**Status:** Approved (design)

## Problem

inkapp has Control components that fire on *presence* of ink (`Checkbox`,
`Passage`) and a Capture component that reads *highlighter* swipes
(`HighlightableText`). It has no component that fires on a **specific pen
gesture** — e.g. striking through an article title to archive it. The old
`rmreader` did this in `src/readback/classify.rs`: it classified a stroke as an
*action* (vs. a content highlight) by geometry — a stroke spanning a label
region's width. We want that intent ported as a clean, reusable, device-agnostic
component.

The point of the component is **gesture specificity**: a deliberate strike fires
it; incidental marks, taps, and highlighter swipes must not. A control that fired
on any ink would be `Passage`; the value here is that it discriminates.

## Component: `GestureAction<M>`

A fixed-affordance **Control** bound to one named region. It carries a
value-message (`on_gesture: M`, Elm-style — no stored closure), exactly like
`Checkbox`/`Passage`, so it drops into any `view` flow. Like `Checkbox` it is a
fixed-affordance control and carries **no `Mode`**.

**File:** `crates/inkapp-core/src/components/gesture.rs`
**Registration:** one line `pub mod gesture;` in `crates/inkapp-core/src/components/mod.rs`.

### Surface

```rust
pub struct GestureAction<M = ()> {
    name: String,
    content: String,
    on_gesture: M,
}

impl GestureAction<()> {
    pub fn new(name: &str, content: &str) -> Self;          // presence-only / no message
}

impl<M> GestureAction<M> {
    pub fn with_msg(name: &str, content: &str, on_gesture: M) -> Self;
    pub fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> bool; // gesture detected?
}

impl<M: Clone> Component for GestureAction<M> {
    type Msg = M;
    fn render(&self, _cx: &mut RenderCx) -> String;
    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<M>;
}
```

This mirrors `Checkbox` (`new` for `M=()`, `with_msg` for carrying a message) so
apps read consistently.

### Render

Emits the target content as a single non-breakable region (one rect), asserting a
valid region name like the other components. Inline Typst (no authored `.typ`,
like `Passage`):

```text
#region("<name>", [#"<escaped content>"])
```

The default (`breakable: false`) `#region` wraps the body in a `box`, so recovery
yields exactly one rect whose width is the laid-out content width — the
denominator the strike must span.

### Decode (detection)

Ported `classify.rs` intent, reduced to one geometric test:

1. `find` the region by name in the manifest (Checkbox's pattern — a single,
   non-breakable region, so first-frame `find` is correct; not the stitched
   `Passage` path).
2. Collect strokes attributed to this region (`ri.region == name`) that are
   **non-highlighter** (`!s.highlighter`) and have at least one point inside the
   region rect (`rect.contains`) — the same two-stage filter `Checkbox` uses.
3. Compute the **combined bounding box** of those strokes (union over all their
   points — robust to a multi-stroke scribble).
4. Fire iff `combined_bbox_width >= STRIKE_WIDTH_RATIO * region_width`, where
   `STRIKE_WIDTH_RATIO = 0.6` ("spans most of the region width").

`decode` returns `vec![on_gesture.clone()]` when `read` is true, else `vec![]`.

Why width-span only (no flatness/aspect guard): it is the minimal port of the
`classify.rs` intent and cleanly separates every captured fixture (see table).
A flatness guard risks a false no-fire on a tall scribble and needs tuning for no
benefit here.

### Why this discriminates

| Input | Tool | Geometry | Result | Reason |
|------------------|-------------|---------------------|----------|----------------------------------------|
| strike-through | pen | stretch-x (full w) | **fire** | spans region width |
| scribble-out | pen | stretch-x (full w) | **fire** | spans region width |
| highlight-swipe | highlighter | stretch (full w) | no-fire | highlighter strokes filtered out |
| checkmark | pen | aspect-fit (narrow) | no-fire | pen but narrow → fails width test |
| tap | pen | single point | no-fire | zero-width bbox |
| (empty) | — | — | no-fire | no strokes |

The `checkmark` case is load-bearing: it is a *pen* stroke that still does not
fire, proving specificity is geometry, not merely the tool.

## Simulator

**No change required.** `crates/inkapp-harness/src/simulator.rs` already has a
`Gesture::Fixture(&'static str)` variant that loads
`tests/fixtures/gestures/<name>.json`, transplants the default sample into the
target region per the fixture's `fit`, and sets the `highlighter` flag from the
fixture's `tool`. That is exactly the fixture-replay path this component's
exerciser needs.

## Tests (TDD)

### Core unit tests — `crates/inkapp-core/tests/gesture_action.rs`

No harness dependency (core cannot depend on harness). Strokes synthesized
directly:

1. **fire** — a wide non-highlighter stroke spanning the region width decodes to
   one message.
2. **no-fire on tap** — a single-point pen stroke decodes to nothing.
3. **no-fire on wrong tool** — a wide *highlighter* stroke (same geometry as the
   firing case) decodes to nothing.
4. **render** — `render` output contains `#region("<name>"` and the content.
5. **end-to-end** — render → `recover_regions` → `attribute_page` → `decode`
   (mirrors `tests/passage.rs::passage_decodes_ink_end_to_end`): synthesize a
   pen stroke spanning the recovered region's width, assert one message.

### Harness exerciser — `crates/inkapp-harness/tests/exercisers.rs`

A `gesture_action_exerciser` test driving real captured fixtures through
`simulate` + `Gesture::Fixture`, round-tripping through the device write/read
byte path:

- `strike-through` → fires.
- `scribble-out` → fires.
- empty `Scenario` → does not fire.
- `highlight-swipe` → does not fire (wrong tool).
- `checkmark` → does not fire (pen, wrong shape).

## Out of scope / non-goals

- No `Mode` axis (fixed-affordance control).
- No authored `.typ` render half (inline, like `Passage`).
- No multi-region / breakable variant (a title is one rect).
- No flatness/aspect-ratio detection guard.

## Conventions / done

- `nix develop -c cargo test --workspace` green.
- Do **not** stage `Cargo.lock`.
- Clear native tasks before committing (commit hook blocks on open tasks).
- **Definition of done:** mark `GestureAction` built in `docs/appdx.md` as the
  final step (every build-order item reconciles appdx).
