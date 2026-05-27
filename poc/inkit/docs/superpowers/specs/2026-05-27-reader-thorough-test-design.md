# Thorough test of the reader app, agent-driven through inkctl

**Status:** design approved 2026-05-27
**Owner:** Dan
**Scope:** test coverage for `apps/reader` and the framework layers it sits on, using `inkctl` as the agent-driving lens; bug fixes in `inkctl` / `inkapp-harness` discovered during the effort

## Summary

Prove the inkapp stack from the bottom up by writing committed Rust tests at each
layer of the data-and-control pipeline that the reader app sits on, using
`inkctl` as the agent-driving lens. Trust at layer N is the precondition for
testing layer N+1. Bugs found in `inkctl` or `inkapp-harness` while testing are
fixed in the same commit cycle — there is no parallel "test against a
known-broken tool" track.

The terminal artifact is a committed test suite that grows across multiple
crates plus a hardened `inkctl` whose observers have been verified honest at
each layer.

## Goals

1. Establish trustworthy automated tests for the reader app and the framework
   layers below it, organized by dependency layer so a green test at layer N
   licenses the use of `inkctl`'s lens for layer N+1.
2. Dogfood `inkctl` as the agent-driving surface it claims to be: the agent
   exercises behaviors via the CLI, records sessions, and emits Rust
   `#[test]`s into the natural crate. CLI bugs found in flight get fixed.
3. Land at least three end-to-end, agent-driven reader sequences as emitted
   `#[test]`s under `apps/reader/tests/loop_emitted.rs`, including the
   stale-manifest and offline-connector paths.

## Non-goals

1. **No new app-facing surface.** The reader's public API, components, and
   connectors are not redesigned here.
2. **No reMarkable hardware in the loop.** Everything runs against `rm-cloud`'s
   `fake` feature and the `inkapp-harness` ink synthesizer.
3. **No duplication of the live Readwise cassette tests** beyond what's needed
   to exercise the reader's connector path.
4. **No reader feature work.** If a reader bug requires a behavior change, we
   file it and stop; we do not redesign the reader inside a testing pass.
5. **Not a coverage-percentage target.** The bar is "every behavior in the
   inventory has an assertion, and the lens we used to find it is honest."

## Approach: the loop, per layer

For each layer N (order in *Scope* below):

1. **Inventory.** List the public functions / components in scope and what
   each is supposed to do. Read existing tests first; do not re-test what's
   already adequately covered.
2. **Drive with the lens.** Exercise the behavior with `inkctl` (where the CLI
   has a verb for it) *and* with the harness library directly. If they
   disagree, that is an `inkctl` bug — fix it before continuing.
3. **Write the Rust test.** Committed `#[test]` in the natural crate.
   Hand-written when the assertion is structural; emitted from `inkctl record`
   when the value is in capturing a real sequence of agent moves.
4. **Fix inkctl/harness bugs inline.** Any defect discovered (CLI lying,
   observer missing a field, synthesizer producing wrong coords, etc.) is
   fixed in the same change-set as the test that exposed it. Each fix gets
   its own commit; the test that exposed it cites the fix commit in its
   message.
5. **Promote.** When the layer's inventory is green and the lens has been
   corrected, move to layer N+1.

**"Trusted" definition.** A layer is trusted when (a) every public surface in
the inventory has at least one assertion-bearing test, (b) every `inkctl` verb
that exposes that layer has been used to produce one of those tests or has
been deliberately marked unused-here, and (c) no known `inkctl` bug at that
layer is outstanding.

## Scope by layer

### Layer 2 — Device coord transform (`crates/rm-device/src/lib.rs`)

- PDF point → device pixel round-trip, default page geometry (420×560pt) and
  one non-default geometry.
- Stroke decoded from a fixture `.rm` lands in the expected PDF-point region.
- Out-of-page strokes are handled deterministically per the impl's stated
  contract (clamped vs. dropped).
- `inkctl` lens check: `ink list` after `ink replay` of a fixture reports the
  same stroke set the library sees.

### Layer 3 — Readback / attribution (`crates/inkapp-core/src/readback.rs`)

- Stroke wholly inside a region → attributed to that region.
- Stroke straddling two regions → attributed per the current containment
  rule; test pins the rule so intent and behavior can be reconciled.
