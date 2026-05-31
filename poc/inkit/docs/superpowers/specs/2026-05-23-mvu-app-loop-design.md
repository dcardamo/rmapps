# inkapp — Spec #4: The MVU App Loop (reading-queue keystone)

**Date:** 2026-05-23
**Status:** Approved (design); plan pending

## Context

Specs #1–#3 built the **bottom half** of `appdx.md`'s own diagram and proved it under
`make test` with no hardware: a faithful `.rm` reader/writer (`rm-files`), a device-agnostic
render + manifest + ink-attribution core with two widgets (`inkapp-core`), a calibrated
reMarkable transform (`inkapp-remarkable`), and a single-cycle loop simulator plus a real-ink
gesture-fixture tier (`inkapp-harness`). Today a test calls `Checkbox::read_state(...)`
**directly** — it proves *decode*, the ink → region → reading path.

What does **not** exist is the entire subject of `appdx.md`: the **MVU app-authoring
surface**. There is no `Model`, no `Msg`, no `update(Msg, &mut Model, &Connectors)` fold, no
`view(&Model, &Connectors) -> Documents`, no `Component` (render + **decode→Msg**), no
`Connectors`, and no `inkapp::app(...).update().view().run()` assembly. The project's stated
bet — *"a passing test exercises the same decode→update path a device would, without the
device"* — is **half-proven**: only the decode side of that path is built.

This spec makes the worked example **real**. It stands up the upper half as the smallest
coherent vertical slice that exercises **decode → update → view across multiple cycles**, with
a connector, and runs both in the automated harness and **round-trip on a real reMarkable** so
the app is hand-usable.

### Position in the four-spec arc

- **Spec #1 — Typst-readback spike** (merged). Render + region recovery feasibility.
- **Spec #2 — The deterministic harness** (merged). Unit tier; synthetic ink; two widgets.
- **Spec #3 — E2E gesture-fixture layer** (merged). Real-ink tier; fidelity bars.
- **Spec #4 — The MVU app loop (this doc).** The app-authoring surface; the reading-queue
  keystone; the first multi-cycle loop; first on-device app round-trip.

The originally-sketched "Spec #4 = the AI step" is **deferred**: AI is an input source that
sits *on top of* the loop, so the loop is the prerequisite. AI becomes a later spec.

### Decisions carried in from brainstorming

- **Vehicle: the reading queue with a cassette-backed connector.** Exactly `appdx`'s worked
  example (ArticleBody + Checkbox, multi-document view), so `appdx`'s own test snippets
  compile. The connector serves **real Readwise data captured once** (via the operator's
  rmreader credentials) and **committed** as fixtures — real *and* deterministic, no per-run
  network.
- **On-device round-trip is a goal, not just automated tests.** A manual `#[ignore]` bar
  pushes the rendered queue to a real reMarkable, the operator inks by hand, the bar pulls,
  folds, re-renders, and pushes again — the same loop the automated harness drives.
- **Short content; pagination deferred.** Articles are constrained to fit one page. Multi-page
  render + cross-page region stitching is its own later spec.
- **Decode binding: re-derive from a small embedded snapshot, decode before fold.** The
  document embeds only `{manifest (regions+rects), key, version}`; on readback the framework
  re-runs pure `view` *before* this cycle's Msgs are folded to reproduce the exact rendered
  trees, matches by key + minted region name, and verifies the version marker. No serialized
  trees, no stored closures. Correct against the base version by construction.
- **Connector machinery: appdx shape, in-memory delivery.** `update` returns nothing; writes
  are recorded and applied to a working overlay so the next `view` reflects them. No real
  write-back, retry, single-flight, `Arc`-concurrency, or caching policy (all deferred /
  open-question machinery).
