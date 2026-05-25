# Widget/Component Consolidation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the obsolete `Widget` trait so `Component` is the sole view abstraction, keeping the typed `read` it provided as plain inherent methods.

**Architecture:** Pure subtraction refactor in three green-at-each-commit steps. (1) Relocate `Widget`'s innocent housemates (`RenderCx`, `region_metadata`, `is_valid_region_name`) to permanent homes and repoint every import, leaving `widget.rs` holding only the trait. (2) Delete the trait and its module, collapsing the three doubled component impls to a single `render` + an inherent `read`, and fixing the few trait-form callers. (3) Update `appdx.md`. No behavior changes; the existing workspace test suite is the regression net and must stay green at every commit with no assertion weakened.

**Tech Stack:** Rust workspace (`inkapp-core`, `inkapp-harness`, `apps/reading-queue`), `cargo test`.

---

### Task 1: Relocate `RenderCx` and region helpers; leave `widget.rs` holding only the trait

**Goal:** Move the three non-trait items out of `widget.rs` to permanent homes and repoint all importers, so a later commit can delete the module by deleting just the `Widget` trait. Build stays green; `Widget` and all its impls keep working.

**Files:**
- Modify: `crates/inkapp-core/src/component.rs` (add `RenderCx` + `RenderCx` tests-none; it's the render context for `Component::render`)
- Modify: `crates/inkapp-core/src/render.rs` (add `region_metadata`, `is_valid_region_name`, and their two unit tests)
- Modify: `crates/inkapp-core/src/widget.rs` (remove the three moved items; keep only the `Widget` trait, now importing `RenderCx` from `component`)
- Modify: `crates/inkapp-core/src/lib.rs` (re-export `region_metadata` at crate root)
- Modify: `crates/inkapp-core/src/components/{calendar_view.rs,notice.rs,stepper.rs,checkbox.rs,highlight_text.rs}` (repoint `crate::widget::*` imports)
- Modify: `crates/inkapp-core/src/runtime.rs` (repoint `RenderCx` import)
- Modify: `apps/reading-queue/src/lib.rs` (repoint `RenderCx` import; keep `Widget` import — still used here until Task 2)
- Modify: `crates/inkapp-harness/src/recording.rs`, `crates/inkapp-harness/tests/simulator.rs` (repoint `region_metadata` to crate root)

**Acceptance Criteria:**
- [ ] `RenderCx` (with `new`, `fresh_id`, `page`, `next_id`) is defined in `component.rs`; `region_metadata` + `is_valid_region_name` + their two tests are in `render.rs`.
- [ ] `widget.rs` contains only the `Widget` trait and `use crate::component::RenderCx;`.
- [ ] `region_metadata` is re-exported from the crate root (`inkapp_core::region_metadata`).
- [ ] No file imports `RenderCx`, `region_metadata`, or `is_valid_region_name` from `crate::widget` / `inkapp_core::widget` anymore.
- [ ] `Widget` is still defined and still implemented by `Checkbox`, `Stepper`, `HighlightableText` (unchanged this task).

**Verify:** `cargo test --workspace` → all pass; `cargo build --workspace` → clean (no unused-import warnings).

**Steps:**

- [ ] **Step 1: Move `RenderCx` into `component.rs`**

In `crates/inkapp-core/src/component.rs`, the current head is:

```rust
use crate::ink::RegionInk;
use crate::manifest::Manifest;
use crate::widget::RenderCx;
```

Replace the `use crate::widget::RenderCx;` line with the `RenderCx` definition itself (moved verbatim from `widget.rs`), placed after the `use` lines:

```rust
use crate::ink::RegionInk;
use crate::manifest::Manifest;

/// Render-time context: supplies the current page index and a monotonically
/// increasing id so components can mint unique region names if needed.
#[derive(Debug, Default)]
pub struct RenderCx {
    pub page: usize,
    next_id: u64,
}

impl RenderCx {
    pub fn new(page: usize) -> Self {
        Self { page, next_id: 0 }
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

- [ ] **Step 2: Move the region helpers + their tests into `render.rs`**

Append to `crates/inkapp-core/src/render.rs` (moved verbatim from `widget.rs`, comments included):

```rust
/// Whether a region name is safe to interpolate into Typst markup.
///
/// Region names are embedded into a Typst string literal by [`region_metadata`];
/// a name containing `"`, `)`, `]`, or other markup characters would silently
/// break compilation of the whole document. We constrain names to an identifier
/// alphabet (plus `:` for namespacing, e.g. `box:checkmark:0`) so that failure
/// mode cannot occur. Component authors mint names from this alphabet.
pub fn is_valid_region_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ':')
}

