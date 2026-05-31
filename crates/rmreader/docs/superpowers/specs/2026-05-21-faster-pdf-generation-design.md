# Faster PDF generation — design

Date: 2026-05-21
Status: Approved (pending spec review)

## Problem

A full cold run takes ~41s. Measured breakdown (release build, 58 Library docs +
100 Feed docs, `time -v` on a no-deploy config):

| Phase                              | Time     | Notes                                            |
|------------------------------------|----------|--------------------------------------------------|
| Image fetch + normalize            | ~31s     | ~18s Library + ~13s Feed; serialized across docs |
| fulgur render                      | ~7.5s    | 5.8s Library + 1.7s Feed                         |
| Readwise list fetch + postprocess  | ~2–3s    | Cheap                                            |
| **Total wall clock**               | **40.8s**| Peak RSS 543 MB                                  |

Image fetching is ~75% of the run and is currently serialized at the document
boundary: doc N+1's images do not start until doc N's slowest image resolves (up
to the 8s per-request timeout). The fulgur render — the only phase with "PDF" in
its name — is ~18%. The Readwise list fetch is negligible.

## Goals

- **Cold run** (empty cache): ~15s.
- **Warm / dev re-run** (no content changed): ~8s (render-bound).
- **Incremental production run** (a few new feed items): ~8s.
- **Invariant:** output PDFs are byte-identical to today's. Caching and
  parallelism change *speed only*, never bytes. Enforced by golden tests.

## Non-goals (this round)

Splitting the monolithic fulgur render into parallel per-article passes + lopdf
merge. That is the only path below ~8s re-runs, but it is high-risk against
bookmarks, the index cross-links, and the nav post-processor, all of which assume
a single render pass. Deferred.

## Approach

Three independent levers, all preserving output bytes:

1. **Per-document content cache** — skip image fetch + normalize for unchanged docs.
2. **Global concurrent image fetch** — fetch all cache-miss images in one pool.
3. **Parallel Library + Feed** — overlap the two independent pipelines.

Readwise docs are **always fetched fresh** (the list fetch is only ~2–3s, so
there is no staleness risk and no doc-level caching).

---

### 1. Per-document content cache (`src/cache.rs`, new module)

**What is cached:** the *processed output* of a document — sanitized HTML plus the
already-normalized image blobs (post-transcode). A hit therefore skips both the
network and the image decode/transcode.

**Cache key** = stable hash of:

```
doc_id + html_content + max_article_bytes + images_enabled + CACHE_FORMAT_VERSION
```

"New or changed doc" ≡ "no entry for this key." Including `doc_id` keeps each
entry self-contained (asset keys embed the id) and avoids cross-doc aliasing when
two docs share identical content. Bumping `CACHE_FORMAT_VERSION` invalidates the
whole cache when the on-disk format or processing pipeline changes.

