# inkapp — Spec #6: Connector plugin trait + async loop ("C")

**Date:** 2026-05-24
**Status:** Approved (design); plan pending

## Context

`docs/appdx.md` records a build order for making the doc true:
**S** secrets → **E** encryption *(both done, Spec #5)* → **C** connector plugin trait →
**M** mode axis → **T** Typst authoring. This spec is **C**.

Today there is **no `Connector` trait at all.** `Connectors` is an app-defined concrete
struct passed as the generic `Cx` parameter through `Framework<M, Msg, Cx>`; `update`/`view`
call concrete typed methods (`cx.readwise.archive(...)`), and the framework treats `Cx` as
fully opaque — it never touches connectors itself. The appdx "Connectors" section promises
much more: a plugin trait, `Arc<dyn Connector>` shared across a user's apps with a shared
cache, **deferred writes** (record durably, return, flush with retry, surface persistent
failures on the next render), per-connector caches with interior mutability, and **single-flight**
refresh.

Dan's decision: make the **full** Connectors section true in this slice, with the I/O model
committed to **async/tokio** (the appdx had left this explicitly undecided).

### What this spec makes true

- The `Connector` trait + `Arc<dyn Connector>` plugins.
- The async refresh/flush loop bracketing the sync `view`/`update` core.
- Deferred writes with retry and app-driven failure surfacing.
- Single-flight refresh + interior-mutability (`RwLock`) cache.
- Cross-app connector sharing (exercised by test, since only one app exists today).

### Explicitly out of scope

- **Demand-driven (document-dependency) refresh.** Dan's idea: documents declare their
  connector dependencies, and the framework refreshes only the union actually used. This is the
  elegant *end state* but a pure optimization — it only pays off with multiple connectors per app
  and a render that touches a subset, neither of which exists yet. Building it now is speculative
  machinery (YAGNI). It is captured as the documented next evolution; the chosen design does not
  preclude it (the set is already `Arc<dyn Connector>` and the loop already brackets `view` with
  refresh/flush, so adding `depends_on` later is a refinement, not a rewrite).
- **Background re-flush between cycles.** Flush runs inline per loop cycle (see §4); a
  self-timed background flusher is a later refinement.
- Event sourcing / CRDT, multi-device reconciliation, multi-user/cloud key management — stay in
  `FUTURE.md`.

### Position in the spec arc

- **Spec #1** — Typst-readback spike (merged).
- **Spec #2** — Deterministic harness (merged).
- **Spec #3** — E2E gesture-fixture layer (merged).
- **Spec #4** — The MVU app loop (merged).
- **Spec #5** — Secrets store + encryption (merged).
- **Spec #6 — Connector plugin trait + async loop (this doc).** Third increment of
  "make the doc true."

## Key decisions (resolved during brainstorming)

1. **Scope:** the full appdx Connectors section, not a thin seam.
2. **I/O model: async/tokio.** The app-facing methods (`cx.readwise.queue()`, `.archive()`)
   stay **sync** — reads hit a warm cache, writes only enqueue. The framework-facing trait
   methods (`refresh`, `flush`) are **async**, awaited by the loop *around* the sync
   `view`/`update`. So `step`/`run` become async and pull in tokio, but `view`/`update` stay
   sync and pure.
3. **Connector discovery: static `ConnectorSet`, refresh-all up front.** The app keeps its
   concrete `Connectors` struct and writes a one-line `impl ConnectorSet`. No proc-macro.
   Demand-driven refresh is the deferred evolution (above).
4. **Failure surfacing: app-driven.** The connector exposes `failed_writes()`; the app's
   `view` reads it and renders its own banner. The framework stays out of presentation.
5. **Flush timing: inline per cycle.** `flush()` runs its retry loop once per loop cycle,
   awaited by `step`; the permanent-fail threshold is counted across cycles.

## Architecture

### The `Connector` trait (framework-facing, async)

Minimal and `dyn`-compatible (via `async-trait`). It is *only* what the framework calls;
app-facing typed methods live on the concrete connector and stay sync.

```rust
#[async_trait]
pub trait Connector: Send + Sync {
    /// Stable name (e.g. "readwise") — diagnostics and creds lookup.
    fn name(&self) -> &str;

    /// Pull fresh data into the connector's own cache. All network lives here.
    /// Single-flight + cache policy are the connector's internals.
    async fn refresh(&self) -> Result<(), ConnectorError>;

    /// Drain the durable write queue, pushing each write out with retry.
    /// Persistent failures are recorded internally (read via the concrete
    /// connector's `failed_writes()`), not returned — surfacing is app-driven.
    async fn flush(&self);
}
```

`ConnectorError` is a new error type in `inkapp-core` (transport failure, etc.).

### `ConnectorSet` — how the framework enumerates connectors

```rust
pub trait ConnectorSet {
    fn connectors(&self) -> Vec<Arc<dyn Connector>>;
}
```

`Framework<M, Msg, Cx>` gains the bound `Cx: ConnectorSet`. The app's struct implements it
trivially:

```rust
pub struct Connectors { pub readwise: Arc<Readwise> }

impl ConnectorSet for Connectors {
    fn connectors(&self) -> Vec<Arc<dyn Connector>> { vec![self.readwise.clone()] }
}
```

### The async loop

`Framework::step` (and `run`) become `async`. Each cycle brackets the existing sync core
with concurrent refresh/flush:

```
refresh_all (join_all, await)
   → decode ink → fold update → view → render → reconcile   (existing sync core)
   → flush_all  (join_all, await)
```

- **refresh before the core** so `view`/`update` read only warm caches (resolves the
  async-read ordering problem; reinterprets the doc's "reads trigger refresh" as "the
  framework triggers refresh on behalf of the upcoming reads").
- **flush after the core** so the writes `update` enqueued this cycle get pushed; their
  remote effects appear at next cycle's refresh. Writes are already *locally* visible this
  render via the connector's optimistic overlay.

```rust
async fn refresh_all(&self) {
    let cs = self.connectors.connectors();
    futures::future::join_all(cs.iter().map(|c| c.refresh())).await;
}
async fn flush_all(&self) {
    let cs = self.connectors.connectors();
    futures::future::join_all(cs.iter().map(|c| c.flush())).await;
}
```

(Refresh errors are swallowed per-connector for now — a connector that can't refresh serves
its stale cache; this matches "recent is fine" semantics. A failed *write* is the surfaced
case, not a failed read.)

### Deferred writes + retry (Readwise)

The write methods (`archive`, `add_highlight`) already record intent durably in the overlay
and return immediately, and make the write locally visible to `queue()`/`highlights()` this
same render (optimistic). This spec adds the *push* half:

- A pluggable **write transport** behind the connector: `trait WriteTransport` (async) with a
  **fake** (deterministic; can be told to fail K times then succeed, or fail permanently) and
  the **real** Readwise-API transport (still gated behind the existing manual `#[ignore]` live
  bar in `reading-queue`). This keeps retry/failure tests network-free and deterministic.
- `flush()` drains the queued writes through the transport. A write that fails transiently is
  retried on the next `flush()` (next cycle), its attempt count incremented; after **N
  attempts** it moves to a permanently-failed list.
- `failed_writes() -> Vec<FailedWrite>` exposes the permanently-failed list for `view`.

### Single-flight + concurrency (reusable helper)

The doc calls single-flight "the real value-add," so it lives once in `inkapp-core`, not
per-connector:

- `SingleFlight` — collapses concurrent `refresh()` calls into **one** underlying execution;
  all awaiters share the (cloned) result. Generic over a `Result<(), E>` where `E: Clone`.
- The connector cache moves behind `RwLock` (concurrent reads; the write lock is taken only
  briefly to store an already-fetched result — **never held across an `await`**, per the doc's
  rule). Reads (`queue()`, etc.) take the read lock.

### Cross-app sharing

Connector fields are `Arc<Readwise>`, so two `Framework` instances built from `Connectors`
holding clones of the *same* `Arc` share the cache, the write queue, and the single-flight
guard. Exercised by a test (only one app exists, so sharing can't be shown app-to-app
otherwise): app A archives → app B's `view` sees it archived; a concurrent refresh from both
collapses to a single underlying fetch.

### App-driven failure banner (reading-queue)

`reading-queue::view` reads `cx.readwise.failed_writes()` and, when non-empty, prepends a
banner component ("couldn't sync N items to Readwise") to the document set. The framework
contributes nothing to this.

## Ripple

- **`inkapp-core`** gains deps: `tokio`, `async-trait`, `futures`. New: `Connector` trait,
  `ConnectorSet` trait, `ConnectorError`, `SingleFlight`. `Framework::step`/`run` become async.
- **`inkapp-readwise`** implements `Connector`; adds the write-transport seam, `RwLock` cache,
  `SingleFlight`, retry/`failed_writes`.
- **`reading-queue`** — `Connectors` holds `Arc<Readwise>` + implements `ConnectorSet`;
  `main`/`serve`/tests become async (`#[tokio::main]`, `#[tokio::test]`); `view` renders the
  failure banner.
- **`inkapp-harness`** — the keystone real-ink e2e becomes async; still green.
- **`docs/appdx.md`** — see below.

## appdx.md reconciliation (part of "make the doc true")

- Flip the status banner: **C** is now built (S, E, C done; M, T ahead).
- Rewrite the **Connectors** section to the real shape: `Connector` trait (async
  `refresh`/`flush`), `ConnectorSet` enumeration, `Arc<dyn Connector>` sharing, deferred-write
  flush-with-retry, the `SingleFlight` helper, app-driven `failed_writes()` surfacing.
- **"Assembling & running"**: reconcile the aspirational `.connector(Readwise::new(token))`
  single-call form with reality — show the whole-struct `.connector(Connectors { .. })` plus
  the one-line `impl ConnectorSet`; note the single-call sugar as possible future ergonomics.
- Flip the I/O-model line ("std vs tokio… **Not decided yet**") to **decided: async/tokio**,
  with the app-sync / framework-async split.
- Add **document-dependency demand-driven refresh** to the open-questions parking lot as the
  next evolution of connector refresh.

## Testing (all deterministic, no network)

1. **SingleFlight** — two concurrent calls collapse to one underlying execution; both get the
   result.
2. **`Connector::refresh`** — populates the cache; a second concurrent refresh single-flights.
3. **Deferred write + retry** — fake transport fails K times then succeeds → write eventually
   drained; fails permanently → appears in `failed_writes()` after N attempts.
4. **Cross-app shared cache** — two `Framework`s over one `Arc<Readwise>`; a write through A is
   visible in B's `view`.
5. **App-driven banner** — permanent write failure → `view` output contains the banner.
6. **Async loop e2e** — the existing harness keystone, now async, still green end to end.

## Self-review notes

- No placeholders; all five resolved decisions are reflected in the architecture.
- Scope is one implementation plan's worth: one trait pair, one helper, one connector
  migration, one app migration, doc edits.
- The async ripple (tokio across core/app/harness) is the largest mechanical cost and is called
  out explicitly so the plan sequences it (core async first, then connector, then app/harness).
