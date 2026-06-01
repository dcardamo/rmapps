# Cut reMarkable cloud request volume — design

Date: 2026-06-01
Status: approved (brainstorm), pending implementation plan

## Problem

We hit `429 Too Many Requests` from the reMarkable cloud. A prior session
attributed it mostly to a one-time digest backlog and to manual
testing/verification churn, and claimed future digest runs would be cheap
because they "skip unchanged books." Reading the code disproves that claim:

1. **Digest re-downloads the entire library on every run.**
   `rmdigest::generate::process_doc` calls `backend.fetch(doc)` — the full
   bundle download — *unconditionally*, before any skip check
   (`crates/rmdigest/src/generate.rs:79-99`). The `if ing.changed.is_empty()`
   skip only avoids *regeneration + re-upload*; it runs *after* the download and
   needs the downloaded bytes to compute page hashes. The `cloud_version` field
   that was meant to be the cheap skip is dead: `CloudBackend::list` sets
   `version: None` (`apps/rmapps/src/cloud_adapters.rs:26`) and `process_doc`
   never reads `doc.version`. It is written to state after upload and never read
   back. So a digest run on an unchanged library still pulls every book's full
   bundle. This is the recurring steady-state cost, not a one-time backlog.

2. **`ls` fan-out is O(folders × total_docs).** The cloud has no server-side
   child listing, so `Client::ls(folder)` fetches the `.metadata` of *every doc
   in the account* and filters by parent (`crates/rm-cloud/src/porcelain/fs.rs:56`).
   `Client::snapshot()` is **not cached** (`crates/rm-cloud/src/client.rs:109`),
   and `list_recursive` → `walk` calls `ls` **once per folder, recursively**
   (`apps/rmapps/src/cloud.rs:216`), each call re-snapshotting and re-fetching
   all account metadata. `mkdir_p` repeats the same fan-out per path segment
   (`crates/rm-cloud/src/porcelain/fs.rs:101`).

3. **Nothing reuses immutable data** across calls or across CLI invocations.

The existing reactive 429 handling (`crates/rm-cloud/src/transport.rs`,
`send_retrying` with `Retry-After` + exponential backoff) stays — it is the
correct floor. This work attacks the *volume* so the floor is rarely reached.

## Key fact this design rests on

Every content-addressed read in the crate flows through one chokepoint,
`Client::get_blob(hash, name)` (`crates/rm-cloud/src/client.rs:160`): root index
(`client.rs:122`), per-doc index, `.metadata`, and page/file blobs
(`crates/rm-cloud/src/porcelain/document.rs:33-71`). The blob URL *is* the hash
and the returned bytes sha256 to it, so **hash is a perfect, immutable cache
key** and the `name` argument is just a server-side logical label, irrelevant to
cache identity. One cache seam at `get_blob` covers the whole crate.

## Scope

In scope: reduce request volume via a content-addressed disk cache, snapshot
generation-keying, and wiring the dead digest cheap-skip.

Out of scope (explicitly deselected during brainstorming):
- Proactive client-side rate governor (token bucket / global concurrency cap).
- An offline-FakeCloud workflow for manual diagnostics.
- Push/upload dedup guards.

Rationale: the volume reduction below should make 429s a non-issue without these.
They remain available as follow-ups if real-world 429s persist.

## Components

### Component 1 — Content-addressed disk blob cache (core)

A `BlobCache` wrapping `Client::get_blob` / `Client::put_blob`.

- **Backing:** a hand-rolled content-addressed store — **not** foyer. Foyer's
  value is hot/cold tiering and eviction; for immutable-by-hash blobs with
  abundant disk, a plain sharded directory is simpler with no failure mode foyer
  would prevent. (Foyer stays the fallback if a hard in-memory tier is ever
  wanted.)
- **Layout:** `<cache_dir>/blobs/<first-byte-hex>/<full-hash>`, sharded by the
  first byte of the hash. Writes are atomic (write temp file in the same dir,
  `fsync`, rename).
- **Read path (`get_blob`):**
  - Hit → read bytes from disk, verify `sha256(bytes) == hash`. On match, return
    without any network call. On mismatch (disk corruption), delete the entry and
    fall through to a miss.
  - Miss → fetch over the network (through `send_retrying` as today),
    write-through to disk, return.
- **Write-through on `put_blob`:** blobs uploaded during `commit` are written to
  the cache at upload time (we already hold the bytes and hash), so reads
  immediately after a commit are served locally.
- **Injection:** the rm-cloud `Client` takes an optional cache directory (the
  library stays path-agnostic and can run cacheless, e.g. in unit tests).
  `rmapps` wires the default `~/.cache/rmapps`.
