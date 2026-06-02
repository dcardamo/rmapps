# reMarkable cloud sync-index re-architecture — design

Date: 2026-06-02
Status: approved (brainstorm), pending implementation plan

Supersedes the listing/metadata half of
[2026-06-01-cloud-request-volume-design.md](2026-06-01-cloud-request-volume-design.md).
That work shipped a content-addressed blob cache, snapshot generation-keying, and the
digest cheap-skip. It reduced *steady-state* volume but left the structural defect below
untouched, so 429s returned on the first real run.

## Problem

`rmapps reader` (and any path-based porcelain) still trips `429 Too Many Requests`
almost immediately. The failure is always in `ls`, never in render. This morning's run
(session `670b4273`, 07:14) died with:

```
Error: ls: rate limited (429): retry budget exhausted
  at Cloud::doc_ids_in → Cloud::replace → reader::run
```

### Root cause: we conflate "what changed" with "where does it live"

The reMarkable cloud is a git-style content-addressed Merkle store
(`poc/inkit/docs/rm-cloud-protocol.md`):

- A **root ref** (`GET /sync/v4/root`) carries a root-index hash + a monotonic
  `generation`.
- The **root index** blob lists every doc as `<docHash>:0:<docId>:<numFiles>:<size>` —
  it carries the doc hash **but not the doc's `parent` or `visible_name`**.
- `parent` and `visible_name` live only inside each doc's `.metadata` content blob.

Change *detection* is therefore cheap and already implemented: `Snapshot::diff`
(`crates/rm-cloud/src/plumbing/snapshot.rs:67`) classifies added/changed/removed purely
by comparing doc hashes from the root index. This is exactly what the tablet does to sync
instantly — poll the root generation, diff hashes, pull only the deltas.

But our porcelain is **path-based**. `Client::ls(folder)` must return children by
`parent`/`name`, which the root index does not contain. So `ls_with`
(`crates/rm-cloud/src/porcelain/fs.rs:60`) brute-force reads the `.metadata` of **every
document in the account** and filters by `parent`. Each metadata read is **two** blob GETs
(per-doc index + `.metadata`, `crates/rm-cloud/src/porcelain/document.rs:60-73`):

```
cost(ls) = 2 × (total docs in account) network GETs, fired 16-wide (LS_CONCURRENCY)
```

And the `reader` deploy calls `ls` ~4 times — `ensure_folder`(`mkdir_p`) + `doc_ids_in`,
once each for Feed and Library (`apps/rmapps/src/cloud.rs:136,152,185`). With ~150 account
docs that is ~1200 GETs in a 16-wide burst — instant 429.

### Why the prior fix did not help

1. **The blob cache only helps a *warm* cache.** The first run after it shipped was cold,
   so `ls` paid the full 2×N fan-out anyway. The cache attacks steady-state volume, not the
   burst, and the burst is what trips the limiter.
2. **No client-side throttle.** The prior design explicitly deselected a rate governor
   (lines 55-61). Nothing spaces the 16-wide burst, so a cold cache — or any influx of new
   docs, or a `cache gc`/`clear` — instantly re-trips 429.
3. **Wrong abstraction.** The blob cache caches metadata *bytes* but keeps the O(N)
   iteration and does not model "what changed." We already have the Merkle diff and throw
   it away on every listing.

## The fix: mirror the tablet's persistent local sync state

The tablet keeps a durable local index of every doc's hash, name, and parent, and re-reads
metadata only for docs whose hash moved. We have no such state — every `ls` rebuilds the
whole tree from scratch. Add it.

### Component 1 — `SyncStore` (persistent local sync state)

A small, injectable, persisted index:

```
SyncStore {
  schema_version: u32,
  account_generation: i64,
  docs: { docId → { hash, parent, name, doc_type } }   // doc_type: Document | Collection
}
```

- **Location:** `~/.cache/rmapps/sync-index.json`. Atomic write (temp file in the same
  dir, `fsync`, rename). **Exempt from `rmapps cache gc`** — gc evicts only content blobs;
  the index is tiny and load-bearing.
- **Durability/trust:** fully reconstructible from the cloud, so a missing, unparseable, or
  schema-mismatched file is not an error — it triggers a cold rebuild.
- **Injection:** mirrors `BlobCache`. `Client` gains an optional `Arc<SyncStore>`; rm-cloud
  stays stateless for unit tests, and `rmapps` wires the default path (alongside the blob
  cache, at every auth-path `Client` construction — `cloud.rs`, `auth.rs`).
- **Concurrent access:** the `watch` daemon and a manual CLI may both touch the file.
  Writes are atomic-rename (last-writer-wins is safe — the value is rebuildable), and a
  torn/older read at worst forces a re-diff against the current generation, which is
  correct by construction.

### Component 2 — `Client::resolved_snapshot()` (the new listing heart)

Replaces the brute-force account scan. Algorithm:

1. `GET /sync/v4/root` → `generation`. **One request, always.**
2. **`generation == store.account_generation`** → the store is authoritative for this
   generation. Return a resolved view built entirely from the store. **Total: 1 request,
   zero metadata fetches.**
3. **Generation changed (or cold store)** → fetch the root-index blob (served by the blob
   cache when its hash is known) → `Snapshot`. Diff the snapshot's doc hashes against the
   store: `added` + `changed` need metadata, `removed` are dropped, unchanged reuse the
   store's `(parent, name, doc_type)`. Fetch `.metadata` only for `added + changed`
   (O(delta), typically 1–3). Write the updated store back and persist.

