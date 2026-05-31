# readwise-reader connector + durable cache — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the data foundation for the `reader` app — a live `inkapp-readwise-reader` connector (real HTTP reads + write-back) backed by a reusable, durable `inkapp-core::cache` primitive.

**Architecture:** A generic `Cache` (thin wrapper over a `foyer` hybrid memory+disk cache, sha256 integrity via `sha2`) lives in `inkapp-core`. The renamed `inkapp-readwise-reader` connector keeps its existing shape (sync app-facing methods, async `refresh`/`flush`, optimistic overlay, single-flight, `WriteTransport`) and grows: a richer `Article` model, a `FetchTransport` read seam, a live `reqwest-middleware` HTTP impl for both read and write, an optional durable `Cache`, and a `SecretStore`-sourced token. Cassette mode is retained for offline tests.

**Tech Stack:** Rust, tokio, `foyer` (hybrid cache), `sha2`, `reqwest` + `reqwest-middleware`, `serde`/`serde_json`, `async-trait`. Spec: `docs/superpowers/specs/2026-05-24-readwise-reader-connector-and-cache-design.md`.

**Reference — the live Readwise Reader API (from rmreader):**
- Auth header: `Authorization: Token <token>`.
- List: `GET https://readwise.io/api/v3/list/?withHtmlContent=true&location=<loc>&pageCursor=<cursor>&limit=50` → `{ nextPageCursor, results: [doc] }`. Locations: `new`, `later`, `shortlist`, `archive`, `feed`. (The list endpoint is GET, rate-limited ~20/min — verified against the live API and rmreader's code.)
- Move: `PATCH https://readwise.io/api/v3/update/<id>/` body `{ "location": "new"|"later"|"archive" }`.
- Delete: `DELETE https://readwise.io/api/v3/delete/<id>/`.
- Create highlight: `POST https://readwise.io/api/v2/highlights/` body `{ "highlights": [{ text, title, author, source_url, category }] }`.
- Retry on 429/5xx up to 5 times with backoff (honor `Retry-After`).

---

### Task 0: Pure crate rename — `inkapp-readwise` → `inkapp-readwise-reader`

**Goal:** Rename the crate with zero behavior change; whole workspace stays green.

**Files:**
- Rename dir: `crates/inkapp-readwise/` → `crates/inkapp-readwise-reader/`
- Modify: `crates/inkapp-readwise-reader/Cargo.toml` (package name + description)
- Modify: `Cargo.toml` (workspace member path)
- Modify: `crates/inkapp-readwise-reader/src/lib.rs` (connector `name()` string)
- Modify: `apps/reading-queue/Cargo.toml`, `apps/reading-queue/src/lib.rs`, `apps/reading-queue/tests/{banner.rs,shared.rs,app.rs}`
- Modify: `crates/inkapp-harness/Cargo.toml`, `crates/inkapp-harness/tests/app_loop.rs`

**Acceptance Criteria:**
- [ ] No file or symbol still references `inkapp_readwise` / `inkapp-readwise`.
- [ ] `cargo test --workspace` passes (same tests as before, all green).

**Verify:** `cargo test --workspace` → all pass; `grep -rn "inkapp[-_]readwise\b" . --include=*.rs --include=Cargo.toml` (excluding the new name) → no matches.

**Steps:**

- [ ] **Step 1: Move the crate directory (preserve history)**

```bash
cd <repo-root>
git mv crates/inkapp-readwise crates/inkapp-readwise-reader
```

- [ ] **Step 2: Rename the package**

In `crates/inkapp-readwise-reader/Cargo.toml`:
```toml
[package]
name = "inkapp-readwise-reader"
version = "0.1.0"
edition = "2021"
license = "MIT"
description = "Readwise Reader connector for inkapp: live sync + durable cache (cassette mode for tests)"
```

- [ ] **Step 3: Update the workspace member path**

In `Cargo.toml`, change the member `"crates/inkapp-readwise"` → `"crates/inkapp-readwise-reader"`.

- [ ] **Step 4: Update the connector name**

In `crates/inkapp-readwise-reader/src/lib.rs`, the `Connector::name` impl returns `"readwise"`. Change it to `"readwise-reader"` (no test asserts this string — verified by grep).

- [ ] **Step 5: Update all dependents**

Replace the crate name in dependents' `Cargo.toml` (`inkapp-readwise` → `inkapp-readwise-reader`) and the import path in Rust (`inkapp_readwise` → `inkapp_readwise_reader`):
```bash
# Cargo.toml dependency lines
sed -i 's/inkapp-readwise\b/inkapp-readwise-reader/g' apps/reading-queue/Cargo.toml crates/inkapp-harness/Cargo.toml
# Rust import paths
sed -i 's/inkapp_readwise\b/inkapp_readwise_reader/g' \
  apps/reading-queue/src/lib.rs \
  apps/reading-queue/tests/banner.rs apps/reading-queue/tests/shared.rs apps/reading-queue/tests/app.rs \
  crates/inkapp-harness/tests/app_loop.rs
```

- [ ] **Step 6: Verify green**

Run: `cargo test --workspace`
Expected: all tests pass (unchanged set). Then run the grep in **Verify** and confirm no stale references.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "inkapp-readwise → inkapp-readwise-reader (pure rename)"
```

---

### Task 1: `inkapp-core::cache` — durable cache primitive (foyer + sha2)

**Goal:** A generic, durable, bounded keyed cache with sha256 integrity, used by the connector now and the image layer later.

**Files:**
- Create: `crates/inkapp-core/src/cache.rs`
- Modify: `crates/inkapp-core/src/lib.rs` (add `pub mod cache;` + re-export)
- Modify: `crates/inkapp-core/src/error.rs` (add `Cache` variant)
- Modify: `crates/inkapp-core/Cargo.toml` (add `foyer`, `sha2`)

**Acceptance Criteria:**
- [ ] bytes and JSON round-trip; miss returns `Ok(None)`.
- [ ] identical content yields identical `Integrity`; different content differs.
- [ ] `derived_key` is stable for identical parts and distinct for different parts.
- [ ] values written then `close`d survive reopening the cache at the same dir (warm restart).

**Verify:** `cargo test -p inkapp-core cache::` → all pass.

**Steps:**

- [ ] **Step 1: Add dependencies**

In `crates/inkapp-core/Cargo.toml` under `[dependencies]`:
```toml
foyer = { version = "0.22", features = ["serde"] }
sha2 = "0.10"
```

- [ ] **Step 2: Add the error variant**

In `crates/inkapp-core/src/error.rs`, add to `enum Error`:
```rust
    #[error("cache failed: {0}")]
    Cache(String),
```

- [ ] **Step 3: Write the failing tests**

Create `crates/inkapp-core/src/cache.rs` with a `tests` module first:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    async fn open(dir: &std::path::Path) -> Cache {
        Cache::open(dir, 1 << 20, 8 << 20).await.unwrap()
    }

    #[tokio::test]
    async fn bytes_round_trip_and_miss() {
        let dir = tempfile::tempdir().unwrap();
        let c = open(dir.path()).await;
        assert!(c.get_bytes("k").await.unwrap().is_none());
        c.put_bytes("k", b"hello").await.unwrap();
        assert_eq!(c.get_bytes("k").await.unwrap().unwrap(), b"hello");
    }

    #[tokio::test]
    async fn json_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let c = open(dir.path()).await;
        let v = vec!["a".to_string(), "b".to_string()];
        c.put_json("j", &v).await.unwrap();
        let got: Vec<String> = c.get_json("j").await.unwrap().unwrap();
        assert_eq!(got, v);
    }

    #[tokio::test]
    async fn integrity_is_content_addressed() {
        let dir = tempfile::tempdir().unwrap();
        let c = open(dir.path()).await;
        let a = c.put_bytes("a", b"same").await.unwrap();
        let b = c.put_bytes("b", b"same").await.unwrap();
        let d = c.put_bytes("c", b"different").await.unwrap();
        assert_eq!(a, b);
        assert_ne!(a, d);
    }

    #[test]
    fn derived_key_stable_and_distinct() {
        assert_eq!(Cache::derived_key(&["i", "rm", "2x"]), Cache::derived_key(&["i", "rm", "2x"]));
        assert_ne!(Cache::derived_key(&["i", "rm", "2x"]), Cache::derived_key(&["i", "rm", "1x"]));
    }

    #[tokio::test]
    async fn survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        {
            let c = open(dir.path()).await;
            c.put_bytes("persist", b"v").await.unwrap();
            c.close().await.unwrap();
        }
        let c2 = open(dir.path()).await;
        assert_eq!(c2.get_bytes("persist").await.unwrap().unwrap(), b"v");
    }
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test -p inkapp-core cache::`
Expected: FAIL to compile (`Cache` undefined).

- [ ] **Step 5: Implement the primitive**

Prepend to `crates/inkapp-core/src/cache.rs` (above the tests module):
```rust
//! A durable, bounded cache primitive: a thin wrapper over a `foyer` hybrid
//! (in-memory + disk) cache. Generic over content; knows nothing about any
//! connector. Values are stored as bytes under string keys; the `*_json` helpers
//! (de)serialize via serde_json. Every put returns the sha256 `Integrity` of the
//! stored bytes — the basis for content-addressed derived keys (e.g. a per-device
//! rendered image keyed on its original's integrity, so a changed original misses).

use std::path::PathBuf;

use foyer::{BlockEngineConfig, DeviceBuilder, FsDeviceBuilder, HybridCache, HybridCacheBuilder};
use serde::{de::DeserializeOwned, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// The hex sha256 of a stored value's bytes. Stable for identical content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Integrity(pub String);

/// A durable, bounded keyed cache (memory + disk; disk survives restart).
pub struct Cache {
    inner: HybridCache<String, Vec<u8>>,
}

impl Cache {
    /// Open a hybrid cache rooted at `dir`, bounded by `mem_bytes` in memory and
    /// `disk_bytes` on disk.
    pub async fn open(dir: impl Into<PathBuf>, mem_bytes: usize, disk_bytes: usize) -> Result<Self> {
        let dir = dir.into();
        let device = FsDeviceBuilder::new(&dir)
            .with_capacity(disk_bytes)
            .build()
            .map_err(|e| Error::Cache(e.to_string()))?;
        let inner = HybridCacheBuilder::new()
            .memory(mem_bytes)
            .storage()
            .with_engine_config(BlockEngineConfig::new(device))
            .build()
            .await
            .map_err(|e| Error::Cache(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Flush in-memory entries to disk and shut down cleanly. Call on app exit;
    /// tests call it before reopening to assert durability.
    pub async fn close(&self) -> Result<()> {
        self.inner.close().await.map_err(|e| Error::Cache(e.to_string()))
    }

    /// Fetch raw bytes; `Ok(None)` on miss (cold or evicted).
    pub async fn get_bytes(&self, key: &str) -> Result<Option<Vec<u8>>> {
        match self.inner.get(&key.to_string()).await {
            Ok(Some(entry)) => Ok(Some(entry.value().clone())),
            Ok(None) => Ok(None),
            Err(e) => Err(Error::Cache(e.to_string())),
        }
    }

    /// Store raw bytes; returns their sha256 integrity.
    pub async fn put_bytes(&self, key: &str, bytes: &[u8]) -> Result<Integrity> {
        let integrity = Self::integrity(bytes);
        self.inner.insert(key.to_string(), bytes.to_vec());
        Ok(integrity)
    }

    /// Fetch and deserialize a JSON value; `Ok(None)` on miss.
    pub async fn get_json<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        match self.get_bytes(key).await? {
            None => Ok(None),
            Some(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|e| Error::Cache(e.to_string())),
        }
    }

    /// Serialize and store a JSON value; returns the integrity of the bytes.
    pub async fn put_json<T: Serialize>(&self, key: &str, value: &T) -> Result<Integrity> {
        let bytes = serde_json::to_vec(value).map_err(|e| Error::Cache(e.to_string()))?;
        self.put_bytes(key, &bytes).await
    }

    /// A stable, collision-resistant key derived from parts, e.g.
    /// `[original_integrity, device, params]`.
    pub fn derived_key(parts: &[&str]) -> String {
        let mut h = Sha256::new();
        for p in parts {
            h.update(p.as_bytes());
            h.update([0x1f]); // unit separator — unambiguous boundary
        }
        format!("{:x}", h.finalize())
    }

    fn integrity(bytes: &[u8]) -> Integrity {
        Integrity(format!("{:x}", Sha256::new().chain_update(bytes).finalize()))
    }
}
```

- [ ] **Step 6: Wire the module**

In `crates/inkapp-core/src/lib.rs`, add `pub mod cache;` (alphabetical, before `calendar`) and a re-export line:
```rust
pub use cache::{Cache, Integrity};
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p inkapp-core cache::`
Expected: PASS. (If foyer's builder method names differ in 0.22.x, consult `docs.rs/foyer` and adjust the three builder calls in `open`; the wrapper's public API stays as written.)

- [ ] **Step 8: Commit**

```bash
git add crates/inkapp-core
git commit -m "inkapp-core: add durable Cache primitive (foyer + sha2 integrity)"
```

---

### Task 2: Expand the `Article` model, `Location`, and `Write` variants

**Goal:** Carry everything `reader` needs, add Feed/Library views and move/delete writes — while the cassette and `reading-queue` keep working unchanged.

**Files:**
- Modify: `crates/inkapp-readwise-reader/src/lib.rs` (model, `Write`, accessors, `flush` match)
- Test: `crates/inkapp-readwise-reader/tests/model.rs` (new)

**Acceptance Criteria:**
- [ ] The committed cassette JSON (only `id/title/body/highlights`) still deserializes (new fields `#[serde(default)]`).
- [ ] `queue()` still returns all non-archived (back-compat for reading-queue).
- [ ] `library()` returns articles whose `location` ∈ configured library locations; `feed()` returns `Feed`-located articles.
- [ ] `move_to(id, loc)` / `delete(id)` enqueue the right `Write` and update the optimistic overlay.

**Verify:** `cargo test -p inkapp-readwise-reader model` and `cargo test --workspace` → all pass.

**Steps:**

- [ ] **Step 1: Write failing tests**

Create `crates/inkapp-readwise-reader/tests/model.rs`:
```rust
use inkapp_readwise_reader::{ArticleId, Location, Readwise};

#[test]
fn cassette_still_loads_with_defaults() {
    let rw = Readwise::from_cassette();
    let all = rw.queue();
    assert!(!all.is_empty());
    // Defaulted field present and parseable.
    assert!(all.iter().all(|a| !a.title.is_empty()));
}

#[test]
fn move_and_delete_enqueue_and_hide() {
    let rw = Readwise::fake();
    let id = rw.queue()[0].id.clone();
    rw.move_to(&id, Location::Archive);
    assert!(rw.queue().iter().all(|a| a.id != id), "archived/moved leaves the queue");
    let id2 = rw.queue()[0].id.clone();
    rw.delete(&id2);
    assert!(rw.queue().iter().all(|a| a.id != id2), "deleted leaves the queue");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p inkapp-readwise-reader model`
Expected: FAIL to compile (`Location`, `move_to`, `delete` undefined).

- [ ] **Step 3: Expand the model**

In `crates/inkapp-readwise-reader/src/lib.rs`, replace the `Article` struct and add `Location`. Keep `body` (plain text for the worked example / fallback) and `highlights`; add the rich fields with `#[serde(default)]` so old cassettes load:
```rust
/// Where an article sits in Readwise Reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Location { New, Later, Shortlist, Archive, Feed }

impl Default for Location {
    fn default() -> Self { Location::New }
}

impl Location {
    /// The Reader API location string.
    pub fn as_str(self) -> &'static str {
        match self {
            Location::New => "new",
            Location::Later => "later",
            Location::Shortlist => "shortlist",
            Location::Archive => "archive",
            Location::Feed => "feed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Article {
    pub id: ArticleId,
    pub title: String,
    /// Plain-text body — the worked-example/highlight source until the content
    /// pipeline lands. Rich source HTML rides in `html_content`.
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub highlights: Vec<String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub source_url: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub site_name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub location: Location,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub word_count: Option<u32>,
    #[serde(default)]
    pub reading_time: Option<String>,
    #[serde(default)]
    pub published_date: Option<String>,
    #[serde(default)]
    pub saved_at: String,
    #[serde(default)]
    pub html_content: Option<String>,
}
```

- [ ] **Step 4: Extend `Write` and the `flush` match**

Replace the `Write` enum:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Write {
    Move(ArticleId, Location),
    Delete(ArticleId),
    Highlight(ArticleId, String),
}
```
The existing `flush` pushes each `Write` through `transport.push(&p.write)` — no match on variants there, so it is unaffected. The `archive()` helper now enqueues `Write::Move(id, Location::Archive)` (see next step).

- [ ] **Step 5: Add accessors and write helpers**

Add a `ReaderConfig` (used by `library()`; full wiring in Task 5) and the methods. For now store config with a default:
```rust
/// Which locations make up the Library view, and per-collection caps.
#[derive(Debug, Clone)]
pub struct ReaderConfig {
    pub library_locations: Vec<Location>,
    pub library_max: usize,
    pub feed_enabled: bool,
    pub feed_max: usize,
}

impl Default for ReaderConfig {
    fn default() -> Self {
        Self {
            library_locations: vec![Location::New, Location::Later, Location::Shortlist],
            library_max: 100,
            feed_enabled: true,
            feed_max: 100,
        }
    }
}
```
Add a `config: ReaderConfig` field to `Readwise` (default it in the existing `build` constructor: `config: ReaderConfig::default()`). Then add methods on `impl Readwise`:
```rust
/// Articles in the configured Library locations (overlay applied), capped.
pub fn library(&self) -> Vec<Article> {
    let locs = &self.config.library_locations;
    let mut v: Vec<Article> = self.queue().into_iter()
        .filter(|a| locs.contains(&a.location))
        .collect();
    v.truncate(self.config.library_max);
    v
}

/// Feed articles (overlay applied), capped; empty if the feed is disabled.
pub fn feed(&self) -> Vec<Article> {
    if !self.config.feed_enabled { return Vec::new(); }
    let mut v: Vec<Article> = self.queue().into_iter()
        .filter(|a| a.location == Location::Feed)
        .collect();
    v.truncate(self.config.feed_max);
    v
}

/// Move an article to a new location (optimistic + enqueued).
pub fn move_to(&self, id: &ArticleId, loc: Location) {
    let mut ov = self.overlay.lock().unwrap();
    if !ov.archived.contains(id) {
        ov.archived.push(id.clone()); // overlay "removed from current view"
        ov.pending.push(PendingWrite { write: Write::Move(id.clone(), loc), attempts: 0 });
    }
    self.save(&ov);
}

/// Delete an article (optimistic + enqueued).
pub fn delete(&self, id: &ArticleId) {
    let mut ov = self.overlay.lock().unwrap();
    if !ov.archived.contains(id) {
        ov.archived.push(id.clone());
        ov.pending.push(PendingWrite { write: Write::Delete(id.clone()), attempts: 0 });
    }
    self.save(&ov);
}
```
Update the existing `archive()` to delegate: `self.move_to(id, Location::Archive)`.

> Note: the overlay's `archived: Vec<ArticleId>` is reused as the generic "hidden from view" set for move/delete/archive — names stay for serde back-compat with existing persisted overlays.

- [ ] **Step 6: Run tests**

Run: `cargo test -p inkapp-readwise-reader model` then `cargo test --workspace`
Expected: PASS. (`reading-queue` still uses `a.body`, `queue()`, `archive()` — all intact.)

- [ ] **Step 7: Commit**

```bash
git add crates/inkapp-readwise-reader
git commit -m "inkapp-readwise-reader: rich Article model, Location, library()/feed(), move/delete"
```

---

### Task 3: `FetchTransport` read seam + cache-backed `refresh()`

**Goal:** Make reads pluggable (like writes already are) and wire the durable `Cache` so reads are warm across restart; keep the network out of unit tests.

**Files:**
- Modify: `crates/inkapp-readwise-reader/src/lib.rs` (`FetchTransport`, `Page`, fake fetch, optional `Cache`, `refresh`)
- Modify: `crates/inkapp-readwise-reader/Cargo.toml` (depend on `inkapp-core` already present; add `tempfile` dev-dep is already present)
- Test: `crates/inkapp-readwise-reader/tests/refresh.rs` (extend existing or add cases)

**Acceptance Criteria:**
- [ ] A fake `FetchTransport` returning two cursor pages is fully paged, deduped by id, and sorted by `saved_at` descending.
- [ ] Per-collection cap is applied.
- [ ] A fetch error mid-refresh leaves the prior warm cache intact and returns `Err`.
- [ ] With a `Cache` attached, after `refresh()` the article set is in the durable cache; a fresh connector `with_cache` over the same dir serves it before any refresh (warm restart).
- [ ] Overlay reconciliation (spec §D): an optimistic "hidden" id is dropped once `refresh()` returns a server set that no longer contains it.

**Verify:** `cargo test -p inkapp-readwise-reader refresh` and `cargo test --workspace` → pass.

**Steps:**

- [ ] **Step 1: Write failing tests**

Add to `crates/inkapp-readwise-reader/tests/refresh.rs`:
```rust
use inkapp_readwise_reader::{Article, ArticleId, FetchTransport, Page, Readwise};
use inkapp_core::connector::{Connector, ConnectorError};
use std::sync::Arc;

struct TwoPages;
#[async_trait::async_trait]
impl FetchTransport for TwoPages {
    async fn list(&self, location: &str, cursor: Option<&str>) -> Result<Page, ConnectorError> {
        if location != "new" { return Ok(Page { articles: vec![], next_cursor: None }); }
        match cursor {
            None => Ok(Page {
                articles: vec![art("a", "2024-01-02")],
                next_cursor: Some("c2".into()),
            }),
            Some("c2") => Ok(Page {
                articles: vec![art("b", "2024-01-03"), art("a", "2024-01-02")], // dup a
                next_cursor: None,
            }),
            _ => Ok(Page { articles: vec![], next_cursor: None }),
        }
    }
}
fn art(id: &str, saved: &str) -> Article {
    Article { id: ArticleId::new(id), title: id.into(), saved_at: saved.into(), ..Default::default() }
}

#[tokio::test]
async fn pages_dedupes_and_sorts() {
    let rw = Readwise::fake()
        .with_fetch(Arc::new(TwoPages))
        .with_locations(vec!["new".into()]);
    rw.refresh().await.unwrap();
    let ids: Vec<String> = rw.queue().iter().map(|a| a.id.0.clone()).collect();
    assert_eq!(ids, vec!["b".to_string(), "a".to_string()]); // newest first, deduped
}
```
(For this test, derive `Default` for `Article` — add `#[derive(Default)]` alongside its other derives in Task 2's struct, and `#[derive(Default)]`-compatible fields; `ArticleId` and `Location` already default.)

> Adjust: in Task 2, add `Default` to the `Article` derive list and make `ArticleId` derive `Default` (empty string). Do that now if not already present.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p inkapp-readwise-reader refresh`
Expected: FAIL (`FetchTransport`, `with_fetch`, `with_locations` undefined).

- [ ] **Step 3: Define the read seam**

In `crates/inkapp-readwise-reader/src/lib.rs`:
```rust
/// One page of a Reader list response.
pub struct Page {
    pub articles: Vec<Article>,
    pub next_cursor: Option<String>,
}

/// The read seam: how the connector fetches a location's articles. Mirrors
/// `WriteTransport`. The default is a cassette fetch; a live build injects HTTP;
/// tests inject canned pages. (Connectors may bring their own — escape hatch.)
#[async_trait::async_trait]
pub trait FetchTransport: Send + Sync {
    async fn list(&self, location: &str, cursor: Option<&str>) -> Result<Page, ConnectorError>;
}

/// Cassette fetch: returns the committed source split into a single page per
/// location (matching `Article::location`).
struct CassetteFetch { source: Vec<Article> }
#[async_trait::async_trait]
impl FetchTransport for CassetteFetch {
    async fn list(&self, location: &str, _cursor: Option<&str>) -> Result<Page, ConnectorError> {
        let articles = self.source.iter().cloned()
            .filter(|a| a.location.as_str() == location)
            .collect();
        Ok(Page { articles, next_cursor: None })
    }
}
```

- [ ] **Step 4: Add fields, builders, and an optional cache**

Add to `struct Readwise`:
```rust
    fetch: Arc<dyn FetchTransport>,
    cache: Option<Arc<inkapp_core::cache::Cache>>,
    locations: Vec<String>, // which locations refresh() pulls
```
In `build`, default them:
```rust
    fetch: Arc::new(CassetteFetch { source: source.clone() }),
    cache: None,
    locations: vec!["new".into(), "later".into(), "shortlist".into(), "feed".into()],
```
Add builders:
```rust
#[must_use]
pub fn with_fetch(mut self, fetch: Arc<dyn FetchTransport>) -> Self { self.fetch = fetch; self }
#[must_use]
pub fn with_locations(mut self, locations: Vec<String>) -> Self { self.locations = locations; self }
#[must_use]
pub fn with_cache(mut self, cache: Arc<inkapp_core::cache::Cache>) -> Self { self.cache = Some(cache); self }
```

- [ ] **Step 5: Rewrite `refresh()` to page, assemble, and cache**

Replace the `refresh` impl body. Cache key constant: `const ARTICLES_KEY: &str = "articles/v1";`.
```rust
async fn refresh(&self) -> Result<(), ConnectorError> {
    let fetch = Arc::clone(&self.fetch);
    let locations = self.locations.clone();
    let cache = self.cache.clone();
    let warm = Arc::clone(&self.cache_articles); // see note below
    self.refresh_flight
        .run(move || async move {
            let mut all: Vec<Article> = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for loc in &locations {
                let mut cursor: Option<String> = None;
                loop {
                    let page = fetch.list(loc, cursor.as_deref()).await?;
                    for a in page.articles {
                        if seen.insert(a.id.0.clone()) { all.push(a); }
                    }
                    match page.next_cursor {
                        Some(c) => cursor = Some(c),
                        None => break,
                    }
                }
            }
            all.sort_by(|a, b| b.saved_at.cmp(&a.saved_at)); // newest first
            // Persist to durable cache (best-effort) and update the warm cache.
            if let Some(cache) = &cache {
                let _ = cache.put_json(ARTICLES_KEY, &all).await;
            }
            *warm.write().unwrap() = all;
            Ok(())
        })
        .await?;

    // Reconcile the optimistic overlay against new server truth (spec §D): keep a
    // "hidden" id only while it's still present server-side (i.e. the move/delete
    // hasn't been applied yet); drop added-highlights the server now reflects.
    {
        let warm = self.cache_articles.read().unwrap();
        let present: std::collections::HashSet<String> =
            warm.iter().map(|a| a.id.0.clone()).collect();
        let mut ov = self.overlay.lock().unwrap();
        ov.archived.retain(|id| present.contains(&id.0));
        ov.added.retain(|(id, text)| {
            warm.iter().find(|a| &a.id == id).map_or(true, |a| !a.highlights.contains(text))
        });
        self.save(&ov);
    }
    Ok(())
}
```
> Note: the existing field is named `cache: Arc<RwLock<Vec<Article>>>` (the warm read cache). Rename that in-memory field to `cache_articles` to avoid confusion with the new durable `Cache`, and update `queue()` and `build()` accordingly. The durable cache is the new optional `cache` field. The reconciliation block runs on `&self` *after* the single-flight closure (which is `'static` and can't borrow `self.overlay`); a fetch error returns via `?` before the warm set or overlay is touched, preserving last-known-good.

