# Comprehensive component test suite — automated + synthesizer self-proof + manual real-pen

**Status:** design approved 2026-05-28
**Owner:** Dan
**Scope:** test infrastructure additions to `inkapp-harness` and `inkctl`; per-layer test corpora under existing crates. No app-facing or framework API changes.

## Summary

Extend the existing layer-by-layer trust model (established in
`2026-05-26-inkctl-test-harness-design.md` and
`2026-05-27-reader-thorough-test-design.md`) into a comprehensive coverage
suite organized as three tracks:

- **Track A — Automated.** Fast in-process `#[test]`s driven by
  `inkapp-harness`. Carries the bulk of the matrix.
- **Track B — Synthesizer self-proof.** For each component or behavior, the
  suite builds a doc, drives synthesized ink through `inkctl`, decodes it,
  and publishes a side-by-side annotated PDF (original / synth-overlay /
  decoded result) to the reMarkable. A checklist band on each page captures
  the human's ✓ / ✗ / notes on sync-back. This is the artifact that earns
  trust in the synthesizer.
- **Track C — Manual real-pen.** Small. Self-instructing docs covering only
  behaviors where synthesized ink cannot faithfully stand in (stroke shape,
  multi-segment highlights, pressure edge cases), plus a stretch
  one-per-component sanity sample.

A single source of truth — `manual-test.toml` — drives all three tracks per
test entry. Verification across all three tracks is fully automated; the
human's only role is marking up the on-device doc as instructed and ticking
the checklist.

## Goals

1. Give every component and load-bearing framework behavior at least one
   assertion-bearing test in Track A, with Track B coverage at the layers
   where the synthesizer's honesty matters.
2. Earn justified trust in `inkctl`'s ink synthesizer through Track B's
   on-device visual proof, so Track A can carry the full matrix at speed.
3. Reserve Track C for behaviors where real-pen ink genuinely differs from
   synthesized strokes, plus a small periodic sanity sample per component.
4. Reuse the existing layer-by-layer build cadence; add no new crates; keep
   `inkctl` a thin shell over a public `inkapp-harness` API.

## Non-goals

1. No app-facing surface changes. Component, connector, runtime APIs are
   unchanged.
2. No new test-only crate. The infrastructure lives in `inkapp-harness`; the
   CLI lives in `inkctl`; corpora co-locate with the code they cover.
3. No live-cloud or live-Readwise additions. L2 fake cloud and existing
   cassette tests are the substrate.
4. Tracks B and C never gate CI. Only Track A runs in `cargo test`.
5. Not a coverage-percentage target. The bar is "every behavior in the
   inventory has an assertion in Track A and, where relevant, a self-proof
   page in Track B."

## Architecture

Three coverage tracks applied selectively per layer in the existing trust
model:

| Layer | A | B | C |
|---|---|---|---|
| L2  device coord transform & `.rm` parse                                        | yes | —   | —     |
| L3  Typst render + region attribution                                            | yes | yes | —     |
| L4  components (Heading, Section, ActionBand, NavBand, Index, Gesture, Highlight, Checkbox, Stack, Passage, Notice, Stepper, CalendarView) | yes | yes | stretch |
| L5  framework integration (mode axis, manifest version guard, pagination, connectors, manifest sealing, secret isolation) | yes | yes | —     |
| L6  full agent loop                                                              | yes | —   | small |

Track B is the bridge: once a layer's self-proof PDFs look right when the
human scrolls through them on the device, Track A carries the matrix at
speed.

Common infrastructure, added once, reused by all three tracks:

```
crates/inkapp-harness/src/suite/
  mod.rs          # public API the CLI calls
  schema.rs       # manual-test.toml types (serde)
  build.rs        # toml -> published doc (Track B and Track C builders)
  overlay.rs      # synth-stroke overlay onto a rendered page (lifted from inspector.rs)
  selfproof.rs    # 3-panel page composition for Track B
  verify.rs       # decode + expect-eval + checklist parse + rollup
  publish.rs      # idempotent push via inkapp::publish and the existing DeviceTransport
  reset.rs        # republish clean copies
  report.rs       # JSON + human + optional _reports/*.pdf

crates/inkctl/src/cmd/suite.rs   # thin clap wrapper over suite::*
```

The CLI gains one new top-level subcommand (`Top::Suite`) wired through
`inkctl/src/main.rs`. Architectural rules from the inkctl-test-harness
design carry over unchanged: CLI owns no domain logic; all behavior lives
in `inkapp-harness`; Track A `#[test]`s call the same API the CLI calls.

## `manual-test.toml` — single source of truth per test entry