A "resolved view" is the existing `Snapshot` (hash-level doc set) paired with the store's
`(parent, name, doc_type)` per id — enough for every path operation.

Porcelain rewires onto this:

- `ls_with` consults the resolved view instead of fetching metadata per doc.
- `ls`, `mkdir_p`, `resolve_folder`, `doc_id_in`, `doc_ids_in` all resolve against one
  `resolved_snapshot()` call rather than issuing an independent account scan each.
- The in-memory snapshot generation-memo (Component 2 of the prior design) stays and
  composes: within one process the root-index blob is fetched at most once per generation.

Effect: a warm `ls` goes from **2·N requests → 1**. The reader deploy's four `ls` calls
collapse to one root poll plus the changed-doc delta.

### Component 3 — Transport rate governor

A process-global throttle wrapping `send_retrying` in
`crates/rm-cloud/src/transport.rs`, protecting the one unavoidable cold O(N) scan:

- **Concurrency cap:** a shared async semaphore, default **4** permits (replacing the
  effectively-unbounded 16-wide metadata burst).
- **Minimum inter-request spacing:** a shared gate enforcing a minimum interval between
  request *starts*, default **150 ms** (~6–7 req/s ceiling).
- Both env-overridable: `RM_CLOUD_MAX_CONCURRENCY`, `RM_CLOUD_MIN_INTERVAL_MS`.
- The existing `Retry-After` + exponential backoff (`send_retrying`) stays as the reactive
  floor underneath the governor.

The governor is shared across all requests from a process (the `LS_CONCURRENCY` semaphore
in `fs.rs` becomes redundant once the governor caps concurrency globally; it is removed to
avoid two overlapping limits). Effect: even a cold scan drips under the cloud's limit
instead of bursting — 429 becomes structurally unreachable in steady state and survivable
(just slower) on a cold start.

### Component 4 — Blob cache demotion

The content-addressed `BlobCache` stays, scoped to **content** blobs (PDF/ink/`.content`/
`.pagedata`) that serve the digest cheap-skip and changed-doc refetch. It is no longer on
the listing critical path. Its existing per-doc-index/`.metadata` caching becomes redundant
with `SyncStore` but is harmless and need not be ripped out.

## Scope

In scope: `SyncStore`, `resolved_snapshot()` and the porcelain rewire onto it, the
transport rate governor, removal of the now-redundant `LS_CONCURRENCY` semaphore.

Out of scope (deferred):
- Multi-account keying of the SyncStore (one account → one file for now; a forthcoming
  config system can key by account id later).
- Resumable/checkpointed cold scan (the governor already makes a cold scan safe; partial
  persistence is a nicety, not required).
- Removing the blob cache's redundant metadata caching.

## Defaults chosen (override during planning if wrong)

- Rate governor: concurrency **4**, min interval **150 ms**.
- SyncStore: single-account, JSON at `~/.cache/rmapps/sync-index.json`.

## Testing

TDD throughout, all against `FakeCloud` (`crates/rm-cloud/src/fake/`), zero real-cloud
calls in the suite. The fake already counts per-hash blob GETs; extend it to count root-ref
GETs where needed.

- **Unchanged generation:** `resolved_snapshot()` issues exactly one request (root ref) and
  zero metadata GETs; the store is returned verbatim.
- **Single changed doc:** only that doc's `.metadata` (+ its doc-index) is fetched; every
  other doc is a store hit. Assert via the fake's per-hash GET counter.
- **Cold store:** first call performs the full scan and persists; an immediately following
  call issues one request.
- **Removed doc:** disappears from the resolved view without any fetch.
- **Corrupt / missing / wrong-schema index file:** clean cold rebuild, no surfaced error.
- **Governor — concurrency:** with the cap at N, in-flight requests never exceed N
  (instrument the fake to record max concurrency).
- **Governor — spacing:** request starts are separated by at least the min interval
  (drive time with a controllable clock / record start timestamps).
- **Deploy `replace` of Feed + Library:** one root poll + the changed-doc delta, not four
  account scans (assert request count).
- **Atomic persistence:** a write interrupted before rename leaves the previous index
  intact (simulate by writing to temp then asserting the live file is unchanged until
  rename).

The repo's gated real-cloud tests (`apps/rmapps/tests/ws_live.rs`, `watch_live.rs`) and live
`rmapps reader`/`digest`/`sync` runs are not exercised while the account is rate-limited;
they resume once the sliding window clears.

## Sequencing note

The rate governor (Component 3) is small, isolated, and independently unblocks today's
429 even before the SyncStore lands — the implementation plan should land it first. The
SyncStore + `resolved_snapshot()` (Components 1–2) are the dramatic volume cut and land
second; the porcelain rewire and blob-cache demotion follow.

## Risks / open questions

- **Store/generation skew across devices:** another device committing bumps the generation;
  our per-call root poll catches it and re-diffs. The store is never trusted past a
  generation it was not built at.
- **Governor too aggressive / too loose:** the real cloud limit is unknown. Defaults are
  conservative; both are env-tunable, and the reactive `Retry-After` floor remains. If real
  runs still 429, lower concurrency / raise interval before anything else.
- **Two processes racing the index file:** atomic-rename writes plus rebuildable content
  make last-writer-wins safe; the worst case is a redundant re-diff, never corruption.
