# inkapp — Spec #11: `readwise-reader` connector + durable cache primitive

**Date:** 2026-05-24
**Status:** Approved (design); plan pending

## Context

inkapp's spine is complete (Specs #1–#10). The framework is now ready for its
**first real proof-point app**: `reader`, a from-scratch reimplementation of
`~/git/rmreader` on inkapp. The overriding goal is not just the app — it is to
*validate and improve inkapp by building a real thing on it*, so the next app is
easier still.

`reader` is a program of work, not one spec. The big rocks are **pagination**
(inkapp core; built in a sibling worktree) and an **HTML→Typst content pipeline**
(a later worktree). This spec is the **data foundation**: a great connector that
talks to Readwise Reader for real, and the durable caching that makes it fast —
both prerequisites every later piece consumes.

### The problem being solved

The current `inkapp-readwise` connector is **cassette-backed**: `refresh()` copies
committed JSON into an in-memory `RwLock` cache, and writes go through a
`NoopTransport`. Its code comments mark exactly where live behavior belongs
(*"A live build would await the network inside this closure"*; *"a live build pushes
to the Readwise API"*). What is missing:

- **No live HTTP** for reads or writes.
- **No durable read cache.** Only the *write* overlay is persisted; on restart the
  article cache is empty until the next `refresh()`. For "fast, hundreds of articles
  with images," durable caching is the whole point, and it is absent.
- **A thin data model.** `Article` is `id/title/body/highlights` — far short of what
  reader (and rmreader) need (locations, source_url, author, category, html, …).

### What this spec makes true

- A live `inkapp-readwise-reader` connector: real HTTP reads (auth, paginated Reader
  list across Feed + Library locations) and real write-back (archive/move, delete,
  create-highlight).
- An expanded `Article` model carrying everything the later content layer needs.
- A **durable read cache** (warm restart, offline reads) for article data.
- A reusable **`inkapp-core::cache`** primitive (a `cacache` wrapper) the connector
  uses now and the image/content layer reuses later — with content-addressing so
  derived (per-device rendered) cache entries invalidate automatically when their
  original changes.
- The Readwise token stored in the existing `SecretStore`, not a config file.

### Explicitly out of scope (later worktrees)

- **Pagination** in inkapp core — concurrent sibling worktree.
- **HTML→Typst content pipeline** (sanitize, reflow) — later worktree.
- **Image fetch / normalize / render-cache** — later worktree. *This spec designs the
  cache primitive so images drop in without redesign* (content-addressed derived
  keys, `get_bytes`/`put_bytes`, `touch`/`sweep`), but fetches no images.
- **rmapi device push/pull transport** — later worktree.
- **The reader app UI** (Feed/Library documents, filing-row components) — later.

### Position in the spec arc

- **Specs #1–#10** — framework spine (foundation, harness, gestures, MVU loop,
  secrets+encryption, connector trait, mode axis, Typst authoring, state field,
  Widget/Component consolidation). All merged.
- **Spec #11 — `readwise-reader` connector + durable cache (this doc).** The first
  brick of the `reader` proof-point app; the data foundation.

## Key decisions (resolved during brainstorming)

1. **The connector owns its HTTP, not the framework.** This was always the
   `Connector` design (*"All network reads live here"* inside `refresh()`); an earlier
   loose phrasing of "inkapp does HTTP transport" was wrong. The framework only calls
   `refresh`/`flush`. **Escape hatch:** the `Connector` trait stays minimal, so any
   connector may bring its own HTTP/cache and ignore the batteries inkapp offers.
2. **Crate boundary (option A):** the connector owns *Readwise data*; the *content
   layer* (later) owns images + HTML→Typst + their cache. Different failure domains,
   independently testable. Mirrors rmreader's split of `readwise/` (API) vs `cache.rs`
   (processed content + images).
3. **A reusable cache primitive in inkapp-core (option C),** not a connector-private
   one — rmreader's cache pattern is generic enough that the framework should offer it
   so the next content-heavy app doesn't reinvent it.
