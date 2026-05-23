# inkapp — Spec #3: The E2E Gesture-Fixture Layer

**Date:** 2026-05-23
**Status:** Approved (design); plan pending

## Context

Spec #2 built the **deterministic harness**: the device-agnostic framework core, a faithful
`.rm` writer, an in-software loop simulator, a layers inspector, and two exerciser widgets —
all provable under `make test` with no hardware and no network. That is the **unit tier** of
the test pyramid: the simulator drives the loop with *synthetic* strokes (`Gesture::Tap`,
`Gesture::Swipe`) shaped from region rectangles.

This spec adds the **e2e tier**. We record a small vocabulary of **real ink gestures**
on-device **once**, check them in, and let tests *transplant* a real gesture into any target
region by translate/scale — so the same harness loop runs on real ink instead of synthetic
strokes. The synthetic tier proves the geometry and logic; the e2e tier proves that real
human strokes survive the write→parse→attribute→read path and produce the right widget
readings.

It also closes the three items Spec #2 deliberately deferred (its "As-built note" callouts):

1. **Writer byte/block-structural fidelity** vs. a real device file (Spec #2's level-2 was
   only a fixture-data round-trip).
2. **reMarkable transform fidelity** — calibrate/validate the self-consistent PDF↔scene
   model against real recorded ink at known PDF positions.
3. **Device acceptance** — write a `.rm`, push via `rmapi`, confirm it renders.

### Position in the three-spec arc

- **Spec #2 — The deterministic harness** (merged). Unit tier; synthetic ink.
- **Spec #3 — E2E gesture-fixture layer (this doc).** Real-ink tier; one documented manual
  recording bar; closes the three deferred fidelity items.
- **Spec #4 — The AI step.** Backend-pluggable AI trait with a deterministic fake; the
  layers-inspector artifact is the vision input. The `handwritten-word` fixture recorded
  here is banked for Spec #4's handwriting input.

### Decisions carried in from brainstorming