```toml
id          = "component-highlight-text"
title       = "highlight_text — multi-word and line-spanning"
layer       = 4
component   = "highlight_text"          # informational
tracks      = ["A", "B", "C"]           # which tracks consume this entry

[setup]
fixture     = "highlight_only"          # a pre-canned harness app
# OR
inline      = """
= Test passage
This is the body where you will highlight things.
"""

[[case]]
key            = "single-word"
region         = "body"
instruction    = "Highlight the word 'banana' on line 3."
synth          = { kind = "highlight", target = { text = "banana" } }
expect         = { msg = "Highlighted", args = { text = "banana" } }

[[case]]
key            = "line-spanning"
region         = "body"
instruction    = "Highlight from 'apple' on line 1 through 'cherry' on line 2."
synth          = { kind = "highlight", target = { text_range = ["apple", "cherry"] } }
expect         = { msg = "Highlighted", args = { text_range = ["apple", "cherry"] } }
```

Conventions:

- `synth` drives Tracks A and B; `instruction` drives Track C; both
  `expect` against the same decoded-msg shape, so one verifier serves all
  three tracks.
- `synth = "skip"` makes the case manual-only; `instruction = "skip"` makes
  it synth-only. Default is both.
- `expect.msg` matches an `App::Msg` variant string; `expect.args` is
  structural (extra fields tolerated unless `strict = true`).
- Region names are stable across re-render so `expect` does not depend on
  layout.

## Doc-builder

For each entry, `inkapp_harness::suite::build` produces a regular inkapp
document via the normal render pipeline:

- Instructions are rendered inside their target regions so the human reads
  what they are about to mark up.
- A **checklist band** at the foot of the last page contains one `checkbox`
  region per case (keyed by `case.key`) plus one freeform `notes` region.
- The manifest is the real manifest — same encryption, same version,
  identical to any other inkapp doc.

## Track B — synthesizer self-proof renderer

For each entry, Track B produces one publishable PDF. Each case is one
page, three stacked panels plus a per-case checklist row:

```
+-----------------------------------------------------------+
|  Case: single-word           layer 4 / highlight_text     |
+-----------------------------------------------------------+
|  Panel 1 — Original                                       |
|  [rendered region exactly as the real doc shows it]       |
+-----------------------------------------------------------+
|  Panel 2 — Synthesized ink overlay                        |
|  [same render + synthesized strokes in contrasting color  |
|   + per-region bbox outlines]                             |
+-----------------------------------------------------------+
|  Panel 3 — Decoded                                        |
|  msg: Highlighted { text: "banana" }                      |
|  region: "body"  strokes: 1  attribution: clean           |
|  expected: Highlighted { text: "banana" }   PASS          |
+-----------------------------------------------------------+
|  [ ] looks right    [ ] looks wrong    notes:             |
+-----------------------------------------------------------+
```

The final page is a summary index: case id -> automated PASS/FAIL plus a
mirrored checklist row.

Implementation notes:

- Panel 1 reuses the normal render pipeline, cropped to the case region
  with a fixed margin.
- Panel 2 reuses inspector overlay code (extended to render to PDF, not
  only debug PNGs).
- Panel 3 is plain Typst text populated from synth/decode/expect.
- The self-proof PDF is itself a regular inkapp doc and rides the same
  `rm-device` sync path as any app's published doc.

## The manual loop

Folder layout on the device:

```
/inkapp-tests/
  self-proof/L4-components.pdf     # Track B, one file per layer
  self-proof/L5-framework.pdf
  manual/<test-id>.pdf             # Track C, one file per entry
  _reports/<run-ts>.pdf            # optional last verify report
```

Commands (thin wrappers around `inkapp_harness::suite::*`):

- `inkctl suite publish --track {self-proof|manual} [--layer N | --id <test-id> | --all]`
  Builds and pushes. Idempotent: only re-uploads if content hash changed.
- `inkctl suite verify --all [--track ...]`
  Pulls every doc under `/inkapp-tests/{self-proof,manual}/`; for each:
  1. Decode ink against the embedded manifest (stale manifest is rejected
     with a clear diagnostic).
  2. Per case, evaluate `expect` against the decoded msg.
  3. Parse the checklist region: ✓ / ✗ / notes (notes as raw stroke set;
     OCR is a v2 stretch).
  4. Compose a rollup:
     `{ test_id, case_key, automated: pass|fail|skip, human: pass|fail|unmarked, notes }`.
  5. Emit JSON to stdout, human report to stderr, optionally a
     `_reports/<ts>.pdf` published back to the device.