- **Effect:** eliminates the `ls` / `walk` / `mkdir_p` metadata fan-out after the
  first warm-up (metadata + index blobs are paid once, until their hashes
  change), and shrinks a *changed* document's re-fetch to only the page/file
  blobs whose hashes actually changed (unchanged page blobs are cache hits).

### Component 2 — Snapshot generation-keying + snapshot-once walk

The snapshot (root index) is the only mutable thing; it changes whenever
anything syncs. The root ref carries a monotonic `generation`
(`Snapshot.generation`), and `Client::current_generation()`
(`crates/rm-cloud/src/client.rs:130`) already fetches just the root ref (no big
blob) to read it.

- **Memoize the snapshot in `Client`** behind the existing `RwLock`.
  `snapshot()` polls `current_generation()`; if the generation equals the cached
  snapshot's generation, return the cached snapshot; otherwise rebuild it (the
  root-index blob fetch is itself served by Component 1 when the hash is known).
  This is correct by construction: the generation bumps on every cloud mutation.
- **Snapshot-once walk:** `list_recursive` / `walk` currently re-snapshot inside
  every `ls`. Refactor `walk` to snapshot **once** at the top and thread
  `&Snapshot` down. Add an internal `ls_with(&snapshot, parent)` that does the
  metadata filtering against a given snapshot; the public `ls(parent)` keeps its
  signature (snapshots then delegates). This turns N generation-polls into 1 per
  recursive listing; combined with Component 1, the metadata reads are served
  from disk.

### Component 3 — Wire the dead digest cheap-skip

- Carry the doc hash outward: `rm_cloud` `Entry` (from `ls`) and `rmapps`
  `RemoteDoc` gain a `hash` field, populated from `snap.doc(id).hash` /
  `snap.docs()` — the value is already in the snapshot.
- `CloudBackend::list` sets `version: Some(doc_hash)` instead of `None`
  (`apps/rmapps/src/cloud_adapters.rs`).
- `process_doc` (`crates/rmdigest/src/generate.rs`): **before** `backend.fetch`,
  if `prev.cloud_version == doc.version` and `!prev.page_hashes.is_empty()`,
  return early — no download at all for unchanged documents.
- Keep persisting `prev.cloud_version = doc.version` after a successful upload
  (already present; now `doc.version` is actually populated).

This is an all-or-nothing skip at the document level. Component 1 covers the
*changed*-document case by fetching only the delta blobs.

### Component 4 — Cache management CLI

A `rmapps cache` subcommand group:
- `rmapps cache gc [--max-size N]` — size-based eviction (LRU by file mtime) down
  to a generous default cap (a few GB). Eviction runs only here, never in the hot
  read/write path.
- `rmapps cache info` — entry count and total size on disk.
- `rmapps cache clear` — wipe the store.

## Testing

TDD throughout, all against `FakeCloud` (`crates/rm-cloud/src/fake/`), zero
real-cloud calls in the suite:

- Cache hit serves bytes without a second network request (assert the fake's
  request count does not increase on the second read).
- A corrupted cache entry (bytes whose sha256 ≠ filename) is detected, discarded,
  and refetched.
- `put_blob` write-through: a blob read immediately after commit causes no GET.
- Generation unchanged → `snapshot()` does not re-download the root-index blob.
- `list_recursive` over a multi-folder tree issues one snapshot's worth of
  metadata reads, not folders × docs.
- Cheap-skip: `backend.fetch` is never invoked for a document whose
  `version == prev.cloud_version` (verify via a fetch-counting backend).
- Changed document: only the changed page/file blobs are fetched; unchanged page
  blobs are cache hits.
- `cache gc` evicts oldest entries down to `--max-size` and stops.

The repo's gated real-cloud tests (`apps/rmapps/tests/ws_live.rs`,
`watch_live.rs`) and any live `rmapps digest`/`sync` runs are **not** exercised
while the account is rate-limited; they resume only after the sliding window
clears.

## Sequencing note

Component 3 (cheap-skip) is the largest recurring win and is small and isolated.
The implementation plan should land it first, so the worst offender (full-library
re-download per digest run) is eliminated the moment the rate-limit window
resets — even before the cache layer lands.

## Risks / open questions

- **Cache key trust:** we trust the server's hash as the cache key and verify by
  re-hashing on read. If the reMarkable cloud ever served a blob whose bytes did
  not match the requested hash, we would cache it under the wrong key — but our
  read-time `sha256` verification would reject it on the next read, and the
  content-addressed model guarantees this never happens in practice.
- **Snapshot memoization staleness in long-running daemon:** mitigated by the
  per-`snapshot()` generation poll; the cost is one cheap root-ref GET per
  logical operation, which is acceptable and far below the current cost.
- **Disk growth:** bounded by `cache gc`; default cap generous given available
  disk. Page/file blobs dominate size; metadata/index blobs are negligible.