- Stroke outside any region → unattributed bucket, not silently dropped.
- `guard_version`: ink from a stale manifest version is rejected with the
  documented error.
- `inkctl` lens check: `page describe` regions match what `attribute` sees;
  `page inspect --layers` overlay agrees with attribution.

### Layer 4 — Components in isolation (`crates/inkapp-core/src/components/*`)

Priority order, biggest payoff first:

- `GestureAction` — tap inside / outside / on the edge → correct `Msg` (or none).
- `ActionBand` — each action's region is tappable; renders consistently across pages.
- `HighlightText` — highlight stroke over text yields the expected highlight
  `Msg`; render produces a region of the right shape.
- `NavBand` — Prev / Home / Next taps each fire the correct `Msg`; disabled
  states render but don't decode.
- `Stack` composer — composed children's regions don't collide; child decode
  is forwarded.
- `Index` (compact-row) — row taps map to the correct entry; masthead is
  non-interactive.
- `inkctl` lens check: `page inspect` region overlay shows the same boxes the
  component reports.

### Layer 5 — Reader composition (`apps/reader/src/lib.rs`)

- `update` is exhaustively tested per `Msg` variant against the existing
  model.
- `view` produces a non-empty doc set for the canonical fixture state
  (Index + at least one Article).
- Connector wiring: a `RefreshDone` `Msg` produces the expected model delta.

### Layer 6 — Full loop, agent-driven (`apps/reader/tests/`)

- Publish reader → Index page → tap an article row → step → Article page →
  NavBand Next → step → Article page 2 → NavBand Home → step → Index. Emitted
  from an `inkctl record` session.
- Stale-manifest path: publish v1, tap an action, publish v2 *without*
  applying v1's ink, replay v1's ink → version-guard rejection surfaces as
  the documented `Msg`/no-op.
- Offline-connector path: cassette in offline mode → Index renders the
  cached state, no crash.

Each of these forces `session step` + a reader entry in `inkctl`'s app
registry to land — the first known `inkctl` bug we will hit and fix.

## Test locations

| Layer | Crate dir                                                              |
|-------|------------------------------------------------------------------------|
| 2     | `crates/rm-device/tests/`                                              |
| 3     | `crates/inkapp-core/tests/`                                            |
| 4     | `crates/inkapp-core/tests/`                                            |
| 5     | `apps/reader/tests/`                                                   |
| 6     | `apps/reader/tests/loop_emitted.rs`                                    |
| Lens  | `crates/inkctl/tests/` (CLI shape) and `crates/inkapp-harness/tests/` (library shape) |

Naming: extend the existing file when one already covers the area (e.g.
add to `crates/inkapp-core/tests/readback.rs`); otherwise use
`layerN_<topic>.rs` only when grouping is needed. Layer-5 update/view tests
stay in `apps/reader/tests/app.rs`; layer-6 emitted sequences land in
`apps/reader/tests/loop_emitted.rs`, one file with multiple `#[test]`s.

## Commit shape

- `tests(layer-N): <area> — <what's now covered>` for each batch of tests.
- `fix(inkctl|inkapp-harness|inkapp-core|rm-*): <bug>` for each defect found
  in flight. The next `tests(...)` commit references the fix commit hash in
  its body so the trail is legible.
- A layer ends with a `docs(appdx): layer N covered` commit that updates
  `docs/appdx.md`'s testing section.

Commits land on `main` as they go.

## Done criteria

**Per-layer.**

- Inventory above fully covered by passing `#[test]`s.
- `make test` and `make clippy` clean for the whole workspace.
- Every `inkctl` / `inkapp-harness` bug surfaced at this layer has either a
  landed fix or, if deferred by explicit decision, a tracking entry in
  `docs/inkctl-known-issues.md` *and* a note in `docs/appdx.md` that the
  corresponding lens is not yet trusted.
- `docs/appdx.md`'s testing section updated to mark the layer covered.

**Overall.**

- All six layers green.
- `apps/reader/tests/loop_emitted.rs` contains at least the three Layer-6
  sequences listed above.
- `inkctl` can drive the reader end-to-end (`session step` works; reader is
  in `inkctl`'s app registry).
- This spec and a matching plan under `docs/superpowers/plans/` are
  committed before implementation work starts.

## Out of scope

- No benchmark suite.
- No CI changes.
- No doc site or public test-authoring guide.
- No coverage-percentage target.
