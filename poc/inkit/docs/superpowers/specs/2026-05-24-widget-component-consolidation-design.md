# Widget/Component consolidation — design

**Status:** approved, ready for plan.
**Goal:** remove the `Widget` trait. `Component` becomes the sole view abstraction;
the typed `read` that `Widget` provided survives as plain inherent methods on the
structs that benefit from it. This closes the last surface-level wart an app author
hits (the doubled trait impl) before we start building real apps on inkapp.

## Why

`Widget` (`render` + typed `read` → `Output`) and `Component` (`render` + `decode`
→ `Vec<Msg>`) are two near-identical traits. `Widget` predates the switch to
`Component` and is now baggage:

- **Nothing consumes `Widget` polymorphically.** No `dyn Widget`, no `<W: Widget>`
  bound exists anywhere except the impls themselves. The document flow holds
  `Box<dyn Component>`. The `Widget` *trait* therefore buys zero abstraction — it's
  used only as a per-type naming convention for a `render` + typed-`read` pair.
- **The doubled impls leak into app code on day one.** `ArticleBody`
  (reading-queue) implements `Component` but reaches into `Widget::render` and
  `Widget::read` to reuse `HighlightableText`. `Checkbox` and `Stepper` each carry
  both `impl Widget` and `impl Component`, with `Component::render` delegating to
  `Widget::render`.
- **The one genuine "reusable typed primitive"** — `HighlightableText` — already
  shows the trait is unnecessary: it is composed *inside* `ArticleBody`, never
  placed in a flow, and only ever needs `render` + `read` as ordinary methods.

This is a pure subtraction: delete a trait and a module, collapse the doubled
impls, keep every typed read as a plain method. No new abstraction is introduced.
(Alternatives considered: a `Widget`-as-reusable-core with a blanket `Component`
adapter — rejected as speculative machinery for a uniformity nothing needs; a
single trait carrying both `read` and `decode` — rejected because making `read`
optional needs unstable associated-type defaults.)

## The three roles, before and after

| Struct              | Today                                  | After                                                |
|---------------------|----------------------------------------|------------------------------------------------------|
| `HighlightableText` | `impl Widget` only (no `Component`)    | plain helper struct: inherent `render` + `read`      |
| `Checkbox<M>`       | `impl Widget` + `impl Component`       | `impl Component` + inherent `read` (+ `read_state`)  |
| `Stepper`           | `impl Widget` + `impl Component`       | `impl Component` + inherent `read`                   |

## Changes

### 1. Dissolve `crates/inkapp-core/src/widget.rs`

`widget.rs` holds the `Widget` trait plus three non-trait companions used widely
(harness `src` + tests, every component). Relocate the companions, delete the rest:

- `RenderCx` → `component.rs` (it is the context handed to `Component::render`).
- `region_metadata`, `is_valid_region_name` → `render.rs` (render-time markup/region
  emit). Re-export `region_metadata` at the crate root (`lib.rs`) because the
  harness imports it (`inkapp_core::widget::region_metadata` today).
- Delete the `Widget` trait, its doc, and the `region_name_validation` /
  `region_metadata_panics_on_unsafe_name` unit tests move with their functions into
  `render.rs`.
- Remove `pub mod widget;` from `lib.rs`.

### 2. `Checkbox<M>` (`components/checkbox.rs`)

- Delete `impl<M> Widget for Checkbox<M>`. Its `Widget::render` body merely called
  the existing pub inherent `render_at`, and `Component::render` is the real
  (Typst `#checkbox(...)`) render path — so nothing is lost.
- Add inherent `fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> bool`
  with the former `Widget::read` body (`self.read_state(ink, manifest) != Empty`).
- `read_state`, `render_at`, and the `Component` impl are unchanged.
- Fix imports: `RenderCx` from `component`, `region_metadata`/`is_valid_region_name`
  from `render`; drop `Widget`.

### 3. `Stepper` (`components/stepper.rs`)

- Delete `impl Widget for Stepper`. Move its `render` body into `Component::render`
  (today `Component::render` just delegates via `<Self as Widget>::render`).
- Add inherent `fn read(&self, ink, manifest) -> u64` with the former `Widget::read`
  body (`carried_base + increments`).
- `decode`, `state_key`, `render_state`, and the private helpers
  (`increments`, `carried_base`, `region_name`) are unchanged.
- Fix imports as in §2.

### 4. `HighlightableText` (`components/highlight_text.rs`)

- Delete `impl Widget for HighlightableText`; convert its `render` and `read` to
  inherent methods on the struct (bodies unchanged). It remains a plain helper
  composed inside `ArticleBody`.
- Fix imports as in §2.

### 5. `ArticleBody` (`apps/reading-queue/src/lib.rs`)

- `Widget::render(&self.text, cx)` → `self.text.render(cx)`.
- `self.text.read(...)` already resolves to the new inherent method.
- Drop `use inkapp_core::widget::{RenderCx, Widget};`; import `RenderCx` from its
  new path.

### 6. Harness (`crates/inkapp-harness`)

- `tests/e2e.rs`, `tests/exercisers.rs`: import `RenderCx` from its new path, drop
  `Widget`. `.read()` calls on `Checkbox`/`HighlightableText` now resolve to the
  inherent methods (identical signatures), so test bodies are unchanged.
- `tests/simulator.rs`, `src/recording.rs`: import `region_metadata` from its new
  path (crate-root re-export).

### 7. `docs/appdx.md`

- Remove the open-questions bullet **"`Widget`/`Component` two-layer
  consolidation"** (the trailing bullet of the parking lot).
- Scan the Components section for any prose framing `Widget`/`Component` as two
  layers and reword to the single-`Component` reality. (The parking-lot bullet is
  the primary mention; verify no others remain via grep.)

## Acceptance criteria

- [ ] The `Widget` trait no longer exists; `widget.rs` is deleted and `pub mod
      widget;` is gone from `lib.rs`.
- [ ] `RenderCx` lives in `component`; `region_metadata` + `is_valid_region_name`
      live in `render`; `region_metadata` is re-exported at the crate root.
- [ ] `Checkbox`, `Stepper`, `HighlightableText` each have exactly one `render`
      path and an inherent `read`; no `impl Widget` remains.
- [ ] `ArticleBody` and the harness compile against the new paths/methods with no
      behavioral change.
- [ ] `appdx.md` no longer lists the consolidation as an open question, and no
      stale two-layer framing remains.

## Verification

- `cargo build --workspace` is clean.
- `cargo test --workspace` is green — every pre-existing test passes unchanged
  (no assertion weakened; `.read()` callers hit inherent methods of the same
  signature, only `use … Widget` import lines are dropped).
- `grep -rn 'trait Widget\|impl Widget\|: Widget\|dyn Widget\|::widget' crates apps`
  returns nothing (excluding `RwLock`/cache `.read()` calls, which are unrelated).
</content>
</invoke>