/// Emit the `#place`d metadata markup that [`crate::manifest::recover_regions`]
/// reads back. Coordinates are Typst-space (top-left origin) points.
///
/// # Panics
/// Panics if `name` is not a valid region name (see [`is_valid_region_name`]).
/// This is a programmer error: names are developer-/component-chosen, never
/// end-user input, so a constraint violation indicates a bug at the call site.
pub fn region_metadata(name: &str, page: usize, x: f64, y: f64, w: f64, h: f64) -> String {
    assert!(
        is_valid_region_name(name),
        "region name must be non-empty ASCII alphanumeric/_/-/:, got: {name:?}"
    );
    format!(
        "#place(top + left, dx: {x}pt, dy: {y}pt, box(width: {w}pt, height: {h}pt)[#metadata((name: \"{name}\", page: {page}, x: {x}, y: {y}, w: {w}, h: {h})) <region>])\n"
    )
}

#[cfg(test)]
mod region_tests {
    use super::*;

    #[test]
    fn region_name_validation() {
        assert!(is_valid_region_name("done"));
        assert!(is_valid_region_name("tok-3"));
        assert!(is_valid_region_name("habit_streak"));
        assert!(
            is_valid_region_name("box:checkmark:0"),
            "colon namespace separator is allowed"
        );
        assert!(!is_valid_region_name(""), "empty is rejected");
        assert!(!is_valid_region_name("has space"), "space is rejected");
        assert!(!is_valid_region_name("quote\"inside"), "quote is rejected");
        assert!(!is_valid_region_name("paren)inside"), "paren is rejected");
    }

    #[test]
    #[should_panic(expected = "region name must be")]
    fn region_metadata_panics_on_unsafe_name() {
        // A name with a quote would close the Typst string literal and silently
        // break compilation, so it must be rejected loudly at the call site.
        let _ = region_metadata("bad\"name", 0, 0.0, 0.0, 1.0, 1.0);
    }
}
```

Note: the test module is named `region_tests` (not `tests`) to avoid colliding with any existing `mod tests` already in `render.rs`. If `render.rs` already imports nothing it needs, no extra `use` is required beyond the `use super::*;` inside the test module.

- [ ] **Step 3: Reduce `widget.rs` to just the trait**

Replace the entire contents of `crates/inkapp-core/src/widget.rs` with:

```rust
use crate::component::RenderCx;
use crate::ink::RegionInk;
use crate::manifest::Manifest;

