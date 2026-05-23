# inkapp — Spec #2: The Deterministic Harness

**Date:** 2026-05-22
**Status:** Approved (design); plan pending

## Context

The Typst spike (Spec #1) settled the two physically hard bets independently: Typst, used
as a library, yields region bounding boxes in PDF-point coordinates (Bar 1), and the
`rm-files` crate reads `.rm` v6 ink back. Spec #1 deliberately left the framework itself
unbuilt — the handler API, the render/device traits, and any widget or app were named as
non-goals.

This spec builds the **deterministic harness**: the part of the framework that can be
built and proven entirely in software, with no tablet and no network. It is the keystone,
because every hardware-dependent step so far (spike Bars 4, 5-device, 6) has been the
project's recurring bottleneck. If a widget cannot be tested without the tablet in hand,
"test the various parts" stays slow forever. The harness removes that dependency for the
geometric and logical core of the loop.

The deliverable is **a harness and a great testing/iteration setup**, not a shipped app.
The "apps" in this spec are test exercisers — minimal programs that drive the hard parts
of real use cases that widgets will eventually serve — kept simple and flexible.

### Decomposition (this spec is #2 of a three-spec arc)

- **Spec #2 — The deterministic harness (this doc).** Framework extraction, the `.rm`
  writer + writer validation, the widget trait and loop simulator, the layers inspector,
  and two non-AI exerciser widgets. Fully automated; no hardware, no network.
- **Spec #3 — E2E gesture-fixture layer.** Record a small real-ink vocabulary on-device
  once; transplant fixtures into target regions by translate/scale; use it to validate the
  realism of the writer's output. One documented manual recording bar.
- **Spec #4 — The AI step.** A backend-pluggable trait (shell out to `claude`, `opencode`,
  `pi`, … ; direct OpenAI later) with a deterministic fake; the first real backend shells
  out to the `claude` CLI using the **subscription, never an API key**; the layers
  inspector artifact from this spec is the vision input; `handwriting-command` and
  `analysis` exerciser widgets land here.

### Key decisions carried in from brainstorming

- The framework's keystone abstraction is the **widget**: render and readback co-located,
  the way a web `<input>` both draws itself and parses its posted value.
- The `.rm` **writer must be provably good** — its correctness is what licenses
  software-only testing and retires most manual hardware checks.
- The layers inspector is **one artifact, two audiences**: the same composited image
  (PDF background + ink layer + region overlay) serves the human inspector now and the
  Spec #4 vision model later.