- **Writer fidelity: acceptance-driven.** Device-acceptance (#3) is the real fidelity gate.
  We add an *automated structural-diff* signal (our writer's block tree vs. a real device
  file's, by decoded content — not byte-identity) to catch writer drift between rare
  hardware runs, and extend the writer only as far as the device demands to render.
  Byte-identity is an explicit non-goal.
- **Transform fidelity: validate, then gate adoption.** Measure recorded ink at known PDF
  points against the current fit-to-width model; assert within tolerance. Adopt fitted
  constants (regenerating goldens once, with provenance) *only if* the measured error
  exceeds a threshold. The harness converges to reality when it must and stays stable when
  the model is already good enough.
- **Transplant fit: per-gesture policy.** Each fixture declares a fit mode
  (`aspect-fit` | `stretch` | `stretch-x`); shape-sensitive gestures keep their shape,
  span gestures fill the region width.
- **Vocabulary: the six briefed gestures + `scribble-out`.** The extra one exercises the
  `Checkbox` widget's mark-vs-scribble discrimination end-to-end with real ink.

## Goals (this spec)

Stand up the real-ink e2e tier: a maintainable catalog of self-instructing recording
templates, a one-time on-device recording, an automatic extraction to checked-in fixtures,
transplant math, and `Gesture::Fixture` wired into the existing simulator — plus the three
fidelity bars. Everything except two documented manual bars is provable under `make test`.

## Non-goals (deferred)

- Any AI/LLM step, handwriting recognition, or analysis (Spec #4). `handwritten-word` is
  recorded and banked but not interpreted here.
- A statistical model of human ink variance. Fixtures are *representative real ink* (a few
  samples per gesture), not a distribution.
- Transport as framework runtime. `rmapi` is shelled out to **only** from the `#[ignore]`
  manual bars; the device transport seam stays out of the framework, as in Spec #2.
- Multi-cycle `simulate`. Still single-cycle, as in Spec #2.

## A. The organizing principle: 1:1:1:1 symmetry

Each gesture is exactly **one catalog entry → one template PDF → one device document → one
fixture file**. That symmetry is the maintainability contract: there is never a "which sheet
was that on" question, and adding a gesture later touches exactly one thing in each layer.

```
catalog entry (code)  ─►  template PDF  ─►  /InkAppDev/fixtures/<name>  ─►  gestures/<name>.json
   fixtures::catalog()      generated         pushed by #[ignore]            written by extraction
```

## B. Crate layout & where things live

All gesture-fixture machinery lives in **`inkapp-harness`** (the e2e/iteration crate that
already owns the simulator, inspector, and exercisers, and already uses `inkapp-remarkable`
in tests). Keeping it in one crate is itself a maintainability decision.

```
crates/inkapp-harness/
  src/
    fixtures.rs        # gesture catalog, fixture types, JSON load, transplant math
    recording.rs       # template-PDF generation; extraction (recording -> fixtures)
  tests/
    fixtures/
      gestures/        # checked-in fixture JSON, one per gesture
        checkmark.json  circle.json  arrow.json  highlight-swipe.json
        strike-through.json  handwritten-word.json  scribble-out.json
      recordings/      # checked-in raw device captures (.rmdoc), one per gesture
        2026-05-23-checkmark.rmdoc  ...  2026-05-23-calibration.rmdoc
    transplant.rs      # transplant-math unit tests (all fit modes)
    e2e.rs             # real-ink exercisers via Gesture::Fixture (+ goldens)
    transform_fidelity.rs  # deferred #2: validate model vs calibration taps
    record.rs          # #[ignore] manual bars: push templates, pull captures
    acceptance.rs      # #[ignore] manual bar: write .rm, push, eyeball (deferred #3)
  tests/golden/        # new real-ink composites added here
```

The **writer structural-diff** (deferred #1) lives in `rm-files` (it concerns `.rm` bytes
and reuses the existing `tests/fixtures/stamped-labels.rmdoc`).

The `#[ignore]` bars shell out to the `rmapi` CLI from a small test-only helper in
`recording.rs`; this is not framework runtime and preserves Spec #2's "transport out of the
framework" boundary.

## C. The gesture catalog (single source of truth)

`fixtures::catalog() -> &[CatalogEntry]` is the one list everything derives from. Each entry:

```rust
pub struct CatalogEntry {
    pub name: &'static str,        // "checkmark"; the gesture id embedded in the template
    pub tool: Tool,                // Pen | Highlighter -> Stroke.highlighter
    pub fit: Fit,                  // AspectFit | Stretch | StretchX
    pub instruction: &'static str, // printed on the template ("draw a check in each box")
    pub box_shape: BoxShape,       // Square | Wide  (guide-box aspect, matched to the gesture)
    pub sample_text: Option<&'static str>, // faint words to act on (highlight / strike)
}
```

| name              | tool        | fit         | box  | drives (now / later)                                  |
|-------------------|-------------|-------------|------|-------------------------------------------------------|
| `checkmark`       | pen         | aspect-fit  | sq   | `Checkbox` positive (e2e)                              |
| `scribble-out`    | pen         | stretch-x   | sq   | `Checkbox` mark-vs-scribble discrimination (e2e)      |
| `highlight-swipe` | highlighter | stretch-x   | wide | `HighlightableText` span selection (e2e)              |
| `strike-through`  | pen         | stretch-x   | wide | future text-strike / span-delete widget               |
| `handwritten-word`| pen         | aspect-fit  | wide | Spec #4 handwriting input (banked)                    |
| `circle`          | pen         | aspect-fit  | sq   | "circle a task" / selection use case                  |
| `arrow`           | pen         | aspect-fit  | wide | linking / pointing use case                           |

Three drive e2e assertions in *this* spec (`checkmark`, `scribble-out`, `highlight-swipe`);
the rest are recorded-and-banked so a future widget gets real ink without another hardware
session.

## D. The recording workflow (one-time manual bar)

Self-instructing templates live in a single device folder so you can browse them on the
tablet and know what to do with each.

1. **Generate templates.** For each catalog entry, `recording::render_template(entry)`
   produces one PDF that prints **its own title and how-to** ("**Checkmark** — draw a ✓ in
   each of the 3 boxes"), draws **3 guide boxes** (named regions `box-0/1/2`, sized to the
   gesture's natural shape), and — for `highlight-swipe`/`strike-through` — prints faint
   sample words inside each box so the gesture is drawn *as in real use*. Content starts
   below the top ~40pt so the pen toolbar never covers a cell
   (`remarkable-pdf-mechanics.md §7`). The manifest embedded in the PDF carries the
   **gesture id** plus each box's PDF rect, so the document is fully self-describing.
   A separate **calibration** template prints crosshairs at known PDF coordinates with a
   "tap the centre of each cross" instruction (used by deferred #2).
2. **Push** (`#[ignore]`, `tests/record.rs`): create `/InkAppDev/` then
   `/InkAppDev/fixtures/` (non-recursive `mkdir`, per `§10`), and `rmapi put` each template
   in. The document title is the gesture name, so the folder lists as `calibration`,
   `checkmark`, `circle`, … — browsable and self-explaining on the device.
3. **Draw.** On the tablet, open each doc, follow its printed instruction, draw in each
   box; tap the calibration crosses. Sync device → cloud.
4. **Pull** (`#[ignore]`, `tests/record.rs`): `rmapi get` the `/InkAppDev/fixtures/` folder
   into `tests/fixtures/recordings/`.
5. **Extract** (automated; deterministic from the checked-in `.rmdoc`s):
   `recording::extract_all()` parses each capture via `Remarkable::read_ink`, recovers the
   embedded manifest, attributes strokes to boxes by containment, normalizes each box's
   strokes to *the gesture's own bounding box*, and writes `gestures/<name>.json`. Matching
   a capture to its catalog entry uses the **embedded gesture id**, not the filename.

Only steps 2–4 (push / draw / pull) are manual. Once the `.rmdoc`s are checked in, extraction
and everything downstream is automated and regenerable. Re-recording one gesture re-pushes
and re-pulls a single document; no other gesture is touched.

## E. Fixture format + provenance

One JSON per gesture in `tests/fixtures/gestures/`. Multiple drawn samples are banked in a
`samples` array (keeping one file per gesture); e2e tests use `default`.

```json
{
  "name": "checkmark",
  "tool": "pen",
  "fit": "aspect-fit",
  "default": 0,
  "samples": [
    { "native_aspect": 1.42, "strokes": [ { "points": [[0.10, 0.55], [0.40, 0.05]] } ] },
    { "native_aspect": 1.38, "strokes": [ { "points": [ /* ... */ ] } ] },
    { "native_aspect": 1.51, "strokes": [ { "points": [ /* ... */ ] } ] }
  ],
  "source": {
    "recording": "recordings/2026-05-23-checkmark.rmdoc",
    "device": "reMarkable Paper Pro Move",
    "recorded": "2026-05-23"
  }
}
```

- **Points are normalized to the gesture's own bounding box** (`[0,1]²`, PDF y-up), *not*
  the guide box — the box is only used to attribute which strokes belong to the gesture.
- **`native_aspect = bbox_w / bbox_h`** is stored *because* unit-square normalization
  discards the real width:height ratio, and the aspect-aware fit modes need it back.
- **`tool`** maps to `Stroke.highlighter`. **`source`** records the recording, device, and
  date for provenance and regenerability.

## F. Transplant math

- **Normalize (extraction):** for each sample, `u = (px − bbox.x0)/bbox.w`,
  `v = (py − bbox.y0)/bbox.h` (PDF y-up). Store `native_aspect = bbox.w/bbox.h`.
- **Transplant (replay)** of a unit-box sample into a target rect `T`:
  - `stretch` → `x = T.x0 + u·T.w`, `y = T.y0 + v·T.h`. Fills `T`; ignores shape.
  - `stretch-x` → fill width; `height = T.w / native_aspect`, centred vertically in `T`.
    Span gestures (swipe, strike, scribble) span the region and keep proportion.
  - `aspect-fit` → fit a box of the native aspect inside `T`, centred:
    `W = min(T.w, native_aspect·T.h)`, `H = W / native_aspect`. Shape gestures
    (checkmark, circle, arrow, handwriting) keep their shape.

Unit tests pin all three modes against hand-checked target rects (the subtle case is that
`aspect-fit` must *restore* the native aspect that normalization discarded).

## G. Replay through the existing simulator

`Scenario`'s gesture source gains a real-ink variant:

```rust
pub enum Gesture {
    Tap,                       // synthetic (Spec #2)
    Swipe,                     // synthetic (Spec #2)
    Fixture(&'static str),     // real ink: load gestures/<name>.json, transplant default sample
}
```

In `simulator::synthesize`, a `Fixture` step loads the fixture, transplants its `default`
sample into the target region's rect (per the fixture's `fit`), and yields
`Vec<Stroke>` — feeding the **exact same** `write_ink → read_ink → attribute` path the
synthetic gestures use. Nothing else in the loop changes; this is a new ink *source*, not a
new code path.

**Real-ink e2e exercisers** (`tests/e2e.rs`), each with a committed golden composite:

- `checkmark` → `done` region → `Checkbox::read` is `true`.
- `scribble-out` → `done` region → exercises the `Checkbox` mark-vs-scribble discrimination
  with real ink (the negative/disambiguation case).
- `highlight-swipe` transplanted over the `tok-4`/`tok-5` rects → `HighlightableText::read`
  returns `{lazy, dog}`.

## H. Closing the three deferred items

1. **Writer structural-diff (automated, `rm-files`).** Decode both `write_scene(...)` output
   and `stamped-labels.rmdoc` into block trees; assert our output is a structural
   *equivalent* — same line-item block types, ordering, and decoded stroke content — *not*
   byte-identity. This is the cheap automated drift signal; the device-acceptance bar is the
   render gate.
2. **Transform fidelity (validate, gate adoption; `tests/transform_fidelity.rs`).** From the
   checked-in calibration capture, pair each known crosshair PDF point `pᵢ` with its
   recorded tap centroid in raw device coords `dᵢ` (via `Scene::parse`, *before*
   `device_to_pdf`). Compare to the model's `pdf_to_device(pᵢ)`; assert max error within a
   stated tolerance. If error exceeds the threshold, least-squares fit the model's
   constants (scale + x-centre + y-offset), adopt them in `inkapp-remarkable`, regenerate
   goldens once, and record provenance in this doc and the crate. Automated against the
   checked-in capture.
3. **Device acceptance (manual `#[ignore]`, `tests/acceptance.rs`).** Write a known stroke
   set via `write_ink`, `rmapi put` it under `/InkAppDev/acceptance/`, and eyeball that it
   renders on the tablet. Mirrors `spikes/.../on_device.rs`; documented run command.

## I. Automation boundary

- **Automated under `make test`:** transplant math (all fit modes), real-ink replay e2e +
  new goldens, writer structural-diff, transform-fidelity validation, fixture extraction
  (regenerates JSON from the checked-in `.rmdoc`s).
- **Manual `#[ignore]` bars (documented run commands):**
  - **Record the vocabulary** — push templates to `/InkAppDev/fixtures/`, draw, pull.
  - **Device acceptance** — push a written `.rm` to `/InkAppDev/acceptance/`, eyeball.
  - Both honor the `rmapi` v4/token/`mkdir` gotchas in `remarkable-pdf-mechanics.md §10`.

The one-time recording, once checked in, makes everything except those two bars automated —
mirroring the spike's `#[ignore] on_device` pattern.

## Done when

- The gesture catalog drives self-instructing per-gesture template PDFs (+ a calibration
  sheet); the `#[ignore]` push/pull bars target `/InkAppDev/fixtures/`.
- The vocabulary is recorded once; per-gesture `.rmdoc` captures **and** extracted
  `gestures/*.json` fixtures are checked in.
- Transplant math is unit-tested across `aspect-fit` / `stretch` / `stretch-x`.
- `Gesture::Fixture` is wired into the simulator; the three real-ink exercisers pass
  (`checkmark`, `scribble-out`, `highlight-swipe`) with committed goldens.
- Writer structural-diff is automated vs. `stamped-labels.rmdoc`; device-acceptance
  `#[ignore]` is documented.
- Transform-fidelity validation is automated against the calibration capture; adoption is
  gated on tolerance with provenance recorded.
- `make test` and `make clippy` are green.

## Risks

- **The transform may be materially off.** The gated-adoption path handles it, but a swap
  shifts goldens; surfaced cheaply as a finding (Bar-1 style) with provenance.
- **Single-/few-sample fixtures.** A few human draws per gesture, not a distribution.
  Accepted: representative real ink, banked with variants for forgiveness.
- **Aspect normalization.** The unit-square + `native_aspect` split is the subtle bit; the
  transplant unit tests pin it.
- **`rmapi` fragility.** Token-clobber / v4 break / non-recursive `mkdir` affect only the
  two manual bars; documented mitigations from the mechanics doc apply.
