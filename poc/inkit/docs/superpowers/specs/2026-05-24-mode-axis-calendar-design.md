# inkapp — Spec #7: The mode axis + calendar connectors ("M")

**Date:** 2026-05-24
**Status:** Approved (design); plan pending

## Context

`docs/appdx.md` records a build order for making the doc true:
**S** secrets → **E** encryption → **C** connector plugin trait *(all done, Specs #5–#6)* →
**M** mode axis → **T** Typst authoring. This spec is **M**.

Today the doc's "Components" section makes promises that the code does not keep:

- The **three interaction modes** (Display / Capture / Control) exist only *implicitly*, as
  three unrelated widgets — `Notice` (renders, decodes nothing), `HighlightableText` (freeform
  decode), `Checkbox` (structured decode). Nothing carries a *mode*.
- The **`mode` axis** (`CalendarView { events, mode: ReadOnly | Editable }`, where `view`/`update`
  decides the mode from the backing connector's capability, and the same mode reaches both
  `render` and `decode`) is entirely aspirational. The appdx ergonomics check even admits
  "`mode` wasn't needed here," noting it "earns its keep only when a component fronts connectors
  of differing capability (a calendar on ICS vs CalDAV)."
- The doc's **read-only feed connector** archetype (ICS) is described but never built; only
  Readwise (bidirectional) exists.

Dan's decision: make **M** true *faithfully* — not a bare `mode` flag (which would be exactly the
speculative machinery the connector spec was disciplined about cutting), but the axis **with a
real capability contrast on both sides**. That means landing the doc's second and third connector
archetypes and a component that flips behavior by mode, demonstrated in a new app where `view`
chooses the mode from connector capability.

### What this spec makes true

- A shared **`Mode { ReadOnly, Editable }`** axis, carried by components as a *field* (a
  convention honored by `render`/`decode`), with the same value reaching both halves.
- A reusable **`CalendarView`** component that spans Display-like behavior (ReadOnly: renders
  content, decodes nothing) and Control-like behavior (Editable: renders per-event affordances,
  decodes structured ink → `Msg`).
- The doc's **read-only ICS feed** connector (`inkapp-ics`), and a **writable** CalDAV-shaped
  stand-in (`inkapp-localcal`) so the Editable branch is driven by *real* capability.
- A new minimal **`agenda`** app wiring both connectors, with `view` choosing each
  `CalendarView`'s mode from the backing connector's capability — the doc's "policy, not just
  capability" claim, shown side by side in one document.
- The `widgets/` module renamed to **`components/`** to match the doc's vocabulary (folded in as
  the plan's first task, since `CalendarView` lands there anyway).

### Explicitly out of scope

- **Real CalDAV.** `inkapp-localcal` (local-file/in-memory, CalDAV-shaped) stands in so the
  Editable path is exercised end to end; real CalDAV transport stays future.
- **Reworking the `Widget` trait.** `Widget` (`render` + typed `read` → `Output`) is a distinct,
  *live* lower-level primitive (`HighlightableText` is Widget-only; reading-queue consumes its
  `read()`), separate from `Component` (`render` + `decode` → `Msg`). Collapsing the two-layer
  design is a real refactor orthogonal to the mode axis — tracked as a follow-up, not bundled.
  This spec renames only the *module*, not the trait.
- **Event sourcing of calendar edits**, recurring-event/timezone correctness beyond what the
  `ical` crate yields for the fixtures, and the **state-field payload** open item — all stay out.
- **T (Typst authoring)** remains the next increment after this one.

### Position in the spec arc

- **Spec #1** — Typst-readback spike (merged).
- **Spec #2** — Deterministic harness (merged).
- **Spec #3** — E2E gesture-fixture layer (merged).
- **Spec #4** — The MVU app loop (merged).
- **Spec #5** — Secrets store + encryption (merged).
- **Spec #6** — Connector plugin trait + async loop (merged).
- **Spec #7 — The mode axis + calendar connectors (this doc).** Fourth increment of
  "make the doc true."

## Key decisions (resolved during brainstorming)

1. **Next increment: M, done faithfully.** Not a bare flag — the mode axis *with* a real
   capability contrast, because the doc itself says mode only earns its keep when a component
   fronts connectors of differing capability.
2. **`mode` is a field convention, not a trait change.** A shared `Mode` enum in `inkapp-core`
   that components carry as a field; `render`/`decode` honor it, and the framework needs no new
   machinery. Fixed-affordance components (`Checkbox`, `Notice`) don't carry a mode.
3. **Both sides real.** Build the read-only ICS feed *and* a writable local calendar connector so
   `view` chooses each mode from genuine capability, not a hardcoded flag.