- [ ] **Step 6: Load the durable cache on construction**

Add a constructor that hydrates the warm cache from the durable cache so reads work before the first refresh:
```rust
/// Build over an existing durable cache, hydrating the warm cache from it.
pub async fn with_cache_hydrated(mut self, cache: Arc<inkapp_core::cache::Cache>) -> Self {
    if let Ok(Some(stored)) = cache.get_json::<Vec<Article>>(ARTICLES_KEY).await {
        *self.cache_articles.write().unwrap() = stored;
    }
    self.cache = Some(cache);
    self
}
```

- [ ] **Step 7: Add the warm-restart test**

Append to `tests/refresh.rs`:
```rust
#[tokio::test]
async fn warm_restart_serves_from_durable_cache() {
    let dir = tempfile::tempdir().unwrap();
    let cache = std::sync::Arc::new(
        inkapp_core::cache::Cache::open(dir.path(), 1 << 20, 8 << 20).await.unwrap());
    {
        let rw = Readwise::fake()
            .with_fetch(std::sync::Arc::new(TwoPages))
            .with_locations(vec!["new".into()])
            .with_cache(cache.clone());
        rw.refresh().await.unwrap();
        cache.close().await.unwrap();
    }
    let cache2 = std::sync::Arc::new(
        inkapp_core::cache::Cache::open(dir.path(), 1 << 20, 8 << 20).await.unwrap());
    let rw2 = Readwise::fake().with_cache_hydrated(cache2).await;
    let ids: Vec<String> = rw2.queue().iter().map(|a| a.id.0.clone()).collect();
    assert_eq!(ids, vec!["b".to_string(), "a".to_string()]);
}

#[tokio::test]
async fn refresh_prunes_applied_overlay_entry() {
    // Server first returns {a, b}; user archives `a` locally (hidden, pending).
    struct OnlyB;
    #[async_trait::async_trait]
    impl FetchTransport for OnlyB {
        async fn list(&self, location: &str, _c: Option<&str>) -> Result<Page, ConnectorError> {
            if location == "new" { Ok(Page { articles: vec![art("b", "2024-01-03")], next_cursor: None }) }
            else { Ok(Page { articles: vec![], next_cursor: None }) }
        }
    }
    let rw = Readwise::fake()
        .with_fetch(Arc::new(TwoPages))
        .with_locations(vec!["new".into()]);
    rw.refresh().await.unwrap();
    let a = ArticleId::new("a");
    rw.archive(&a);
    assert!(rw.queue().iter().all(|x| x.id != a), "a hidden by overlay");
    // Server now reflects the archive (a gone). After refresh, overlay entry is pruned.
    let rw = rw.with_fetch(Arc::new(OnlyB));
    rw.refresh().await.unwrap();
    assert!(rw.archived().iter().all(|id| id != &a), "applied overlay entry pruned");
}
```
(`with_fetch` consumes/returns `self`; in this test the connector is rebound. If `Readwise` is shared as `Arc` in real use, swapping transports isn't needed — the live transport is fixed. This test rebinds only to simulate changing server truth.)

- [ ] **Step 8: Run tests**

Run: `cargo test -p inkapp-readwise-reader refresh` then `cargo test --workspace`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/inkapp-readwise-reader
git commit -m "inkapp-readwise-reader: FetchTransport read seam + cache-backed refresh + warm restart"
```

---

### Task 4: Live HTTP — request building & response parsing (pure), behind `reqwest-middleware`

**Goal:** Implement the live Reader API read/write as pure, unit-tested request builders + JSON parsers, plus the thin `reqwest-middleware` impls that call them. Network itself is exercised by Task 6's live bar.

**Files:**
- Create: `crates/inkapp-readwise-reader/src/http.rs` (request URLs/params, response parsing, `HttpFetch`, `HttpWrite`)
- Modify: `crates/inkapp-readwise-reader/src/lib.rs` (`mod http;`, `ConnectorError` mapping helpers, re-exports)
- Modify: `crates/inkapp-core/src/connector.rs` (add `Auth`, `RateLimited` to `ConnectorError`)
- Modify: `crates/inkapp-readwise-reader/Cargo.toml` (add `reqwest`, `reqwest-middleware`)
- Test: `crates/inkapp-readwise-reader/tests/http.rs` (new — pure parse/build tests)

**Acceptance Criteria:**
- [ ] A sample Reader `v3/list` JSON body parses into the expected `Vec<Article>` + `next_cursor`, mapping all fields (location string → `Location`, html content, source_url, etc.).
- [ ] The list URL/query is built correctly for a location + cursor (`withHtmlContent=true&location=…&pageCursor=…&limit=50`).
- [ ] The highlight POST body serializes to the v2 shape `{ "highlights": [{text,title,author,source_url,category}] }`.
- [ ] 401 maps to `ConnectorError::Auth`, 429 to `RateLimited`.

**Verify:** `cargo test -p inkapp-readwise-reader http` → pass. `cargo test --workspace` → pass.

**Steps:**

- [ ] **Step 1: Add deps**

In `crates/inkapp-readwise-reader/Cargo.toml`:
```toml
reqwest = { version = "0.12", features = ["json"] }
reqwest-middleware = "0.4"
```

- [ ] **Step 2: Extend `ConnectorError`**

In `crates/inkapp-core/src/connector.rs`:
```rust
    #[error("connector auth failed: {0}")]
    Auth(String),
    #[error("connector rate limited")]
    RateLimited,
```

- [ ] **Step 3: Write failing pure tests**

Create `crates/inkapp-readwise-reader/tests/http.rs`:
```rust
use inkapp_readwise_reader::http::{build_list_url, highlight_body, parse_list, ListResponse};
use inkapp_readwise_reader::Location;

#[test]
fn list_url_has_expected_query() {
    let u = build_list_url("later", Some("CUR"));
    assert!(u.contains("location=later"), "{u}");
    assert!(u.contains("withHtmlContent=true"), "{u}");
    assert!(u.contains("pageCursor=CUR"), "{u}");
    assert!(u.contains("limit="), "{u}");
}

#[test]
fn parses_reader_list_json() {
    let raw = r#"{
      "nextPageCursor": "NEXT",
      "results": [{
        "id": "01", "url": "https://readwise.io/read/01",
        "source_url": "https://example.com/x", "title": "T", "author": "A",
        "site_name": "Site", "category": "article", "location": "later",
        "summary": "S", "image_url": "https://img/x.png", "word_count": 1200,
        "reading_time": "5 min", "published_date": "2024-01-01",
        "saved_at": "2024-02-02T00:00:00Z",
        "html_content": "<p>hi</p>"
      }]
    }"#;
    let ListResponse { articles, next_cursor } = parse_list(raw).unwrap();
    assert_eq!(next_cursor.as_deref(), Some("NEXT"));
    let a = &articles[0];
    assert_eq!(a.id.0, "01");
    assert_eq!(a.location, Location::Later);
    assert_eq!(a.html_content.as_deref(), Some("<p>hi</p>"));
    assert_eq!(a.source_url, "https://example.com/x");
}