- **Simple MVU only.** `update` mutates and calls connectors inline (the `appdx` "Adoption,
  not tax" basic path). No event-sourcing / merge — that is opt-in for multi-device/collab.

## Goals (this spec)

Stand up the app surface and prove the loop end to end:

1. A `Component` trait (`render → Typst`, `decode → Vec<Msg>`) that the Spec #2/#3 widgets
   implement, carrying **value-messages** (no closures).
2. `Model` / `Msg` / `update` / `view` / `Connectors` types and an `inkapp::app(...)` builder.
3. The framework **render walk** (mint region names from the component tree) and **decode
   walk** (re-derive trees, attribute ink, fold Msgs).
4. Keyed `view -> Documents` **reconciliation** (create / update / delete; ink preserved by
   key) and a **multi-cycle** loop driver (`step`).
5. A **cassette-backed Readwise connector** (committed real data + working overlay) and a
   `#[ignore]` refresh bar.
6. The **reading-queue app** assembled via `inkapp::app(...)`, with `appdx`'s two test snippets
   passing.
7. An **automated ≥2-cycle e2e** harness test (simulator + real-ink fixtures, inspector
   goldens) and a **manual `#[ignore]` on-device round-trip** bar.

Everything except the two `#[ignore]` bars (cassette refresh; on-device round-trip) is
provable under `make test`.

## Non-goals (deferred)

- **Real Readwise HTTP write-back.** The cassette connector records/applies writes locally;
  it never mutates the operator's Readwise account. The real bidirectional HTTP connector is a
  later spec.
- **Pagination & cross-page region stitching.** Single-page documents only.
- **Multi-device** (one logical doc × N devices) and **multi-user** tenancy.
- **Event-sourcing / CRDT merge.** Inline `update` only.
- **Connector delivery machinery** — durable write-queue, retry, single-flight, `Arc<dyn>`
  concurrency, caching/TTL policy, failure-as-next-render banner.
- **The full secrets/config store.** The refresh bar reads the Readwise token from an env var
  / known path; the per-user key store stays as in Spec #2.
- **The AI step** (handwriting recognition, vision input). Banked for a later spec.

## A. Crate layout

```
crates/
  inkapp-core/         # gains the app surface (below); widgets -> components
  inkapp-remarkable/   # unchanged
  inkapp-harness/      # gains the multi-cycle driver + e2e loop test
  rm-files/            # unchanged
  inkapp/              # NEW facade: re-exports core + remarkable as `inkapp::*`
  inkapp-readwise/     # NEW: cassette-backed Readwise connector + refresh bar
apps/
  reading-queue/       # NEW: the vehicle app (Model/update/view/ArticleBody)
```

New `inkapp-core` modules:

```
inkapp-core/src/
  component.rs   # the Component trait (render + decode); RenderCx already exists
  components/    # Checkbox, HighlightableText evolve to impl Component
  connector.rs   # Connector trait + the typed Connectors set + Connectors::fake()
  document.rs    # Document, Documents, DocKey
  reconcile.rs   # keyed diff: Create | Update | Delete (ink-preserving)
  runtime.rs     # App builder + step() (one loop cycle) + run() (forever wrapper)
```

The **`inkapp`** facade exists only so app code reads as in `appdx` (`inkapp::app`,
`inkapp::Component`, `inkapp::Checkbox`, …). It pulls in `inkapp-remarkable` as the default
device. Apps depend on `inkapp` (+ connector crates), never on `inkapp-core` directly.

## B. The `Component` trait (evolving `Widget`)

Spec #2's `Widget` is `render(&mut RenderCx) -> String` + `read(&[RegionInk], &Manifest) ->
Output`. This spec renames it to `Component` and replaces `read` with `decode`:

```rust
pub trait Component {
    type Msg;
    fn render(&self, cx: &mut RenderCx) -> String;            // Typst (unchanged shape)
    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<Self::Msg>;
}
```

- **`Checkbox<M>`** carries `on_check: M` (value-message). `decode` reuses the existing
  `read_state`: `Marked` → `vec![on_check.clone()]`; `Empty` → `vec![]`. (`ScribbledOut` is
  available for an app that wants an un-check Msg; the reading queue treats any non-`Empty`
  mark as archive, so a single `on_check` suffices here.)
- **`HighlightableText`** stays the reusable span-reader; its existing span-read powers the
  app's bespoke `ArticleBody::decode`, which builds `Msg::Highlighted { article, text }`
  directly (the `appdx` "app-specific content component" path — no stored closure).
- Region names are minted positionally via the existing `RenderCx::fresh_id()` /
  `region_metadata`, so authors never name regions (auto-regions, `appdx` §Components).

`read`/`read_state` remain as public helpers (handy in unit tests and inside `decode`); only
the *trait surface* moves from `read` to `decode`.

**Signature note.** `appdx` sketches `decode(&self, ink: Ink)` and is explicit that its
snippets are *illustrative, not a committed surface*. The committed signature keeps the
Spec #2 shape — `decode(&self, ink: &[RegionInk], manifest: &Manifest)` — because a
subdivided component (ArticleBody's `tok-N`) owns multiple regions and needs the manifest
rects for containment. The two `appdx` test snippets are reproduced **adapted to this
signature**, not byte-for-byte.

## C. Documents, keys, reconciliation

```rust
pub struct DocKey(String);              // app-stable identity (e.g. article id)
pub struct Document { key: DocKey, flow: Vec<Box<dyn ComponentDyn<Msg>>> }
pub struct Documents(Vec<Document>);    // the complete set view returns

pub fn view(m: &Model, cx: &Connectors) -> Documents;
```

`view` returns the **complete set** of documents that should exist; the framework diffs it
against the prior set **by key** (`appdx`: "React, for a folder of PDFs"):

```rust
enum DocOp { Create(DocKey), Update(DocKey), Delete(DocKey) }
fn reconcile(prev: &DocSet, next: &Documents) -> Vec<DocOp>;
```

- **Create** — key new this cycle → render + materialize (in-memory doc, or `rmapi put`).
- **Update** — key present, content changed → re-render the PDF background **but keep the
  existing ink layer** (ink preserved by key; on device this means replacing the page render,
  not the `.rm` strokes).
- **Delete** — key vanished (article archived) → remove the document.

A `DocSet` (the framework's registry of `key → {device handle, last manifest, version}`) is
held in memory by the harness driver and persisted by the device bar.

## D. The render walk and the decode walk

**Render walk (view → PDF + manifest).** The driver walks each `Document`'s flow, calling
`component.render(&mut RenderCx)`; each component emits Typst with `<region>` metadata at
minted names (`r0`, `r1`, … / subdivided `tok-N`). Compile via `compile_to_document`, recover
regions via `recover_regions` (giving a `Manifest` whose `version` field — already in the type
— is set to this document's version marker), embed the encrypted manifest, export PDF.

**Decode walk (ink → Msgs), and why it is correct.** On readback the driver, **before folding
any of this cycle's Msgs**, re-runs `view(&model, &connectors)` to regenerate the identical
component trees, then for each readback document:

1. Match it to a regenerated `Document` by **`DocKey`**.
2. Recover the embedded `{manifest, version}` and assert `version` matches the regenerated
   document's version (staleness guard — impossible to fail single-user; the cheap seed of
   `appdx`'s per-(user×device) vector clock).
3. `attribute(strokes, manifest)` → per-region `RegionInk`; hand each component its ink and
   collect `component.decode(...)` → `Vec<Msg>`.

This is correct *against the base version the ink was written on* because, single-user, the
connector + model state at the **top of `step()`** equals the state the last render used
(decode precedes this cycle's writes). Reproducibility across a **process restart** (the
device bar) holds because the read cassette is immutable and the working overlay is persisted —
re-running pure `view` reproduces the same trees. Two invariants this rests on, stated and
test-pinned: **`view` is deterministic** given its reads, and **region minting is positional /
deterministic**.

## E. The loop driver (`runtime`)

```rust
pub struct App<M> { model: M, update: UpdateFn<M>, view: ViewFn<M>, connectors: Connectors }

impl<M> App<M> {
    // one cycle; the unit the harness drives and run() repeats
    pub fn step(&mut self, device: &dyn Device, prior: &mut DocSet, ink: InkByDoc) -> StepReport;
    pub fn run(self, device: impl Device) -> !;   // forever: sync-pull -> step -> sync-push
}

pub fn app<M>(model: M) -> AppBuilder<M>;  // .connector(..).update(..).view(..).run()
```

`step()`: decode walk (§D) → fold each Msg through `update(msg, &mut model, &connectors)` →
render walk on the new `view` → `reconcile` against `prior` → apply `DocOp`s. `run()` is the
thin forever-wrapper the **device bar** uses; the **automated harness calls `step()`
directly** with simulator/fixture ink, so no real loop or transport is baked into the
framework (preserving Spec #2's "transport out of the framework" boundary).

## F. The cassette-backed Readwise connector (`inkapp-readwise`)

```rust
pub struct Readwise { cassette: Cassette, overlay: Overlay }   // implements Connector
impl Readwise {
    pub fn queue(&self) -> Vec<Article>;                 // cassette minus archived (overlay)
    pub fn add_highlight(&self, a: ArticleId, text: &str); // recorded -> overlay (returns ())
    pub fn archive(&self, a: ArticleId);                   // recorded -> overlay (returns ())
}
```

- **Cassette** — committed JSON captured once from real Readwise: a handful of real articles
  (id, title, short body, existing highlights), under
  `crates/inkapp-readwise/fixtures/cassette/`. Real *and* deterministic.
- **Overlay** — applied writes (archived set, added highlights). It makes the loop's behavior
  real: archive → the article leaves `queue()` next cycle (a `Delete`); highlight → it renders
  into the body next cycle. **In-memory** for automated tests (deterministic, ephemeral);
  **persisted to a gitignored local file** for the device bar so hand-use survives restarts.
- **Shape matches `appdx`** — writes recorded (assertable via `archived()` / `highlights()`),
  `update` returns nothing, reads hit the local cassette. No network, retry, single-flight, or
  `Arc` concurrency.
- **`Connectors` is a concrete app-defined struct for this slice** — `struct Connectors {
  readwise: Readwise }` — so `cx.readwise` resolves by a plain field, not framework codegen.
  The generic typed-set machinery that lets `.connector(X)` synthesize `cx.x` is deferred;
  with one connector a hand-written struct is honest and unblocks the loop.
- **`Connectors::fake()`** for unit tests returns a `Connectors` whose `Readwise` is backed by
  a tiny inline cassette so `appdx`'s snippets run without the committed fixture.
- **Refresh bar** (`#[ignore]`, `tests/refresh.rs`): read the Readwise token from
  `READWISE_TOKEN` (or rmreader's config path), fetch a few articles + highlights, write the
  committed cassette. Documented run command; the only credentialed step.

## G. The reading-queue app (`apps/reading-queue`)

`Model`, `Msg`, `update`, `view`, and the bespoke `ArticleBody` exactly as in `appdx`'s worked
example, wired with:

```rust
fn main() {
    inkapp::app(App)
        .connector(Readwise::from_cassette())   // or ::live() behind the refresh bar
        .update(update)
        .view(view)
        .run();
}
```

`view` returns one keyed `Document` per `cx.readwise.queue()` article
(`[ArticleBody, Checkbox{on_check: Archived}]`). This is the buildable, on-device-runnable
artifact.

## H. Testing tiers

- **Unit (`make test`).**
  - `appdx`'s two snippets (adapted to the committed signatures, §B):
    `archiving_pushes_to_readwise` (update → connector records archive) and
    `ink_on_the_box_decodes_to_archive` (`Checkbox::decode`).
  - Component decode: `Checkbox` (Marked/Empty/ScribbledOut → Msgs), `ArticleBody`
    (highlighted spans → `Highlighted` Msgs).
  - `reconcile`: create / update / delete by key; ink preserved on `Update`.
  - Determinism guards: `view` re-run yields identical region names + manifest; version
    marker round-trips and the staleness guard fires on a forced mismatch.
- **E2E (`make test`, harness).** A `≥2-cycle` loop on the cassette: cycle 1 renders the
  queue; the simulator applies a real-ink `highlight-swipe` fixture over a token region of one
  article and a `checkmark`/`Tap` over another's Archive box; `step` folds; cycle 2 re-renders
  and asserts — the archived article's document is **Deleted**, the highlight is **rendered
  into the body**, and a third untouched article's **ink is preserved**. Runs through the real
  `write_ink → read_ink → attribute` path with committed inspector goldens.
- **Manual `#[ignore]` bars (documented run commands).**
  - **On-device round-trip** (`apps/reading-queue` or `inkapp-harness/tests/device.rs`):
    `run()`-style — push the queue via `rmapi`, ink by hand, pull, `step`, re-render, push.
    The operator's real hand-use, honoring the `rmapi` v4/token/`mkdir` gotchas
    (`remarkable-pdf-mechanics.md §10`).
  - **Cassette refresh** (§F).

## I. Automation boundary

- **Automated under `make test`:** the full app surface, both `appdx` snippets, component
  decode, reconciliation, determinism guards, and the ≥2-cycle real-ink e2e with goldens.
- **Manual `#[ignore]`:** cassette refresh (credentialed fetch) and the on-device round-trip
  (real `rmapi` transport). Mirrors Spec #3's `#[ignore]` pattern; nothing in the framework
  runtime depends on them.

## Done when

- `Component` trait exists; `Checkbox` and `HighlightableText` implement it with
  value-messages; `read`/`read_state` retained as helpers.
- `Model`/`Msg`/`update`/`view`/`Connectors`/`Document(s)`/`DocKey` exist; `inkapp::app(...)`
  builder assembles them; the `inkapp` facade re-exports the `appdx` surface.
- The render walk mints positional regions; the decode walk re-derives trees, attributes ink,
  and folds Msgs; the version-marker staleness guard is in place.
- `reconcile` does keyed create/update/delete with ink preserved on update; `step()` runs a
  full multi-cycle loop; `run()` wraps it for the device bar.
- `inkapp-readwise` serves a committed real-data cassette with a persisted working overlay and
  records writes; `Connectors::fake()` works inline; the refresh bar is documented.
- `apps/reading-queue` builds and runs via `inkapp::app(...)`.
- Both `appdx` snippets, component-decode, reconciliation, and determinism unit tests pass;
  the ≥2-cycle real-ink e2e passes with committed goldens; the on-device round-trip bar is
  documented and runnable.
- `make test` and `make clippy` are green.

## Risks

- **Render-walk region minting must be exactly reproducible.** If `view` re-run mints
  different names, decode mis-binds. Mitigated by positional minting + a determinism unit test
  that diffs two `view` runs' manifests; the version-marker guard catches drift loudly.
- **Determinism of `view`.** Re-derivation assumes `view` is a pure function of its reads. The
  cassette is immutable and the overlay carried, so this holds single-user; stated as an
  invariant and pinned by test. Multi-device/event-sourcing (future) replaces re-derivation
  with the embedded base-state log.
- **Ink-preserving `Update` on device.** Replacing a page's PDF while keeping its `.rm` ink is
  the subtle device operation; proven by the on-device bar and an in-memory reconcile test.
  If `rmapi` makes in-place page replacement impractical, fall back to delete+recreate with
  ink re-attached (documented as a finding).
- **`dyn` over `Component` with an associated `Msg`.** A `Document`'s flow holds heterogeneous
  components sharing one app `Msg`; needs an object-safe `ComponentDyn<Msg>` shim (decode →
  `Vec<Msg>`, render → `String`). Standard erase-the-associated-type pattern; called out so
  the plan budgets for it.
- **Cassette realism vs. size.** A handful of real articles, short bodies (pagination
  deferred). Representative, not exhaustive; refreshable via the bar.
```