4. **New `agenda` app** hosts the demo (a calendar doesn't belong in reading-queue).
5. **`ical` crate** for ICS parsing (fuller RFC 5545 coverage than a hand-roll), accepting its
   transitive deps.
6. **Editable interaction = cancel/decline an event** → `Msg::EventCancelled { uid }`. One
   structured control per event row; calendar-honest.
7. **`EventRow` lives in `inkapp-core`** so the component and both connectors share one type.
8. **Rename `widgets/` → `components/`** as the plan's first task; leave the `Widget` trait alone.

## Architecture

### The `Mode` axis (the central mechanism)

A shared enum in `inkapp-core`, carried by components as a field:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode { ReadOnly, Editable }
```

There is **no framework machinery** for the axis itself — it is a convention the component's own
`render`/`decode` honor, with the *same* value reaching both halves. That single-source property
is what guarantees a ReadOnly render that drew no affordances cannot have a decode that tries to
read them: both branch on `self.mode`. This is exactly the doc's "everything a component needs is
in its render input, including its affordances," and "the same `mode` reaches `decode`."

`Checkbox` and `Notice` are unchanged: fixed-affordance components do not carry a mode.

### `EventRow` (shared calendar type, `inkapp-core`)

The minimal event shape the component renders and both connectors produce:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRow {
    pub uid: String,        // stable id; also the region-name basis in Editable mode
    pub summary: String,
    pub start: String,      // RFC 5545 DTSTART, kept as-is for the fixtures
    pub end: String,        // DTEND
    pub cancelled: bool,    // optimistic/edit state; ReadOnly feeds always render false
}
```

(`uid` must satisfy `is_valid_region_name` when used to mint a region; the component namespaces it
as `evt:<uid>` and the connectors are responsible for sane uids from the fixtures.)

### `CalendarView` — new reusable component (`inkapp-core/components`)

```rust
pub struct CalendarView<M> {
    events: Vec<EventRow>,
    mode: Mode,
    on_cancel: fn(&str) -> M,   // value-message: uid → app Msg (Editable only)
}
```

- **render** branches on `mode`:
  - `ReadOnly` → each event as a plain row, **no regions, no affordances** (Display behavior).
    A `cancelled` event renders struck-through/marked but inert.
  - `Editable` → each event row subdivides into a per-event region `evt:<uid>` (minted
    programmatically, the `HighlightableText` `tok-<i>` pattern) with a cancel-mark affordance
    (a small box, reusing the checkbox glyph idiom).
- **decode** branches on `mode`:
  - `ReadOnly` → `vec![]`.
  - `Editable` → a mark in `evt:<uid>`'s region → `(self.on_cancel)(uid)`, one `Msg` per
    cancelled event.
- **Message carry:** `CalendarView` is reusable, so it carries the message *value* to emit (Elm
  value-message via `on_cancel: fn(&str) -> M`), never a stored closure — matching `Checkbox`.

So one component type spans the Display↔Control range by mode: ReadOnly behaves as Display,
Editable behaves as Control. That *is* the demonstration the doc's three-modes table describes.

### Read-only feed connector — `inkapp-ics`

The doc's easy archetype ("pull, cache, done"). A new crate:

- `refresh()` loads the `.ics` source and parses it with the **`ical` crate** into
  `Vec<EventRow>` (`cancelled: false`), storing into an `RwLock` cache. Single-flighted via
  core's existing `SingleFlight`. All parsing/IO lives here.
- `flush()` is a **no-op** (read-only).
- App-facing sync `events() -> Vec<EventRow>` reads the warm cache under the read lock.
- No write queue, no retry, no `failed_writes()` of substance.

`Connector::name()` → `"ics"`.

### Writable connector — `inkapp-localcal`

A CalDAV-shaped stand-in (local-file/in-memory storage) so the Editable axis is driven by real
write capability. A new crate that reuses the **deferred-write pattern from Spec #6**:

- App-facing sync `events() -> Vec<EventRow>` (warm cache, read lock).
- App-facing sync `cancel(uid: &str)` **enqueues** the edit and applies an **optimistic overlay**
  so `events()` reflects `cancelled: true` *this same render*.
- `refresh()` loads events from the local store into the cache (single-flighted). A fixture seeds
  the store for tests/the app.
- `flush()` drains the queued cancels, persisting them to the local store. Local writes don't fail
  over a network, so retry/`failed_writes()` machinery is present (for trait/shape uniformity) but
  stays empty in practice — this connector deliberately exercises the *write-enqueue + flush +
  optimistic-overlay* path without the transient-failure drama.

`Connector::name()` → `"localcal"`.

(Naming: a crate is the honest home — it's a reusable, if local, calendar connector. It stands in
for CalDAV; the spec is explicit that real CalDAV transport is future.)

### `agenda` — new minimal app (`apps/agenda`)

Wires **both** connectors in one typed `Connectors` struct with a one-line `impl ConnectorSet`,
exactly like reading-queue:

```rust
pub struct Connectors { pub feed: Arc<IcsConnector>, pub cal: Arc<LocalCalConnector> }

impl ConnectorSet for Connectors {
    fn connectors(&self) -> Vec<Arc<dyn Connector>> { vec![self.feed.clone(), self.cal.clone()] }
}
```

`view` renders **two `CalendarView`s in one document**, choosing each mode from the backing
connector's capability:

- the ICS feed → `CalendarView { events: cx.feed.events(), mode: ReadOnly, .. }`
  (capability says read-only),
- the local calendar → `CalendarView { events: cx.cal.events(), mode: Editable,
  on_cancel: |uid| Msg::EventCancelled { uid } }` (capability says writable).

`update` routes `Msg::EventCancelled { uid }` → `cx.cal.cancel(&uid)`. `main`/`serve` are async
(`#[tokio::main]`), mirroring reading-queue. This is the doc's "policy, not just capability"
shown literally: the same component, two modes, mode decided by `view` from what the connector can
do.

### Module rename: `widgets/` → `components/`

The doc's vocabulary is "component"; the live `view` flow is `Vec<Box<dyn Component>>`. Rename the
`inkapp-core` `widgets/` module to `components/` (and its `mod` references, imports across core,
tests, and apps). `CalendarView` is authored directly under `components/`. The `Widget` *trait*
and `widget.rs` (holding `RenderCx`, `region_metadata`, `is_valid_region_name`) are **not**
renamed — that two-layer question is a tracked follow-up. (If `widget.rs`'s shared helpers feel
mis-homed after the module rename, moving them is allowed as incidental tidy, but the trait keeps
its name.)

## Ripple

- **`inkapp-core`**: new `Mode` enum; new `EventRow` type; new `CalendarView` component;
  `widgets/` → `components/` module rename (touches `lib.rs`, every widget file's `mod`/`use`,
  core tests). No `Component`/`Widget` trait signature changes.
- **New crate `inkapp-ics`**: `IcsConnector` implementing `Connector`; `ical` dependency.
- **New crate `inkapp-localcal`**: `LocalCalConnector` implementing `Connector` with the
  deferred-write/overlay pattern.
- **New app `apps/agenda`**: `Connectors` + `impl ConnectorSet`, `update`/`view`, async
  `main`/`serve`.
- **Workspace `Cargo.toml`**: add the two crates + the app as members; **`Cargo.lock`**: `ical`
  and transitive deps.
- **`inkapp-harness`**: new agenda real-ink e2e.
- **`docs/appdx.md`**: see below.

## appdx.md reconciliation (part of "make the doc true")

- Flip the status banner: **M is now built** (S, E, C, M done; only **T** ahead). Update the
  build-order line accordingly.
- **"Three interaction modes"** + **"Components never talk to connectors"**: rewrite from
  aspirational to real — the `Mode` enum, `CalendarView` spanning ReadOnly↔Editable, `view`
  choosing mode by connector capability, and the same mode reaching both `render` and `decode`.
- **Connectors**: note the **read-only ICS feed** archetype is now built (was only described),
  alongside the writable local calendar; mark real CalDAV still future (`inkapp-localcal` stands
  in).
- **Ergonomics check**: replace "`mode` wasn't needed here" with the agenda example where the
  axis earns its keep (two `CalendarView`s, modes chosen by capability).
- Note the **`components`** terminology now matches between doc and code; record the `Widget`-trait
  two-layer consolidation as a remaining tidy in the open-questions parking lot.

## Testing (all deterministic, no network)

1. **Mode flows to render** — `CalendarView` in `ReadOnly` emits no region metadata / no
   affordance; in `Editable` emits one `evt:<uid>` region + affordance per event. Both compile
   through Typst.
2. **Mode flows to decode** — identical synthetic ink over an event row: `ReadOnly` decodes
   `[]`; `Editable` decodes `[EventCancelled { uid }]`.
3. **ICS parse/refresh** — a fixture `.ics` → expected `EventRow`s in the cache; a second
   concurrent `refresh` single-flights to one parse.
4. **localcal deferred write** — `cancel(uid)` is optimistically visible via `events()` the same
   render; `flush()` persists; a fresh `refresh()`/reload still shows it cancelled.
5. **agenda e2e (harness)** — real-ink cancel mark on the Editable calendar → `EventCancelled` →
   `localcal` updated → re-render shows the event cancelled; ink on the ReadOnly calendar changes
   nothing. Exercises the async refresh/flush loop around the sync core with two connectors.
6. **Module rename green** — the existing core/app/harness suites stay green after
   `widgets/` → `components/` (rename is behavior-preserving).

## Self-review notes

- No placeholders; all eight resolved decisions are reflected in the architecture.
- Scope is one implementation plan's worth: one shared enum + type, one component, two connector
  crates (one trivial read-only, one reusing the established write pattern), one small app, one
  mechanical module rename, doc edits.
- The largest mechanical cost is the `widgets/` → `components/` rename (broad but shallow); it is
  sequenced **first** so the new `CalendarView` lands in its final home and later tasks don't
  churn paths.
- The `Mode` axis intentionally adds **no framework machinery** — it is a field convention — so
  the risk surface is the two new connectors and the app wiring, both of which follow Spec #6's
  established shapes.