4. **Cache backend: `foyer`** — an actively maintained (releases into 2026; used in
   production by RisingWave and Chroma) hybrid in-memory + disk cache with
   restart-durable disk storage and pluggable eviction. Chosen over `cacache`, which was
   ~18 months stale (last release 13.1.0, Nov 2024). foyer is a *cache* — entries may be
   evicted under capacity pressure — which is exactly the right semantics here: a lost
   entry is always recoverable by re-fetch (rmreader's best-effort philosophy). foyer is
   not content-addressed, so we compute a **sha256 integrity ourselves** (`sha2`) on put
   and return it; the later image layer keys derived per-device render entries on
   `(original_integrity, device, params)`, so a changed original → new integrity → new
   key → automatic miss → re-render.
5. **HTTP client: `reqwest` via `reqwest-middleware`** from day one — the Readwise
   reads gain nothing from cache middleware, but building the client through
   `reqwest-middleware` lets the image layer drop `http-cache-reqwest` in later at zero
   cost. **Images later use `http-cache-reqwest`** with its **`FoyerManager`** backend
   (conditional revalidation via `http-cache-semantics`) — the *same* cache engine as
   this primitive; an ETag/304 *is* the "did the original change?" check.
6. **Crate rename, done as a pure move first:** `inkapp-readwise` →
   `inkapp-readwise-reader` (connector `name()` → `"readwise-reader"`). Rename and keep
   green, *then* build out — so the rename never tangles with new code. Cassette mode is
   kept permanently as the test/dev backend.
7. **Two collections, not two PDFs:** the connector exposes `library()` and `feed()`
   (rmreader produced two PDF files; on inkapp these are two filtered views the app
   later renders into document sets).

## Design

### A. Crate rename & module wiring (mechanical, do first, keep green)

- `crates/inkapp-readwise` → `crates/inkapp-readwise-reader`; package
  `inkapp-readwise-reader`; connector `name()` → `"readwise-reader"`.
- Update dependents (the only ones found): workspace `Cargo.toml` member list;
  `apps/reading-queue` (`Cargo.toml` + `src/lib.rs` + `tests/{banner,shared,app}.rs`);
  `crates/inkapp-harness` (`Cargo.toml` + `tests/app_loop.rs`). `agenda` is unaffected.
- Add `pub mod cache;` + re-export(s) to `crates/inkapp-core/src/lib.rs`.
- Preserve `from_cassette()` / `fake()` / `ScriptedTransport` so existing app + harness
  tests pass across the rename and through build-out.

### B. `inkapp-core::cache` — durable keyed primitive

A thin wrapper over a `foyer` `HybridCache<String, Vec<u8>>`. Generic; knows nothing
about Readwise.

```rust
pub struct Cache { /* foyer HybridCache<String, Vec<u8>> */ }
pub struct Integrity(pub String); // sha256 of stored bytes; basis for derived keys

impl Cache {
    /// Open a hybrid (memory + disk) cache rooted at `dir`, bounded by the given
    /// in-memory and on-disk byte capacities. Disk contents survive restart.
    pub async fn open(dir: impl Into<PathBuf>, mem_bytes: usize, disk_bytes: usize)
        -> Result<Self>;

    // typed JSON (article sets now)
    pub async fn get_json<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>>;
    pub async fn put_json<T: Serialize>(&self, key: &str, v: &T) -> Result<Integrity>;

    // raw bytes (images later)
    pub async fn get_bytes(&self, key: &str) -> Result<Option<Vec<u8>>>;
    pub async fn put_bytes(&self, key: &str, b: &[u8]) -> Result<Integrity>;

    /// Stable derived key from parts — e.g. [original_integrity, device, params].
    pub fn derived_key(parts: &[&str]) -> String;
}
```

- foyer stores `String → Vec<u8>`: `put_json` serializes via `serde_json`, `put_bytes`
  stores raw bytes. Both compute and return the **sha256 integrity** (`sha2`) — the
  derived-key basis for the later image layer.
- foyer handles **eviction internally** (capacity-bounded) and **persists to disk across
  restarts**, giving warm-restart reads for free — no manual touch/expiry-sweep needed.
- It is a *cache*: a miss (cold or evicted) is normal and simply triggers a re-fetch.
  Errors fold into inkapp's existing `Error`/`Result`, and are **non-fatal for reads** —
  a missing/unreadable entry behaves as a miss (best-effort, like rmreader).

### C. `inkapp-readwise-reader` connector

**Data model** (expanded to carry everything the content layer will need):

```rust
pub enum Location { New, Later, Shortlist, Archive, Feed }

pub struct Article {
    pub id: ArticleId,
    pub url: String, pub source_url: String,
    pub title: String, pub author: String, pub site_name: String,
    pub category: String,            // article / email / pdf / tweet / ...
    pub location: Location,
    pub summary: String,
    pub image_url: Option<String>,
    pub word_count: Option<u32>,
    pub reading_time: Option<String>,
    pub published_date: Option<String>,
    pub saved_at: String,            // sort key
    pub html_content: Option<String>,
    pub highlights: Vec<String>,
}
```

**Read seam — `FetchTransport`** (mirrors the existing `WriteTransport`; tests never
hit the network; connectors keep the escape hatch):

```rust
#[async_trait] pub trait FetchTransport: Send + Sync {
    async fn list(&self, location: &str, cursor: Option<&str>)
        -> Result<Page, ConnectorError>;
}
pub struct Page { pub articles: Vec<Article>, pub next_cursor: Option<String> }
```

- **Live impl** (`HttpFetch`): `reqwest_middleware::ClientWithMiddleware` →
  `GET https://readwise.io/api/v3/list/?withHtmlContent=true&location=…&pageCursor=…&limit=50`,
  header `Authorization: Token <token>`; retry on 429/5xx with exponential backoff
  (rmreader's 5-try policy, honoring `Retry-After`). The Reader list endpoint is
  **GET** (verified against the live API + rmreader's own code; rate-limited ~20/min).
- **Cassette impl**: returns committed JSON. Unit tests inject canned pages.

**`refresh()`**: for each configured location, page through `FetchTransport`, dedupe by
id (first-seen wins), sort by `saved_at` desc, cap per collection; write the assembled
set to the durable `Cache` and update the in-memory warm cache (single-flighted, as
today). On construction, the durable cache is loaded into the warm cache so reads work
**cold/offline before the first refresh** — the warm-restart property.

**Write-back** — extend `Write` and provide a real `WriteTransport`:

```rust
pub enum Write {
    Move(ArticleId, Location),     // PATCH v3/update/{id}/ {location}
    Delete(ArticleId),             // DELETE v3/delete/{id}/
    Highlight(ArticleId, String),  // POST v2/highlights/ {text,title,author,source_url,category}
}
```

The `Highlight` push fills `title/author/source_url/category` from the cached `Article`
(Readwise matches by `source_url`). Optimistic overlay + retry + `failed_writes()` stay
as today.

**App-facing sync methods**: `library()`, `feed()`, `archive(id)`, `move_to(id, loc)`,
`delete(id)`, `add_highlight(id, text)`, `failed_writes()`.

**Token + config**: token from `SecretStore` (`Scope::ConnectorCred`, name
`"readwise-reader"`). `ReaderConfig { library_locations: Vec<Location>, library_max:
usize, feed_enabled: bool, feed_max: usize }` with rmreader defaults
(`[New, Later, Shortlist]`, 100; feed on, 100). Constructors: `live(secrets, cache_dir,
config)`, plus retained `from_cassette()` / `fake()`.

### D. Sync / authority semantics

- **Readwise is authoritative for the article list + content.** `refresh()` replaces
  the cached set wholesale.
- **The overlay is local intent.** Reads layer it over the cached set: `library()` /
  `feed()` hide locally archived/moved articles and merge in locally added highlights,
  so the UI reflects the user's action immediately, before `flush()` delivers it.
- **`refresh()` reconciles the overlay against new server truth.** After a successful
  refresh: drop an optimistic archived/moved entry once the server set already reflects
  it; drop an optimistic added-highlight once it appears in the server's `highlights`.
  This bounds overlay growth and avoids the stale-flicker bug (a delivered archive
  reappearing after refresh, then vanishing again). Pending/failed **write** queues are
  untouched by refresh — only `flush()` owns those.
- **Failure isolation:** if a `refresh()` fetch fails partway (one location errors),
  keep the prior warm cache rather than clobbering it with a partial/empty set, and
  return the error. The app keeps showing last-known-good data offline.

### E. Testing & validation

- **Unit (no network):** fake `FetchTransport` drives `refresh` (paging, dedupe, sort,
  cap); existing `ScriptedTransport` drives `flush` (retry, `failed_writes`).
  Reconciliation test: archive locally → refresh with the server reflecting it →
  overlay entry pruned.
- **Cache tests:** round-trip; **warm-restart** (put, drop `Cache`, reopen, get — proves
  durability); `sweep` expiry; integrity / `derived_key` stability.
- **Cassette mode** stays the backend for `inkapp-harness` + `reading-queue` tests, so
  the whole workspace stays green offline.
- **Live bar** — `#[ignore]` test `live_readwise_reader`: reads the token from
  `SecretStore`/env, fetches real Feed + Library, asserts non-empty. **Read-only by
  default** — it does not mutate the account; write-back is exercised only by an
  explicitly opt-in, clearly named test.
- **Proof binary** — `examples/pull.rs`: load token, `refresh()`, print Feed/Library
  counts + first N titles, then a **second pass with networking disabled** to show the
  warm cache serves the data offline. The concrete "great connector" artifact to run
  against a real account.

### F. Error handling

- Keep `ConnectorError::Transport(String)`; add `Auth` (401/invalid token → fail fast,
  surfaced clearly) and `RateLimited` (429 → drives backoff). Minimal, not a sprawling
  taxonomy.
- `refresh()` → `Result`; partial failure preserves last-known-good cache (§D).
- `flush()` keeps its contract: retry to `MAX_ATTEMPTS`, then move to `failed_writes()`
  for the app to surface (the `Notice` banner pattern reading-queue already uses).
- Cache errors are non-fatal for reads (miss → fetch); they surface only on writes that
  cannot complete.

## New dependencies

- `inkapp-core`: `foyer` (hybrid cache backend, `serde` feature) + `sha2` (content
  integrity).
- `inkapp-readwise-reader`: `reqwest`, `reqwest-middleware` (live HTTP). `http-cache-reqwest`
  (with its `FoyerManager` backend) is **not** added here — it lands with the image layer.

## Definition of done

- `cargo test --workspace` green (rename + new unit/cache tests; live bar `#[ignore]`d).
- `examples/pull.rs` demonstrates a live fetch and an offline warm-cache second pass
  against a real account.
- `docs/appdx.md` updated to record the live `readwise-reader` connector + the
  `inkapp-core::cache` primitive as built (the repo's definition-of-done convention).