/// A widget renders Typst markup that declares named regions, and interprets the
/// ink attributed to those regions. Render and readback co-located.
///
/// NOTE: obsolete — being removed in favor of `Component`. Do not add new impls.
pub trait Widget {
    type Output;
    /// Emit Typst markup (including `<region>` metadata for each region).
    fn render(&self, cx: &mut RenderCx) -> String;
    /// Interpret the strokes attributed to this widget's region(s).
    fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> Self::Output;
}
```

- [ ] **Step 4: Re-export `region_metadata` at the crate root**

In `crates/inkapp-core/src/lib.rs`, after the existing `pub use render::...` (or alongside the other `pub use` lines — there is no current `pub use render::` line, so add one), add:

```rust
pub use render::region_metadata;
```

(Place it in alphabetical-ish company with the other `pub use` lines, e.g. right after `pub use reconcile::{reconcile, DocOp};`.)

- [ ] **Step 5: Repoint all internal imports**

Edit each import line:

`crates/inkapp-core/src/runtime.rs`: `use crate::widget::RenderCx;` → `use crate::component::RenderCx;`

`crates/inkapp-core/src/components/calendar_view.rs`: `use crate::widget::RenderCx;` → `use crate::component::RenderCx;`

`crates/inkapp-core/src/components/notice.rs`: `use crate::widget::RenderCx;` → `use crate::component::RenderCx;`

`crates/inkapp-core/src/components/highlight_text.rs`: `use crate::widget::{RenderCx, Widget};` → `use crate::component::RenderCx;\nuse crate::widget::Widget;`

`crates/inkapp-core/src/components/stepper.rs`: `use crate::widget::{region_metadata, RenderCx, Widget};` →
```rust
use crate::component::RenderCx;
use crate::render::region_metadata;
use crate::widget::Widget;
```

`crates/inkapp-core/src/components/checkbox.rs`: `use crate::widget::{is_valid_region_name, region_metadata, RenderCx, Widget};` →
```rust
use crate::component::RenderCx;
use crate::render::{is_valid_region_name, region_metadata};
use crate::widget::Widget;
```

- [ ] **Step 6: Repoint external imports**

`apps/reading-queue/src/lib.rs`: `use inkapp_core::widget::{RenderCx, Widget};` →
```rust
use inkapp_core::component::RenderCx;
use inkapp_core::widget::Widget;
```

`crates/inkapp-harness/src/recording.rs`: `use inkapp_core::widget::region_metadata;` → `use inkapp_core::region_metadata;`

`crates/inkapp-harness/tests/simulator.rs`: `use inkapp_core::widget::region_metadata;` → `use inkapp_core::region_metadata;`

- [ ] **Step 7: Verify green**

Run: `cargo test --workspace`
Expected: all tests pass (the relocation is behavior-preserving; `region_name_validation` and `region_metadata_panics_on_unsafe_name` now run from `render.rs`).

Run: `cargo build --workspace 2>&1 | grep -i warning` (sanity)
Expected: no `unused import` warnings.

- [ ] **Step 8: Commit**

```bash
git add crates/inkapp-core/src apps/reading-queue/src/lib.rs crates/inkapp-harness
git commit -m "inkapp-core: relocate RenderCx + region helpers out of widget.rs (Widget removal, step 1)"
```

---

### Task 2: Delete the `Widget` trait and module; collapse the three component impls to inherent `read`

**Goal:** Remove the `Widget` trait, its module, and all three `impl Widget` blocks, replacing each with a single `render` path plus an inherent `read`; fix the handful of trait-form callers. This is the atomic core of the refactor — the trait cannot be half-removed, so it lands in one commit.

**Files:**
- Modify: `crates/inkapp-core/src/components/checkbox.rs` (drop `impl Widget`; add inherent `read`)
- Modify: `crates/inkapp-core/src/components/stepper.rs` (drop `impl Widget`; move its `render` body into `Component::render`; add inherent `read`)
- Modify: `crates/inkapp-core/src/components/highlight_text.rs` (drop `impl Widget`; convert `render`+`read` to inherent methods)
- Modify: `crates/inkapp-core/src/lib.rs` (remove `pub mod widget;`)
- Delete: `crates/inkapp-core/src/widget.rs`
- Modify: `apps/reading-queue/src/lib.rs` (`self.text.render(cx)`; drop `use ...Widget`)
- Modify: `crates/inkapp-harness/tests/e2e.rs`, `crates/inkapp-harness/tests/exercisers.rs` (drop `use ...Widget`)

**Acceptance Criteria:**
- [ ] The `Widget` trait no longer exists anywhere; `widget.rs` is deleted; `pub mod widget;` is gone from `lib.rs`.
- [ ] `Checkbox`, `Stepper`, `HighlightableText` each have exactly one `render` path and an inherent `fn read` with the same signature/behavior as the old `Widget::read`.
- [ ] No `impl Widget`, `use ...Widget`, or `Widget::render(` remains in the workspace.
- [ ] All pre-existing `.read()` call sites (unit tests, harness, `ArticleBody`) compile and pass unchanged — no assertion weakened.

**Verify:** `cargo test --workspace` → all pass; `grep -rn 'trait Widget\|impl Widget\|use .*::Widget\|Widget::render\|dyn Widget\|pub mod widget' crates apps` → no matches.

**Steps:**

- [ ] **Step 1: `HighlightableText` → inherent methods**

In `crates/inkapp-core/src/components/highlight_text.rs`:
- Change the import line `use crate::component::RenderCx;\nuse crate::widget::Widget;` (from Task 1) to just `use crate::component::RenderCx;` (drop the `Widget` import).
- Change the block `impl Widget for HighlightableText {` to an inherent impl, removing the `type Output` line. Replace:

```rust
impl Widget for HighlightableText {
    /// The set of highlighted token strings.
    type Output = Vec<String>;

    fn render(&self, _cx: &mut RenderCx) -> String {
```

with:

```rust
impl HighlightableText {
    fn render(&self, _cx: &mut RenderCx) -> String {
```

and change `fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<String> {` (the `Widget::read` body) — keep it exactly as-is inside the now-inherent `impl HighlightableText` block. (The `Output` doc comment that sat above `type Output` is dropped with it.)

Note: `render` and `read` become private inherent methods. `ArticleBody` lives in another crate and calls them — so they must be `pub`. Make both `pub fn render(...)` and `pub fn read(...)`.

- [ ] **Step 2: `Checkbox` → drop `impl Widget`, add inherent `read`**

In `crates/inkapp-core/src/components/checkbox.rs`:
- Change the import line `use crate::widget::Widget;` — delete it entirely. (Keep the `use crate::component::RenderCx;` and `use crate::render::{is_valid_region_name, region_metadata};` from Task 1.)
- Delete the whole block:

```rust
impl<M> Widget for Checkbox<M> {
    type Output = bool;

    fn render(&self, cx: &mut RenderCx) -> String {
        self.render_at(cx.page, 20.0, 40.0, 16.0, 16.0)
    }

    fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> bool {
        self.read_state(ink, manifest) != CheckState::Empty
    }
}
```

- Add an inherent `read` to the existing `impl<M> Checkbox<M> { ... }` block (where `with_msg`, `label`, `render_at`, `read_state` live), e.g. right after `read_state`:

```rust
    /// Whether this checkbox's region was marked (any non-empty ink).
    pub fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> bool {
        self.read_state(ink, manifest) != CheckState::Empty
    }
```

The `Component` impl (the Typst `#checkbox(...)` `render`, `typst_sources`, `decode`) is unchanged. The old `Widget::render` (absolute `render_at` placement) is intentionally dropped — `render_at` remains a `pub` inherent method for any test/app that placed absolutely, and `Component::render` is the real render path.

- [ ] **Step 3: `Stepper` → drop `impl Widget`, fold render into Component, add inherent `read`**

In `crates/inkapp-core/src/components/stepper.rs`:
- Change the import line: delete `use crate::widget::Widget;` (keep `use crate::component::RenderCx;` and `use crate::render::region_metadata;` from Task 1).
- Delete the `impl Widget for Stepper { ... }` block, but first move its `render` body into `Component::render`. Currently:

```rust
impl Widget for Stepper {
    type Output = u64;

    fn render(&self, cx: &mut RenderCx) -> String {
        let name = self.region_name();
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
    ...
```

Replace both blocks' render wiring so `Component::render` contains the real body, and add an inherent `read`:

```rust
impl Stepper {
    /// The new count: the carried base plus the increment strokes (idle = base).
    pub fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> u64 {
        self.carried_base(manifest) + self.increments(ink, manifest)
    }
}

impl Component for Stepper {
    type Msg = u64;

    fn render(&self, cx: &mut RenderCx) -> String {
        let name = self.region_name();
        let (x, y, w, h) = (20.0_f64, 40.0_f64, 16.0_f64, 16.0_f64);
        let mut s = region_metadata(&name, cx.page, x, y, w, h);
        s.push_str(&format!(
            "#place(top + left, dx: {x}pt, dy: {y}pt, rect(width: {w}pt, height: {h}pt, stroke: 0.5pt)[#align(center + horizon)[+]])\n"
        ));
        s.push_str(&format!("#text[{}]\n", self.count));
        s
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<u64> {
        // unchanged
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

(The inherent helpers `region_name`, `carried_base`, `increments` in the first `impl Stepper { ... }` block are unchanged; the new inherent `read` can live in that same block instead of a second one — either is fine.)

- [ ] **Step 4: `ArticleBody` (reading-queue) → call inherent methods**

In `apps/reading-queue/src/lib.rs`:
- Change `use inkapp_core::component::RenderCx;\nuse inkapp_core::widget::Widget;` (from Task 1) to just `use inkapp_core::component::RenderCx;` (drop the `Widget` line).
- In `impl Component for ArticleBody`, change:

```rust
    fn render(&self, cx: &mut RenderCx) -> String {
        Widget::render(&self.text, cx)
    }
```

to:

```rust
    fn render(&self, cx: &mut RenderCx) -> String {
        self.text.render(cx)
    }
```

The `self.text.read(ink, manifest)` call in `decode` already resolves to the new inherent `HighlightableText::read` — no change.

- [ ] **Step 5: Harness → drop `Widget` imports**

`crates/inkapp-harness/tests/e2e.rs`: `use inkapp_core::widget::{RenderCx, Widget};` → `use inkapp_core::component::RenderCx;` (drop `Widget`; `RenderCx` now from `component`).

`crates/inkapp-harness/tests/exercisers.rs`: `use inkapp_core::widget::{RenderCx, Widget};` → `use inkapp_core::component::RenderCx;`. Also update the comment on/around line 18 ("Widget::render default-placement path is covered by the checkbox unit tests.") to drop the stale `Widget::render` reference, e.g. "Checkbox default-placement render is covered by the checkbox unit tests."

The `.read()` calls in these files resolve to the new inherent `Checkbox::read` / `HighlightableText::read` (identical signatures) — no body changes.

- [ ] **Step 6: Delete the module**

Delete the file `crates/inkapp-core/src/widget.rs`.
In `crates/inkapp-core/src/lib.rs`, remove the line `pub mod widget;`.

- [ ] **Step 7: Verify green and trait-free**

Run: `cargo test --workspace`
Expected: all tests pass.

Run: `grep -rn 'trait Widget\|impl Widget\|use .*::Widget\|Widget::render\|dyn Widget\|pub mod widget' crates apps`
Expected: no matches.

- [ ] **Step 8: Commit**

```bash
git add -A crates/inkapp-core apps/reading-queue/src/lib.rs crates/inkapp-harness
git commit -m "inkapp-core: delete the Widget trait; Component is the sole view abstraction (Widget removal, step 2)"
```

---

### Task 3: Update `appdx.md` to the single-`Component` reality

**Goal:** Make the developer-experience doc true again — remove the open question that this work resolved and any stale two-layer framing.

**Files:**
- Modify: `docs/appdx.md`

**Acceptance Criteria:**
- [ ] The open-questions "parking lot" no longer contains the `Widget`/`Component` two-layer consolidation bullet.
- [ ] No prose elsewhere in `appdx.md` frames `Widget` and `Component` as two layers.
- [ ] `grep -n 'Widget' docs/appdx.md` returns nothing (or only an intentional, accurate mention if one is added).

**Verify:** `grep -n 'Widget' docs/appdx.md` → no matches; visual read of the Components section reads coherently.

**Steps:**

- [ ] **Step 1: Remove the resolved open question**

In `docs/appdx.md`, delete the final parking-lot bullet (currently the last item under "Open questions parking lot"):

```markdown
- **`Widget`/`Component` two-layer consolidation.** `Widget` (`render` + typed
  `read`) is a lower-level primitive distinct from `Component` (`render` +
  `decode` → `Msg`); the module is now named `components`, but whether the typed-
  `read` layer should fold into `Component` is an open tidy.
```

- [ ] **Step 2: Scan for and reword any remaining two-layer framing**

Run: `grep -n 'Widget' docs/appdx.md`
For each remaining hit (if any), reword so it describes the single `Component` abstraction (render + decode), with the typed `read` mentioned only as an ordinary helper method where relevant. Based on the current doc, the parking-lot bullet is the only mention — confirm the grep is now empty.

- [ ] **Step 3: Commit**

```bash
git add docs/appdx.md
git commit -m "appdx: Widget/Component consolidation done — Component is the sole view abstraction"
```

---

## Notes for the implementer

- This is a **behavior-preserving refactor**. There are no new failing tests to write first; the existing workspace suite is the regression net. The TDD discipline here is: run `cargo test --workspace` before and after each task and confirm the same tests pass. Never weaken or delete an assertion to make the build green — if something fails to compile, fix the import/method path, don't change what a test checks.
- Tasks are strictly ordered: 2 depends on 1 (the relocated `RenderCx`/region paths must exist before the trait is deleted), 3 depends on 2 (the doc should claim "done" only once the code is).
- After Task 2, `Checkbox::render_at` becomes the only "absolute placement" render entry point; it stays `pub` because tests/apps that lay out absolutely use it. Do not delete it.
</content>