- Device-specific infrastructure may carry a device name (`rm-files`); the framework and
  apps are device-agnostic. (The reader's prior `rmfiles` crate is renamed to `rm-files`.)

## Goals (this spec)

Stand up the device-agnostic framework core, a faithful `.rm` writer, an in-software loop
simulator, a layers inspector, and two exerciser widgets — all provable under `make test`
with no hardware and no network.

## Non-goals (deferred)

- The gesture-fixture e2e layer and any on-device recording (Spec #3).
- Any AI/LLM step, handwriting recognition, or analysis (Spec #4).
- Transport/sync (`rmapi`): pushing to and pulling from a real device. The simulator
  substitutes synthesized ink for the device round-trip; transport is hardware and stays
  out of this spec.
- Building `bujo` or any shipped app. The exercisers are tests, not products.

## A. Crate layout & the device seam

```
inkapp/
  crates/
    rm-files/           # .rm v6 READER + new WRITER          (reMarkable-specific infra)
    inkapp-core/        # render, manifest, widget trait,      (device-AGNOSTIC framework)
                        #   readback model, diffing
    inkapp-remarkable/  # Device impl: PDF<->screen transform, (reMarkable-specific infra)
                        #   .rm read/write bridge, ink synth
    inkapp-harness/     # loop simulator + layers inspector,   (device-agnostic; uses
                        #   generic over a Device              #  inkapp-remarkable in tests)
  spikes/typst-readback/  # historical record; superseded by inkapp-core
```

The `rmfiles` crate absorbed in Spec #1 is **renamed to `rm-files`** (directory
`crates/rm-files`, Cargo package `rm-files`, library `rm_files`). Its existing tests move
with it and must stay green.

The architectural keystone is a **minimal `Device` seam** in `inkapp-core`. The simulator
needs a defined boundary at which to substitute synthesized ink for "the user wrote on a
tablet" — and that boundary is exactly the device abstraction the glossary anticipates.
For this spec the trait is deliberately small: coordinate transforms plus read/synthesize
ink. It explicitly excludes transport.

```rust
// inkapp-core
pub trait Device {
    /// Map a manifest region (PDF-point coords) into device ink space, and back.
    fn pdf_to_device(&self, r: PdfRect, page_h: f64) -> DeviceRect;
    fn device_to_pdf(&self, p: DevicePoint, page_h: f64) -> PdfPoint;
    /// Parse this device's native ink bytes into framework strokes.
    fn read_ink(&self, bytes: &[u8]) -> Result<Vec<Stroke>>;
    /// Synthesize native ink bytes from framework strokes (the writer side).
    fn write_ink(&self, strokes: &[Stroke]) -> Result<Vec<u8>>;
}
```

`inkapp-remarkable` implements `Device`: `read_ink`/`write_ink` wrap the `rm-files`
reader/writer; the transforms encode the PDF-point ↔ reMarkable-screen-pixel mapping from
`remarkable-pdf-mechanics.md`.

## B. Core abstractions (`inkapp-core`)

### Render + manifest

The render path is extracted from the spike, applying the findings-doc to-do list:

- One `compile_to_document(world) -> Result<PagedDocument>` shared by PDF export and region
  recovery (no duplicated compile/diagnostic logic).
- `typst_to_pdf_rect` looks up the **per-page** height of `region.page`, not page 0, so
  multi-page documents convert correctly.
- Fonts are **embedded/pinned**, not searched from the host, to keep output deterministic
  (content-only pushes depend on this; mechanics doc §11).
- `World::font` returns `Option` and never panics on a bad index.

The `Manifest` carries `regions: Vec<Region>` (each: name, page index, PDF-point rect) plus
a monotonic `version` marker. It is embedded into the rendered PDF and recovered from it
via the proven introspector + `metadata`/label pattern.

### Widget — the keystone abstraction

```rust
pub trait Widget {
    type Output;
    /// Emit Typst markup; declare named regions via the metadata+label pattern.
    fn render(&self, cx: &mut RenderCx) -> Markup;
    /// Interpret the strokes attributed to this widget's region(s).
    fn read(&self, ink: &RegionInk, manifest: &Manifest) -> Self::Output;
}
```

Render and readback live together so a widget is a single, self-contained unit: it knows
both how it appears and how to interpret what the user did to it.

### Readback + diffing

Stroke attribution maps each stroke to a region by containment (spike-proven). Two
primitives wrap it:

- **New-ink diffing:** act only on strokes not seen in a prior cycle. Without it, every
  readback re-processes the whole page; it is core, not optional.
- **Stale-version guard:** reject ink whose manifest version predates the current document,
  so ink written against an old layout is never misattributed to a new one.

> **As-built note:** both are implemented as standalone, unit-tested primitives
> (`readback::diff_new`, `readback::guard_version`). The harness's `simulate` runs a
> *single* cycle per call, so it does not yet call them in the loop (there is no prior cycle
> to diff against, and synthesized ink is not yet version-tagged). Wiring diffing + the guard
> into a **multi-cycle** `simulate` is deferred to whichever later spec first needs a
> multi-cycle loop — `StepTrace`'s doc comment anticipates the `Vec<StepTrace>` shape.

## C. The `.rm` v6 writer + validation (`rm-files`)

The writer is the trust anchor: if synthetic ink is provably faithful, software tests can
stand in for the tablet. It emits the v6 tagged-block stream from framework `Stroke`s
(paths, pen, color, pressure) — the inverse of the existing reader.

**Validation, three levels:**

1. **Round-trip (automated):** `write(strokes) → Scene::parse → assert == strokes` within
   float tolerance. Proves writer and reader agree.
2. **Real-fixture round-trip (automated):** parse a real device capture
   (`stamped-labels.rmdoc`) to `Stroke`s, re-write them with the writer, re-parse, and
   assert the strokes (tool, color, geometry, and per-point telemetry) are byte-faithfully
   preserved. Proves the writer losslessly encodes *real device stroke data*, not just
   synthetic data.
3. **Device acceptance (Spec #3, manual bar):** push a written `.rm` to a tablet and
   confirm it renders. Deferred, like the spike's `on_device` test.

> **As-built note (corrects this section's original wording):** level 2 is a *round-trip of
> real fixture data*, not a byte/block-structure diff of the writer's output against the
> device's raw bytes. Our minimal writer deliberately omits the surrounding CRDT ids and
> scaffolding blocks a real file carries, so its bytes are not block-identical to a device
> file — only the *decoded stroke content* is. True byte-structural / block-framing fidelity
> against a device is therefore part of the **Spec #3 on-device acceptance bar**, not proven
> here. Levels 1–2 are the gate for "we can stop hand-testing that the writer round-trips
> the data the harness relies on."

## D. Loop simulator + layers inspector (`inkapp-harness`)

### Simulator

Runs the full loop in-process, deliberately through the **real** writer→parse path (no
in-memory shortcut), driven by a scenario script of "user does gesture G in region R":

```
render(app, state) -> (pdf, manifest)
  -> scenario synthesizes Stroke[] into region rects
  -> device.write_ink -> bytes -> device.read_ink            (real writer + real parse)
  -> device.device_to_pdf -> attribute to regions -> diff new ink
  -> app.handle(readback, state) -> state'                   (record inspector artifact)
```

Routing the simulator through the real `write_ink`/`read_ink` is what makes a passing test
trustworthy: it exercises the same bytes path a device would.

### Layers inspector — one artifact, two audiences

`inspect(document, manifest, ink) -> Png` rasterizes the page with **`typst-render`** (pure
Rust, deterministic — no `pdftoppm` dependency), then composites the ink layer and
region-rect overlays on top. Each step emits a PNG (a visual trace) plus the structured
manifest as text. Humans eyeball it now; the Spec #4 vision model consumes the identical
artifact later.

## E. Exerciser widgets & the central risk

Two non-AI exercisers prove the widget pattern across the difficulty range:

- **`checkbox`** — one region; `read -> bool` (stroke present, distinguishing a mark from a
  scribble-out). Establishes the trivial case and the render+readback round-trip.
- **`highlightable-text`** — **the central technical risk of this spec.** The spike proved
  *block-level* region rects; highlighting needs *span-level* rects ("which words"). Method:
  wrap each highlightable token in a labelled `#box` so its rect is individually
  recoverable, then map a highlighter swipe to the set of token-boxes it overlaps. Test:
  synthesize a swipe over "lazy dog" → `read` → assert `{lazy, dog}`. If span-level rects
  prove unreliable, that is a finding that reshapes the readback model — surfaced early and
  cheaply, the way Bar 1 was surfaced in the spike.

## F. Testing & iteration setup

- Every exerciser and the writer validation runs under `make test` — fully deterministic,
  no hardware, no network.
- **Golden-image snapshots** on inspector PNGs catch visual regressions.
- The iteration loop is: edit a widget → `cargo test` → inspect the composited PNG. The
  harness exists to make that loop fast and hardware-free.

## Done when

- The four crates build, with `rmfiles` renamed to `rm-files` and its absorbed tests green.
- The `.rm` writer passes round-trip (level 1) and structural-diff-vs-fixtures (level 2)
  validation.
- The loop simulator round-trips `checkbox` and `highlightable-text` through the real
  `write_ink → read_ink` path and the exercisers' assertions pass.
- The layers inspector emits composited PNGs (background + ink + region overlay) via
  `typst-render`, with golden-image snapshots.
- New-ink diffing and the stale-version guard are covered by tests.

## Risks

- **Span-level region rects (highlightable-text) are this spec's central bet**, the analog
  of the spike's Bar 1. If Typst cannot yield reliable per-token geometry, the readback
  model for fine-grained selection needs rework. The exerciser exists to settle this early.
- **Writer fidelity.** Round-trip validation proves writer↔reader agreement but not that
  the bytes match a real device's exactly; structural diffing against real fixtures narrows
  the gap, and Spec #3's device-acceptance bar closes it. Until then, "faithful" means
  "agrees with the reader and matches recorded fixture structure."
- **`typst-render` raster determinism** must hold across machines for golden-image
  snapshots to be stable; pinned fonts (Section B) are a precondition.
