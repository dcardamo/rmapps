# inkapp — Spec #8: Typst component authoring ("T")

**Date:** 2026-05-24
**Status:** Approved (design); plan pending

## Context

`docs/appdx.md` records a build order for making the doc true:
**S** secrets → **E** encryption → **C** connector plugin trait → **M** mode axis
*(all done, Specs #5–#7)* → **T** Typst authoring. This spec is **T** — the last
item on that list, called out at the top of the appdx as *"the one remaining
aspirational piece."*

Today the doc's "Components" section makes promises the code does not keep:

- The doc says a new component's **render half is authored in Typst's own scripting
  language** — *"functions, `#let`, conditionals, loops, `context` — not by
  string-building Typst markup from Rust"* (appdx §Components). It even shows
  `components/checkbox.typ` as a real `#let checkbox(...) = box[...]` function and
  notes *"the framework wraps it in a region."*
- **The code does the exact opposite.** Every component's `render()` builds Typst
  markup by Rust `format!` string-building (`checkbox.rs`, `calendar_view.rs`,
  `highlight_text.rs`, `notice.rs`). There is no `.typ` authoring path at all.
- **`InkWorld` cannot even load a second file.** `world.rs`'s `source()`/`file()`
  return `FileError::NotFound` for everything except the single in-memory
  `main.typ`, so `#import "...typ"` is impossible. The mechanism that authoring
  requires simply does not exist.

Dan's decision: build the **authoring seam** and prove it end-to-end on one
component, rather than migrate every component. The appdx itself licenses a mixed
world — *"Small components keep render inline (one file); only elaborate
presentation graduates to a `.typ` file"* (§Ergonomics check). So "make the doc
true" means **the capability exists and is demonstrated**, not that all four
components convert. `Checkbox` is the doc's own literal example, so converting it
makes the shown `checkbox.typ` actually true.

### What this spec makes true

- **`InkWorld` resolves multi-file imports** from a virtual filesystem of baked
  Typst sources, so a component's render half can live in its own `.typ` file and
  be `#import`ed by the assembled `main.typ`.
- **A framework Typst prelude** providing `#region(name, body)` — the one primitive
  that emits the `<region>`-labelled metadata (`context` + `here().position()` +
  `measure()`) that `recover_regions` reads back. This replaces the boilerplate
  currently hand-duplicated inside every component's render string.
- **A component declares an authored Typst source**; the render driver registers it
  in the World, auto-imports it into `main.typ`, and the component's `render()`
  emits a *call* to its Typst function (with serialized scalar/string props) wrapped
  in `#region(name)[…]` — instead of inline markup.
- **`Checkbox` is converted** to author its presentation as `checkbox.typ`, proving
  the whole path: render → multi-file compile → region recovery → `decode`
  round-trips unchanged.

### The seam — where each half lives

A component's render half splits along the boundary the appdx names
(*"Typst owns the render half only"*):

| Piece | Lives in | Who writes it |
|---------------------------|------------------------------|------------------------|
| Presentation (layout, rect, styling) | a `.typ` function (`#let checkbox(label) = box[...]`) | Typst scripting |
| The region *name* (render↔decode contract) | Rust component field | host language (Rust) |
| Region metadata emission | framework prelude `#region(name, body)` | framework, once |
| The call binding props → function | Rust `render()` emits `#region(n)[#checkbox(label: "…")]` | host language (Rust) |
| `decode` (ink → `Msg`) | Rust | host language (Rust), always |

This unifies both region cases the doc describes under one prelude primitive:

- **Auto-region** (component = its own bounding box, e.g. `Checkbox`): the framework
  wraps the single call site in one `#region`.
- **Subdivided** (component mints `tok-0`, `tok-1`, …, e.g. `HighlightableText`):
  the Typst author calls `#region` directly inside a loop.

Only the auto-region case (Checkbox) is exercised in this spec; the subdivided case
is enabled by the same primitive and proven later if/when a component migrates.

### Explicitly out of scope

- **Per-device conditional layout in Typst.** The appdx lists it under T, but no
  multi-device render exists yet (single device, single page geometry — see
  `runtime.rs` `DOC_PAGE_W/H`). Building Typst-side per-device branching now would be
  speculative machinery with no caller. Deferred.
- **Component composition in Typst.** Components compose in Rust today (`flow![…]`).
  Moving composition into Typst is a separate rethink, not required to make the
  authoring claim true. Deferred.
- **Migrating `CalendarView`, `HighlightableText`, `Notice`.** Allowed to stay
  inline by the doc. The prelude `#region` is additive, so they keep working
  unchanged; each can migrate later in its own increment.
- **Rich prop types.** Prop serialization is scalars/strings only this spec (reusing
  `esc_typst_str`). Passing structured data (arrays, dicts) into a Typst function is
  a later refinement.
- **State-field payload, event sourcing, multi-user/cloud** — all remain future
  (FUTURE.md and the appdx open-questions list), untouched here.

### Position in the spec arc

Specs #5–#7 built S, E, C, M. This is **#8 = T**, the final build-order item. After
it lands, `docs/appdx.md`'s build-order banner is fully true and the only remaining
work is the explicitly-future material (event sourcing/CRDT, multi-user/cloud, key
management) plus the logged tidies (the `Widget`/`Component` two-layer
consolidation; demand-driven refresh; migrating the remaining components to authored
Typst).

## Architecture

All changes land in `inkapp-core`. No new crate.

### 1. `InkWorld` → virtual filesystem (`world.rs`)

`InkWorld` gains a `HashMap<FileId, Source>` of additional sources beyond `main`.
`source()` looks up the map (falling back to `main`, then `NotFound`); `file()` may
serve the same sources as `Bytes` if Typst requests them that way. Construction
takes the main source plus an iterator of `(VirtualPath, source_text)` pairs.

Baked sources are registered under stable virtual paths, e.g.
`/inkapp/region.typ` (prelude) and `/components/checkbox.typ`. They are
**`include_str!`-baked into the binary** — no runtime disk access — preserving
determinism (the project's core harness bet) and honoring the render-is-sandboxed /
no-I/O rule.

`render.rs::compile_to_document` gains a sibling (or an extended signature) that
accepts the extra sources and threads them into `InkWorld::new`. The existing
single-argument path can delegate with an empty source set so callers that don't
author Typst are unaffected.

### 2. Framework Typst prelude (`crates/inkapp-core/typst/region.typ`, baked)

```typst
// region(name, body): emit <region>-labelled metadata for the laid-out body,
// then place the body. Recovery (recover_regions) queries the <region> label and
// downcasts the MetadataElem, so the label MUST attach to the metadata element.
#let region(name, body) = box[
  #context [
    #metadata((
      name: name,
      page: here().position().page - 1,
      x: here().position().x / 1pt,
      y: here().position().y / 1pt,
      w: measure(body).width / 1pt,
      h: measure(body).height / 1pt,
    )) <region>
  ]
  #body
]
```

This is precisely the pattern duplicated today in `highlight_text.rs` (which
already measures with `measure()`), generalized to take the name and body as
parameters. `recover_regions` is **unchanged** — it still queries the `<region>`
label and reads the metadata dict.

### 3. Component authoring declaration + render driver (`component.rs`, `runtime.rs`)

A `Component` that authors its render in Typst declares its source: the function's
`.typ` text and the virtual path to register it under (a small trait method or
associated data; exact shape decided in the plan). The render driver
(`document_source` in `runtime.rs`):

1. Collects the prelude + every flow component's declared source into the extra-
   source set, de-duplicated by path.
2. Prepends `#import` lines for the prelude and each component module to `main.typ`.
3. Walks the flow as today, but each authored component's `render()` now returns a
   `#region("<name>")[#<fn>(<props>)]` call rather than inline markup.

Components that don't author Typst (the other three, this spec) return their
existing inline strings; the driver just doesn't register a source for them. The
prelude is always registered (cheap, and the first migrating component needs it).

### 4. Convert `Checkbox` (`checkbox.rs` + new `checkbox.typ`)

`checkbox.typ`:

```typst
#let checkbox(label) = box[
  #rect(width: 14pt, height: 14pt, stroke: 0.5pt) #text[#label]
]
```

`Checkbox::render` (the `Component` impl) emits:

```
#region("<self.name>")[#checkbox(label: "<esc label>")]\n
```

`self.name` (caller-supplied today) remains the render↔decode contract; `decode`
and `read_state` are untouched. The `Widget` impl's `render_at` (absolute
placement, used by device tests) can keep its current inline form or route through
the same Typst function — decided in the plan; the `Component` path is the one this
spec proves.

## Testing

1. **Multi-file World.** `InkWorld` with a registered extra source compiles a
   `main.typ` that `#import`s it. Asserts the import resolves (no `NotFound`).
2. **`#region` prelude parity.** A document using `#region("r", body)` recovers a
   region whose rect matches the inline `<region>` pattern it replaces, within the
   spike's 0.0pt-delta expectation (golden/delta test).
3. **Checkbox authored round-trip.** Render the converted `Checkbox` → multi-file
   compile → `recover_regions` → synthetic ink in the region → `decode` returns
   `[on_check]`; empty ink returns `[]`. Mirrors the existing checkbox decode tests.
4. **App regression.** `reading-queue` (which composes `Checkbox`) still renders and
   its checkbox still decodes to `Archived` — guards against the seam breaking a
   live caller.

## Risks / decisions deferred to the plan

- **Rect parity for Checkbox.** The `Component` impl today hardcodes `w:14,h:14`;
  the generic `#region` measures `body`. Wrapping only the tappable affordance (the
  14×14 rect) keeps hit-testing semantics; where the recovered rect legitimately
  differs, the component's tests are updated to the new geometry. Pinned in the plan.
- **Exact shape of the source-declaration API** (trait method vs associated const vs
  a registration call) — a plan-level call; the spec fixes the behavior, not the
  signature.
- **`measure()` of a body containing a fresh `#let` binding** must behave inside the
  prelude `box` exactly as it does in `highlight_text.rs` today; the parity test
  (#2) covers this.

## Open questions (logged, not blocking)

- Whether `Checkbox`'s caller-supplied region name should eventually be
  framework-minted (the appdx's "regions are automatic by default" framing). Today
  the name is the working contract; not churned here.
- Whether `render_at` (the `Widget` absolute-placement path) should also route
  through `checkbox.typ`, unifying the two render entry points. Tied to the broader
  `Widget`/`Component` consolidation already logged in the appdx.
- Structured prop passing (arrays/dicts into Typst functions) — needed before
  `CalendarView` (which renders a list) can migrate.