#[test]
fn highlight_body_matches_v2_shape() {
    let body = highlight_body("the text", "Title", "Author", "https://example.com/x", "articles");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let h = &v["highlights"][0];
    assert_eq!(h["text"], "the text");
    assert_eq!(h["source_url"], "https://example.com/x");
    assert_eq!(h["category"], "articles");
}
```

- [ ] **Step 4: Run to verify failure**

Run: `cargo test -p inkapp-readwise-reader http`
Expected: FAIL (module/functions undefined).

- [ ] **Step 5: Implement `http.rs` (pure functions + thin transports)**

Create `crates/inkapp-readwise-reader/src/http.rs`:
```rust
//! Live Reader API: pure URL/body builders + response parsers (unit-tested), and
//! the thin reqwest-middleware transports that call them. Network round-trips are
//! covered by the `#[ignore]` live bar, not unit tests.

use serde::Deserialize;

use inkapp_core::connector::ConnectorError;

use crate::{Article, ArticleId, Location, Page, Write};

const LIST: &str = "https://readwise.io/api/v3/list/";
const UPDATE: &str = "https://readwise.io/api/v3/update/";
const DELETE: &str = "https://readwise.io/api/v3/delete/";
const HIGHLIGHTS: &str = "https://readwise.io/api/v2/highlights/";
const LIMIT: u32 = 50;