**Hash:** inline FNV-1a 64-bit. No new dependency; stable across runs and machines
(unlike `std`'s `DefaultHasher`, whose output is not guaranteed stable). A hash
scheme change just causes misses + rebuild — never wrong output, only slower.

**On-disk layout** — one directory per entry:

```
<cache.dir>/<key>/
    meta.json     # format version, ordered list of (asset_key, ext), truncated flag
    html          # processed/sanitized HTML
    <asset_key>   # one file per normalized image blob
```

- **Hit:** read `meta.json` + `html` + blobs; then touch the entry via
  `File::open(meta.json)?.set_modified(SystemTime::now())` (touch-on-hit, see
  Expiry). This is a single metadata syscall — no content rewrite, no temp file,
  blobs untouched. (`set_modified` is stable in std since Rust 1.75; toolchain is
  1.94.) A failed touch (e.g. read-only FS) degrades gracefully: the entry merely
  ages as if unused.
- **Miss:** build the entry in `<cache.dir>/.tmp-<rand>/`, then atomically rename
  it to `<key>/`. Atomic rename means the two parallel collection threads (and any
  crash mid-write) never expose a partial entry.

**Expiry (touch-on-hit, last-used age):**

- **Tracking is purely filesystem mtime of each entry's `meta.json`** — no
  sidecar index, no recorded timestamp field. A single sweep runs **once per
  invocation**, at the start, over all of `cache.dir`: for each entry dir,
  `fs::metadata(meta.json)?.modified()` (a bare `stat`, no parse); if older than
  `cache.expiry_days`, `rm -rf` the entry dir. Relying on mtime is safe here —
  `noatime` mounts affect only atime; the only failure modes (copy/restore
  resetting timestamps) cause at most a spurious rebuild, never wrong output.
- Because a hit refreshes `meta.json`'s mtime, "expired" means "not used in N
  days" = genuinely orphaned (the doc rolled out of the feed/library). Current,
  unchanged docs always hit regardless of cron interval.
- No reference tracking, no refcounts. Orphans (including any leaked partial that
  somehow survived) are reclaimed purely by age.
- `cache.expiry_days` default **7**.

Rationale for touch-on-hit over pure write-age: pure write-age collapses the hit
rate to ~zero whenever the run interval is close to the expiry (e.g. weekly cron +
7-day expiry rebuilds nearly everything every run). Touch-on-hit avoids this while
keeping the orphan-reclaim behavior identical.

---

### 2. Global concurrent image fetch

Hoist image fetching out of the per-doc loop:

1. **Pass A — classify.** For each doc, compute its cache key. Hit → take the
   cached processed output. Miss → collect the doc's (post-truncation) `<img>`
   URLs.
2. **One concurrent fetch** over the deduplicated *union* of all miss-doc URLs,
   using the existing bounded thread-pool pattern (`std::thread::scope` +
   `AtomicUsize` cursor) from today's `UreqImageFetcher::fetch_many`, sized by
   `images.concurrency`. Wall time is bounded by the single slowest host, not the
   sum of per-doc tails.
3. **Pass B — assemble misses.** Each miss doc normalizes its images from the
   shared `url → bytes` map, sanitizes its HTML, builds its `(html, assets)`, and
   writes the cache entry.

**Refactors:**

- `content.rs`: split `process_html` into
  - `collect_doc_urls(html, max_bytes) -> (truncated_html, truncated, Vec<url>)`
    (truncate at UTF-8 boundary + dedup URLs, first-seen order), and
  - `assemble_processed(doc_id, truncated_html, truncated, url_to_bytes, images_enabled) -> Processed`
    (normalize from already-fetched bytes + sanitize). Normalization logic
    (`normalize_image`, SVG passthrough, tracking-pixel drop) is unchanged.
- `assemble.rs`: `assemble_document` stops taking a network closure and instead
  takes a precomputed `doc_id -> (html, assets)` map. This is *more* testable
  (pure data in, no closure). The index page and per-doc fragment rendering are
  otherwise unchanged.

---

### 3. Parallel Library + Feed

`generate()` runs both collections inside a `std::thread::scope`. Each thread:
fetches its own Readwise docs → runs the full cache→global-fetch→assemble→render→
postprocess→manifest pipeline. The two threads write disjoint files, so there is
no contention. After both join: run the expiry sweep once (or at start, before
spawning — see below) and return the deploy targets.

- The Readwise list rate limit (20 req/min) is not threatened: ~6 list requests
  for Library (3 locations) + ~2 for Feed, even when the two run concurrently.
- **Sweep ordering:** run the sweep *before* spawning the build threads, so it
  never races a concurrent entry write. Touch-on-hit during the builds then
  refreshes survivors for the next run.

**Send/Sync bounds.** The parallel path requires the fetcher and transport to be
`Sync`. The real `UreqImageFetcher` and `UreqTransport` are (`Sync`, no interior
mutability). The non-`Sync` test fakes (which use `RefCell`) keep working because
they exercise the lower-level functions (`assemble_document`, `assemble_processed`,
`fetch_documents`) directly, not the parallel `generate()`. Where a trait object
must cross the scope boundary, bound it as `&(dyn Trait + Sync)` at the
`generate()` seam only.

**Risk — fulgur thread-safety.** Two `Engine`s rendering concurrently is
unverified. fulgur wraps Blitz (layout) + krilla (PDF emit); both *should* be
independent per-instance, but global font/state is possible. Mitigation:

- Golden visual tests must remain byte-identical under the parallel path.
- Add a determinism test that runs the parallel build and compares bytes against a
  serial build.
- If fulgur has shared global state, fall back to a `Mutex` guarding only the
  `render_html_to_file` call — keeping fetch/process parallel and serializing just
  the render (still a clear win, since render is only ~18%).

Verify empirically during implementation before claiming the parallel render works.

---

### 4. Config additions

New `[cache]` section in `Config`:

```toml
[cache]
enabled = true        # default true
dir = "..."           # default: $XDG_CACHE_HOME/rmreader, else ~/.cache/rmreader
expiry_days = 7       # default 7
```

- Resolved with `std::env` (no `dirs` dependency): `XDG_CACHE_HOME` if set,
  else `HOME` + `/.cache`, then `/rmreader`.
- `enabled = false` bypasses both read and write (every doc treated as a miss, no
  entries written, no sweep) — useful for debugging and for the golden-test path.
- The cache dir is shared across config files by default; keys are content-
  addressed so sharing is benign (identical content → identical entry). Set a
  distinct `cache.dir` per config to isolate.

---

## Module change summary

| File                 | Change                                                                 |
|----------------------|------------------------------------------------------------------------|
| `src/cache.rs` (new) | Key hashing, entry read/write (atomic), touch-on-hit, expiry sweep.    |
| `src/content.rs`     | Split `process_html` → `collect_doc_urls` + `assemble_processed`.      |
| `src/assemble.rs`    | `assemble_document` takes a precomputed `doc_id -> (html, assets)` map.|
| `src/generate.rs`    | Orchestrate cache classify → global fetch → assemble; parallelize the two collections; sweep. |
| `src/config.rs`      | Add `CacheConfig` (`enabled`, `dir`, `expiry_days`) + defaults.        |
| `src/render.rs`      | Unchanged (possible `Mutex` fallback only if fulgur is not thread-safe).|

## Testing

- **Cache unit tests** (`tests/cache.rs`): put/get roundtrip; key changes when
  `html_content` or `max_article_bytes` changes; hit refreshes mtime (touch);
  expiry sweep removes entries older than `expiry_days` and keeps fresh ones;
  atomic write (no partial entry visible); `enabled = false` bypasses.
- **Global-fetch test:** dedup of repeated URLs; correctness independent of fetch
  completion order; results map correctly back to per-doc asset keys.
- **Determinism test:** parallel build produces byte-identical PDFs vs a serial
  build, regardless of which collection finishes first.
- **Cache-transparency test:** a cold run and an immediate warm run produce
  byte-identical PDFs; existing golden visual tests unchanged.
- `make test` (and `make fmt-check`, `make clippy`) green.

## Expected results

| Scenario                         | Before | After  |
|----------------------------------|--------|--------|
| Cold run (empty cache)           | ~41s   | ~15s   |
| Warm re-run (nothing changed)    | ~41s   | ~8s    |
| Incremental (few new feed items) | ~41s   | ~8s    |