- `inkctl suite reset <test-id> | --all`
  Republishes a clean copy. Refuses to reset a doc with unverified ink
  unless `--force`.
- `inkctl suite status`
  Local snapshot of on-device tests, last verify, last reset, outstanding
  markup.

Edge cases:

- A case with neither ✓ nor ✗ is `human: unmarked` (distinct from
  `human: fail`).
- Conflicts between automated and human verdicts are reported, not
  auto-resolved.
- `verify` is read-only against cloud; never mutates server state.

## Inventory by layer

**L2 — Device coord transform & `.rm` parse.** PDF pt <-> device px
round-trip on default and one non-default geometry; `.rm` v6 fixture
decode within tolerance; out-of-page stroke handling per stated contract.

**L3 — Render & attribution.** Typst frames -> manifest region boxes;
stroke containment attribution (inside / outside / straddling boundary);
multi-page region disambiguation.

**L4 — Components.** One suite entry per component, covering each
interaction the component claims:

- `heading` — tap = no-op; long-press / scribble decoded as nothing.
- `section` — content-only rendering; ink containment.
- `action_band` — tap-each-action decoding.
- `nav_band` — prev / next / jump decoding.
- `index` — entry-tap -> link-follow message.
- `gesture` — single tap, double tap, swipe directions, freeform region.
  Real-pen swipe shape is one of Track C's canonical entries.
- `highlight_text` — single-word, multi-word, line-spanning,
  paragraph-spanning. Canonical Track C entry.
- `checkbox` — toggle ✓ / ✗ / clear. Dogfoods the doc-checklist mechanic.
- `stack`, `passage`, `notice`, `stepper`, `calendar_view` — Track A
  layout coverage; Track B only if the component exposes an interaction.

**L5 — Framework integration.** Mode axis (ReadOnly vs Editable decode
branches); manifest version guard; pagination region-id stability;
connector refresh/flush ordering; encrypted manifest round-trip; secret
isolation (manifest export carries no secrets).

**L6 — Full agent loop.** One canonical `inkctl record`-emitted `#[test]`
per app (reading-queue, agenda, reader). One small Track C end-to-end
sample on the reader app.

Out of scope: app-level behavior tests (those live with the app);
live-cloud L3 backend (stays gated); existing live-Readwise cassette
tests.

## Corpora layout

```
crates/rm-device/tests/suite/                                  # L2
  coord-transform.toml
  rm-parse-fixtures.toml

crates/inkapp-core/tests/suite/                                # L3 and L5
  attribution-boundary.toml
  multi-page-region-ids.toml
  mode-axis-readonly-vs-editable.toml
  manifest-version-guard.toml
  pagination-region-stability.toml
  connector-refresh-flush-order.toml
  manifest-no-secrets.toml

crates/inkapp-core/src/components/<name>/tests/suite/          # L4
  heading.toml
  highlight_text/single-word.toml
  highlight_text/line-spanning.toml
  action_band.toml
  ...

apps/<app>/tests/suite/                                        # L6
  reader-loop.toml
  agenda-loop.toml
```

`suite::corpus::discover` walks these directories so `inkctl suite publish
--layer 4` finds entries by file location, not by a central registry.

Track A tests reference corpora via `suite::run_entry("path/to/foo.toml")`
to drive the synth + decode + expect path in-process. Same toml, same
expectations as Tracks B and C; only the transport differs.

## Build order

1. **Infrastructure first.** `suite::schema`, `suite::build` for plain
   manual docs, `suite::verify` (decode + expect-eval + checklist parse).
   One worked example per file type to prove the path. CLI verbs landed
   incrementally: `publish`, then `verify`, then `reset` and `status`.
2. **L2 corpus.** Small — mostly already covered by Track A; brings the
   corpus pattern in.
3. **L3 attribution corpus + first Track B self-proof doc.** This is the
   trust-earning step. Once the boundary-attribution self-proof PDF looks
   right when the human scrolls through it on the device, the synthesizer
   is honest enough to carry the matrix.
4. **L4 component corpus, one component at a time.** Each component lands
   with toml(s) + Track A test(s) + inclusion in the L4 self-proof PDF.
5. **L5 framework corpus.**
6. **L6 end-to-end samples** including the small Track C sample on the
   reader app.

## CI and pre-commit

Track A runs in `cargo test --workspace` as today; no new CI moves. Tracks
B and C are explicit `inkctl suite ...` invocations — never gate commits
on them. `make clippy` stays clean; `cargo fmt --check` stays clean.

## Updating `appdx.md`

Per project convention, each layer's corpus completion includes an update
to `docs/appdx.md` marking the relevant components and behaviors as
covered.