/// Build the list URL for a location + optional cursor.
pub fn build_list_url(location: &str, cursor: Option<&str>) -> String {
    let mut u = format!("{LIST}?withHtmlContent=true&limit={LIMIT}&location={location}");
    if let Some(c) = cursor {
        u.push_str("&pageCursor=");
        u.push_str(c);
    }
    u
}

/// The serialized v2 highlight-create body.
pub fn highlight_body(text: &str, title: &str, author: &str, source_url: &str, category: &str) -> String {
    serde_json::json!({
        "highlights": [{
            "text": text, "title": title, "author": author,
            "source_url": source_url, "category": category,
        }]
    }).to_string()
}

/// A parsed list page.
pub struct ListResponse { pub articles: Vec<Article>, pub next_cursor: Option<String> }

#[derive(Deserialize)]
struct RawList { #[serde(rename = "nextPageCursor")] next: Option<String>, results: Vec<RawDoc> }

#[derive(Deserialize)]
struct RawDoc {
    id: String,
    #[serde(default)] url: String,
    #[serde(default)] source_url: String,
    #[serde(default)] title: String,
    #[serde(default)] author: String,
    #[serde(default)] site_name: String,
    #[serde(default)] category: String,
    #[serde(default)] location: String,
    #[serde(default)] summary: String,
    #[serde(default)] image_url: Option<String>,
    #[serde(default)] word_count: Option<u32>,
    #[serde(default)] reading_time: Option<String>,
    #[serde(default)] published_date: Option<String>,
    #[serde(default)] saved_at: String,
    #[serde(default)] html_content: Option<String>,
}

fn loc_from(s: &str) -> Location {
    match s {
        "later" => Location::Later,
        "shortlist" => Location::Shortlist,
        "archive" => Location::Archive,
        "feed" => Location::Feed,
        _ => Location::New,
    }
}

/// Parse a Reader v3 list body into articles + cursor.
pub fn parse_list(raw: &str) -> Result<ListResponse, ConnectorError> {
    let parsed: RawList = serde_json::from_str(raw)
        .map_err(|e| ConnectorError::Transport(format!("list parse: {e}")))?;
    let articles = parsed.results.into_iter().map(|d| Article {
        id: ArticleId::new(d.id),
        title: d.title,
        body: String::new(),
        highlights: Vec::new(),
        url: d.url,
        source_url: d.source_url,
        author: d.author,
        site_name: d.site_name,
        category: d.category,
        location: loc_from(&d.location),
        summary: d.summary,
        image_url: d.image_url,
        word_count: d.word_count,
        reading_time: d.reading_time,
        published_date: d.published_date,
        saved_at: d.saved_at,
        html_content: d.html_content,
    }).collect();
    Ok(ListResponse { articles, next_cursor: parsed.next })
}

/// Map an HTTP status to a connector error (None = ok).
pub fn status_error(status: u16) -> Option<ConnectorError> {
    match status {
        200..=299 => None,
        401 | 403 => Some(ConnectorError::Auth(format!("status {status}"))),
        429 => Some(ConnectorError::RateLimited),
        s => Some(ConnectorError::Transport(format!("status {s}"))),
    }
}

// --- Thin transports (call the pure helpers above) ---

use crate::{FetchTransport, WriteTransport};
use reqwest_middleware::ClientWithMiddleware;

pub struct HttpFetch { pub client: ClientWithMiddleware, pub token: String }
pub struct HttpWrite {
    pub client: ClientWithMiddleware,
    pub token: String,
    /// Article lookup so a highlight push can fill title/author/source_url/category.
    pub lookup: std::sync::Arc<dyn Fn(&ArticleId) -> Option<Article> + Send + Sync>,
}

#[async_trait::async_trait]
impl FetchTransport for HttpFetch {
    async fn list(&self, location: &str, cursor: Option<&str>) -> Result<Page, ConnectorError> {
        let url = build_list_url(location, cursor);
        let resp = self.client.get(&url)  // Reader v3 list is GET
            .header("Authorization", format!("Token {}", self.token))
            .send().await.map_err(|e| ConnectorError::Transport(e.to_string()))?;
        if let Some(err) = status_error(resp.status().as_u16()) { return Err(err); }
        let raw = resp.text().await.map_err(|e| ConnectorError::Transport(e.to_string()))?;
        let ListResponse { articles, next_cursor } = parse_list(&raw)?;
        Ok(Page { articles, next_cursor })
    }
}

#[async_trait::async_trait]
impl WriteTransport for HttpWrite {
    async fn push(&self, write: &Write) -> Result<(), ConnectorError> {
        let auth = format!("Token {}", self.token);
        let resp = match write {
            Write::Move(id, loc) => self.client
                .patch(format!("{UPDATE}{}/", id.0)).header("Authorization", auth)
                .json(&serde_json::json!({ "location": loc.as_str() })).send().await,
            Write::Delete(id) => self.client
                .delete(format!("{DELETE}{}/", id.0)).header("Authorization", auth).send().await,
            Write::Highlight(id, text) => {
                let a = (self.lookup)(id).unwrap_or_else(|| Article { id: id.clone(), ..Default::default() });
                let cat = if a.category.is_empty() { "articles".to_string() } else { a.category.clone() };
                let body = highlight_body(text, &a.title, &a.author, &a.source_url, &cat);
                self.client.post(HIGHLIGHTS).header("Authorization", auth)
                    .header("Content-Type", "application/json").body(body).send().await
            }
        }.map_err(|e| ConnectorError::Transport(e.to_string()))?;
        match status_error(resp.status().as_u16()) { Some(err) => Err(err), None => Ok(()) }
    }
}
```
Add `pub mod http;` to `lib.rs`, and ensure `Article` derives `Default` (Task 2/3).

- [ ] **Step 6: Run tests**

Run: `cargo test -p inkapp-readwise-reader http` then `cargo test --workspace`
Expected: PASS. (Adjust `reqwest`/`reqwest-middleware` method calls to the exact 0.12/0.4 API if needed; the pure functions and tests are the contract.)

- [ ] **Step 7: Commit**

```bash
git add crates/inkapp-readwise-reader crates/inkapp-core
git commit -m "inkapp-readwise-reader: live Reader API request/parse + HTTP transports; ConnectorError Auth/RateLimited"
```

---

### Task 5: `live()` constructor — token via `SecretStore`, cache dir, config, retry

**Goal:** Assemble a production connector: token from `SecretStore`, a durable `Cache`, `ReaderConfig`, and retrying HTTP transports.

**Files:**
- Modify: `crates/inkapp-readwise-reader/src/lib.rs` (`live(...)`, retry wrapper, config wiring)
- Test: `crates/inkapp-readwise-reader/tests/live_ctor.rs` (new — construction only, no network)

**Acceptance Criteria:**
- [ ] `live(...)` with a `SecretStore` lacking the token returns `ConnectorError::Auth`.
- [ ] `live(...)` with a token present constructs a connector whose `locations` derive from `ReaderConfig` (library locations + `feed` when enabled).
- [ ] Read/write transports are built on a `reqwest-middleware` client that retries 429/5xx.

**Verify:** `cargo test -p inkapp-readwise-reader live_ctor` → pass.

**Steps:**

- [ ] **Step 1: Write failing test**

Create `crates/inkapp-readwise-reader/tests/live_ctor.rs`:
```rust
use inkapp_core::secrets::{Scope, SecretStore};
use inkapp_readwise_reader::{ReaderConfig, Readwise};
use inkapp_core::connector::ConnectorError;

#[tokio::test]
async fn live_requires_token() {
    let dir = tempfile::tempdir().unwrap();
    let secrets_path = dir.path().join("secrets.json");
    let cache_dir = dir.path().join("cache");
    let store = SecretStore::open(&secrets_path).unwrap();
    let err = Readwise::live(&store, &cache_dir, ReaderConfig::default()).await.unwrap_err();
    assert!(matches!(err, ConnectorError::Auth(_)));
}

#[tokio::test]
async fn live_builds_with_token() {
    let dir = tempfile::tempdir().unwrap();
    let secrets_path = dir.path().join("secrets.json");
    let cache_dir = dir.path().join("cache");
    let mut store = SecretStore::open(&secrets_path).unwrap();
    store.set(Scope::ConnectorCred, "readwise-reader", b"tok").unwrap();
    let rw = Readwise::live(&store, &cache_dir, ReaderConfig::default()).await.unwrap();
    // feed enabled by default → "feed" among the refresh locations.
    assert!(rw.locations_for_test().iter().any(|l| l == "feed"));
}
```
(Add a small `#[doc(hidden)] pub fn locations_for_test(&self) -> Vec<String>` returning `self.locations.clone()`.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p inkapp-readwise-reader live_ctor`
Expected: FAIL (`live` undefined).

- [ ] **Step 3: Implement `live()` + retry**

Add a retry middleware wrapper and the constructor:
```rust
use inkapp_core::secrets::{Scope, SecretStore};

impl Readwise {
    /// Derive the refresh location list from config.
    fn locations_from(config: &ReaderConfig) -> Vec<String> {
        let mut v: Vec<String> = config.library_locations.iter().map(|l| l.as_str().to_string()).collect();
        if config.feed_enabled { v.push("feed".to_string()); }
        v
    }

    /// Assemble a live connector: token from the secret store, a durable cache,
    /// retrying HTTP read+write transports.
    pub async fn live(
        secrets: &SecretStore,
        cache_dir: impl Into<std::path::PathBuf>,
        config: ReaderConfig,
    ) -> Result<Self, ConnectorError> {
        let token = secrets
            .get(Scope::ConnectorCred, "readwise-reader")
            .map_err(|e| ConnectorError::Auth(e.to_string()))?
            .ok_or_else(|| ConnectorError::Auth("no readwise-reader token in secret store".into()))?;
        let token = String::from_utf8(token).map_err(|e| ConnectorError::Auth(e.to_string()))?;

        let cache = std::sync::Arc::new(
            inkapp_core::cache::Cache::open(cache_dir.into(), 16 << 20, 512 << 20)
                .await
                .map_err(|e| ConnectorError::Transport(e.to_string()))?,
        );

        let client = Self::http_client();
        let fetch = std::sync::Arc::new(crate::http::HttpFetch { client: client.clone(), token: token.clone() });

        let mut me = Readwise::build(Vec::new(), Overlay::default(), None);
        me.config = config.clone();
        me.locations = Self::locations_from(&config);
        me.fetch = fetch;
        // Write transport needs to look up cached articles for highlight metadata.
        let warm = std::sync::Arc::clone(&me.cache_articles);
        let lookup = std::sync::Arc::new(move |id: &ArticleId| {
            warm.read().unwrap().iter().find(|a| &a.id == id).cloned()
        });
        me.transport = std::sync::Arc::new(crate::http::HttpWrite { client, token, lookup });
        me = me.with_cache_hydrated(cache).await;
        Ok(me)
    }

    /// A reqwest-middleware client with retry on 429/5xx.
    fn http_client() -> reqwest_middleware::ClientWithMiddleware {
        use reqwest_middleware::ClientBuilder;
        use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
        let retry = ExponentialBackoff::builder().build_with_max_retries(5);
        ClientBuilder::new(reqwest::Client::new())
            .with(RetryTransientMiddleware::new_with_policy(retry))
            .build()
    }
}
```
Add dep `reqwest-retry = "0.7"` to `Cargo.toml`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p inkapp-readwise-reader live_ctor` then `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/inkapp-readwise-reader
git commit -m "inkapp-readwise-reader: live() constructor (SecretStore token, durable cache, retrying HTTP)"
```

---

### Task 6: Proof binary `examples/pull.rs` + live `#[ignore]` bar

**Goal:** A runnable artifact that pulls a real account and demonstrates the offline warm cache; plus a read-only live integration test.

**Files:**
- Create: `crates/inkapp-readwise-reader/examples/pull.rs`
- Test: `crates/inkapp-readwise-reader/tests/live.rs` (new, `#[ignore]`)

**Acceptance Criteria:**
- [ ] `examples/pull.rs` compiles; documented run prints Feed/Library counts + first N titles, then a second pass served from the warm cache with networking disabled.
- [ ] `tests/live.rs::live_readwise_reader` is `#[ignore]`, read-only, and asserts non-empty Feed+Library when run with a real token.

**Verify (manual):** `cargo build -p inkapp-readwise-reader --examples`; with a token in the secret store, `cargo run -p inkapp-readwise-reader --example pull` and `cargo test -p inkapp-readwise-reader --test live -- --ignored`.

**Steps:**

- [ ] **Step 1: Write the example**

Create `crates/inkapp-readwise-reader/examples/pull.rs`:
```rust
//! Pull a real Readwise Reader account and prove the warm cache.
//! Requires a token: `SecretStore` cred `readwise-reader` (Scope::ConnectorCred).
//! Run: cargo run -p inkapp-readwise-reader --example pull

use inkapp_core::connector::Connector;
use inkapp_core::secrets::SecretStore;
use inkapp_readwise_reader::{ReaderConfig, Readwise};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = SecretStore::open_default()?;
    let cache_dir = std::env::temp_dir().join("inkapp-readwise-reader-pull");

    // Pass 1: live refresh.
    let rw = Readwise::live(&store, &cache_dir, ReaderConfig::default()).await?;
    rw.refresh().await?;
    println!("LIVE  feed={} library={}", rw.feed().len(), rw.library().len());
    for a in rw.feed().iter().take(5) { println!("  feed: {}", a.title); }
    for a in rw.library().iter().take(5) { println!("  lib : {}", a.title); }
    rw.close().await?; // flush durable cache (add a close() that closes the Cache)

    // Pass 2: no network — hydrate from the durable cache only (no refresh()).
    let cache = std::sync::Arc::new(
        inkapp_core::cache::Cache::open(&cache_dir, 16 << 20, 512 << 20).await?);
    let warm = Readwise::fake().with_cache_hydrated(cache).await;
    println!("WARM  feed={} library={} (served offline from cache)",
        warm.feed().len(), warm.library().len());
    Ok(())
}
```
Add a `pub async fn close(&self) -> Result<(), ConnectorError>` on `Readwise` that closes the durable `Cache` if present.

> Note: `Readwise::fake().with_cache_hydrated(...)` reuses the hydrate path; the warm pass calls no network. (`fake()`'s default cassette source is overwritten by the hydrated set.)

- [ ] **Step 2: Write the live bar**

Create `crates/inkapp-readwise-reader/tests/live.rs`:
```rust
use inkapp_core::connector::Connector;
use inkapp_core::secrets::SecretStore;
use inkapp_readwise_reader::{ReaderConfig, Readwise};

#[tokio::test]
#[ignore = "hits the real Readwise API; requires a token in the secret store"]
async fn live_readwise_reader() {
    let store = SecretStore::open_default().expect("secret store");
    let cache_dir = std::env::temp_dir().join("inkapp-readwise-reader-livetest");
    let rw = Readwise::live(&store, &cache_dir, ReaderConfig::default()).await.expect("live ctor");
    rw.refresh().await.expect("refresh");
    assert!(rw.feed().len() + rw.library().len() > 0, "expected some articles");
    // Read-only: no move/delete/highlight here.
}
```

- [ ] **Step 3: Verify it compiles and the suite stays green**

Run: `cargo build -p inkapp-readwise-reader --examples` and `cargo test --workspace`
Expected: example builds; `--ignored` live test is excluded from the normal run; all non-ignored tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/inkapp-readwise-reader
git commit -m "inkapp-readwise-reader: pull example (offline warm-cache proof) + read-only live bar"
```

---

### Task 7: Mark built in `docs/appdx.md`

**Goal:** Record the live `readwise-reader` connector + `inkapp-core::cache` primitive as built — the repo's definition of done.

**Files:**
- Modify: `docs/appdx.md`

**Acceptance Criteria:**
- [ ] `docs/appdx.md` notes the live Readwise Reader connector and the durable `Cache` primitive (foyer) as implemented, in the same voice as existing status notes.
- [ ] `cargo test --workspace` green.

**Verify:** `cargo test --workspace` → pass; `docs/appdx.md` mentions the connector + cache.

**Steps:**

- [ ] **Step 1: Update the status note**

In `docs/appdx.md`, extend the Status section (and/or the Connectors section) to state that the touchstone reading app now has a **live Readwise Reader connector** (`inkapp-readwise-reader`: HTTP reads + write-back, durable warm-restart cache) backed by a reusable **`inkapp-core::cache`** primitive (foyer hybrid memory+disk, sha256 integrity for content-addressed derived keys), with cassette mode retained for tests. Note pagination and the HTML→Typst content/image pipeline remain the next worktrees.

- [ ] **Step 2: Verify**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add docs/appdx.md
git commit -m "appdx: live readwise-reader connector + inkapp-core::cache primitive built"
```

---

## Self-Review

**Spec coverage:**
- Rename → Task 0. Cache primitive (foyer, integrity, derived keys, warm restart) → Task 1. Expanded model + Location + Write variants + library/feed → Task 2. FetchTransport seam + cache-backed refresh + **overlay reconciliation (spec §D)** → Task 3 (Step 5 reconciliation block + `refresh_prunes_applied_overlay_entry` test). Live HTTP reads/writes (auth, retry, parse) → Tasks 4–5. SecretStore token → Task 5. Testing + live bar + proof binary → Task 6 (unit/cache tests are inline in Tasks 1–5). appdx → Task 7.
- **Error handling (spec §F):** `Auth`/`RateLimited` → Task 4; partial-failure preserves warm cache → Task 3 (refresh `?`-returns on a fetch error before the `*warm.write()` and before overlay reconciliation); cache errors non-fatal for reads → Task 1 (`get_*` map errors but `refresh` uses best-effort `let _ =` on put; durable-cache miss reads as `None`).

**Placeholder scan:** No TBD/TODO; every code step has real code. The two "adjust to exact 0.x API" notes (foyer builder, reqwest-middleware) are explicit fallbacks with the contract pinned by tests, not placeholders.

**Type consistency:** `Article`, `Location`, `Write`, `Page`, `FetchTransport`, `ListResponse`, `Integrity`, `Cache`, `ReaderConfig` names are used consistently across tasks. `Article` must derive `Default` (introduced in Task 2, relied on in Tasks 3–5) — ensure the derive is added in Task 2. The in-memory warm field is renamed `cache_articles` (Task 3) and used by `queue()`/`live()` consistently thereafter.

**Fixes applied inline:** (1) Overlay reconciliation (spec §D) is now coded into Task 3 Step 5 with a covering test and acceptance criterion. (2) `Article: Default` derive requirement is called out in Tasks 2–3.
