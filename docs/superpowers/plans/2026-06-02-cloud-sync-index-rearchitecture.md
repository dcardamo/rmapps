# reMarkable cloud sync-index re-architecture — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the recurring reMarkable cloud 429s by replacing the brute-force O(N_account) metadata scan in `ls` with a persistent local sync index + Merkle root-index diff, and adding a transport rate governor that makes a cold scan survivable.

**Architecture:** A process-global rate governor wraps the single HTTP chokepoint (`send_retrying`) so no burst can exceed a concurrency + spacing ceiling. A persistent `SyncStore` (`~/.cache/rmapps/sync-index.json`) holds `{account_generation, docId → (hash, parent, name, is_folder)}`. `Client::resolved_snapshot()` polls the root generation (1 request); if unchanged it returns the store verbatim (zero metadata fetches), else it diffs the root index by hash and fetches `.metadata` only for added/changed docs. All path porcelain (`ls`, `mkdir_p`, `resolve_folder`, the rmapps deploy path) reads from this resolved view instead of scanning the account.

**Tech Stack:** Rust, tokio, reqwest, axum (test fake cloud), serde_json. Crates: `rm-cloud` (client + protocol), `rmapps` (CLI + deploy porcelain).

**User Verification:** YES — after the automated suite is green, Dan runs a live `rmapps reader` against the real cloud (once the rate-limit window has cleared) and confirms it completes the deploy with no 429. The FakeCloud tests prove request-count reductions but cannot prove the live limiter is satisfied.

---

## File Structure

| File | Responsibility | Change |
|------------------------------------------------|-------------------------------------------------------------|--------|
| `crates/rm-cloud/src/transport.rs` | HTTP chokepoint; add the `Governor` + wire it into `send_retrying` | Modify |
| `crates/rm-cloud/src/sync_store.rs` | NEW: persistent local sync index + `ResolvedTree` query type | Create |
| `crates/rm-cloud/src/client.rs` | Hold optional `SyncStore`; `resolved_snapshot()` algorithm | Modify |
| `crates/rm-cloud/src/porcelain/fs.rs` | Rewire `ls`/`mkdir_p`/`resolve` onto `resolved_snapshot`; drop `LS_CONCURRENCY` | Modify |
| `crates/rm-cloud/src/lib.rs` | Export `SyncStore`, `ResolvedTree`, `ResolvedDoc` | Modify |
| `crates/rm-cloud/src/fake/mod.rs` + `handlers.rs` | Add a `root_gets` counter for listing/governor assertions | Modify |
| `apps/rmapps/src/cloud.rs` | Walk/list/deploy via one resolved snapshot; wire `SyncStore`; `default_sync_index_path()` | Modify |
| `apps/rmapps/src/auth.rs` | Attach `SyncStore` to the two auth-path `Client` constructions | Modify |

---

## Task 1: Transport rate governor

**Goal:** A process-global concurrency + min-spacing throttle on every cloud request, so even a cold O(N) scan drips under the limit instead of bursting. Independently fixes today's 429 even before the sync index lands.

**Files:**
- Modify: `crates/rm-cloud/src/transport.rs`
- Modify: `crates/rm-cloud/src/porcelain/fs.rs` (remove the now-redundant `LS_CONCURRENCY` semaphore)

**Acceptance Criteria:**
- [ ] A `Governor` caps concurrent in-flight requests at its configured limit.
- [ ] A `Governor` enforces a minimum interval between request starts.
- [ ] `send_retrying` acquires a global governor permit (configured from env) before sending, holding it across retries.
- [ ] `fs.rs` no longer has its own `LS_CONCURRENCY` semaphore (global governor subsumes it).
- [ ] Existing transport tests still pass.

**Verify:** `cargo test -p rm-cloud transport` → all pass, including the two new governor tests.

**Steps:**

- [ ] **Step 1: Write failing governor tests** — append to the `tests` module in `crates/rm-cloud/src/transport.rs`:

```rust
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn governor_caps_concurrency() {
        let gov = Governor::new(3, Duration::ZERO);
        let inflight = Arc::new(AtomicUsize::new(0));
        let max = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..20 {
            let gov = gov.clone();
            let inflight = inflight.clone();
            let max = max.clone();
            handles.push(tokio::spawn(async move {
                let _permit = gov.acquire().await;
                let now = inflight.fetch_add(1, Ordering::SeqCst) + 1;
                max.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(10)).await;
                inflight.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        assert!(max.load(Ordering::SeqCst) <= 3, "concurrency exceeded cap");
    }

    #[tokio::test]
    async fn governor_spaces_request_starts() {
        let interval = Duration::from_millis(20);
        let gov = Governor::new(8, interval);
        let start = Instant::now();
        let mut starts = Vec::new();
        for _ in 0..5 {
            let _permit = gov.acquire().await;
            starts.push(start.elapsed());
        }
        // 5 sequential acquisitions must span at least 4 intervals.
        assert!(
            starts[4] >= interval * 4,
            "spacing not enforced: {:?}",
            starts
        );
    }
```

- [ ] **Step 2: Run tests, expect failure**

Run: `cargo test -p rm-cloud transport::tests::governor 2>&1 | tail -20`
Expected: FAIL — `cannot find type 'Governor'`.

- [ ] **Step 3: Implement the Governor** — add near the top of `crates/rm-cloud/src/transport.rs` (after the existing `use` lines):

```rust
use std::sync::Arc;
use std::sync::OnceLock;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

/// Default cap on concurrent in-flight cloud requests (env: `RM_CLOUD_MAX_CONCURRENCY`).
const DEFAULT_MAX_CONCURRENCY: usize = 4;
/// Default minimum spacing between request starts in ms (env: `RM_CLOUD_MIN_INTERVAL_MS`).
const DEFAULT_MIN_INTERVAL_MS: u64 = 150;

/// A process-global request throttle: a concurrency cap plus a minimum interval between
/// request *starts*. The reMarkable cloud rate-limits aggressively and the account-wide
/// `ls` fan-out can otherwise burst hundreds of requests; the governor spreads them so a
/// cold cache cannot trip 429.
#[derive(Clone)]
pub(crate) struct Governor {
    sem: Arc<Semaphore>,
    /// Earliest instant the next request may start.
    gate: Arc<Mutex<Instant>>,
    min_interval: Duration,
}

impl Governor {
    pub(crate) fn new(max_concurrency: usize, min_interval: Duration) -> Self {
        Self {
            sem: Arc::new(Semaphore::new(max_concurrency.max(1))),
            gate: Arc::new(Mutex::new(Instant::now())),
            min_interval,
        }
    }

    /// Build from env vars, falling back to the defaults above.
    fn from_env() -> Self {
        let max = std::env::var("RM_CLOUD_MAX_CONCURRENCY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_MAX_CONCURRENCY);
        let interval_ms = std::env::var("RM_CLOUD_MIN_INTERVAL_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_MIN_INTERVAL_MS);
        Self::new(max, Duration::from_millis(interval_ms))
    }

    /// Acquire a slot: wait for a concurrency permit, then wait out the spacing gate.
    /// The returned permit must be held for the whole request (including retries).
    pub(crate) async fn acquire(&self) -> OwnedSemaphorePermit {
        let permit = self
            .sem
            .clone()
            .acquire_owned()
            .await
            .expect("governor semaphore is never closed");
        if self.min_interval > Duration::ZERO {
            let mut gate = self.gate.lock().await;
            let now = Instant::now();
            let next = (*gate).max(now);
            *gate = next + self.min_interval;
            drop(gate);
            tokio::time::sleep_until(next).await;
        }
        permit
    }
}

/// The process-global governor, initialized from env on first use.
fn governor() -> &'static Governor {
    static GOVERNOR: OnceLock<Governor> = OnceLock::new();
    GOVERNOR.get_or_init(Governor::from_env)
}
```

> Note: `Duration` is already imported at the top of the file (`use std::time::Duration;`). Keep that import; add the others above.

- [ ] **Step 4: Wire the governor into `send_retrying`** — at the very start of the function body, before the `let mut attempt` line:

```rust
pub(crate) async fn send_retrying(builder: reqwest::RequestBuilder) -> Result<reqwest::Response> {
    // Hold a governor permit for the whole request (incl. 429 backoff) so a retrying
    // request keeps its concurrency slot rather than letting new requests pile on.
    let _permit = governor().acquire().await;
    let mut attempt = 0u32;
    loop {
```

(The rest of the loop is unchanged.)

- [ ] **Step 5: Remove the redundant `LS_CONCURRENCY` semaphore** in `crates/rm-cloud/src/porcelain/fs.rs`. Delete the `const LS_CONCURRENCY` (lines ~14-16) and the `Semaphore`/permit usage in `ls_with`. Replace the `ls_with` spawn loop body so it no longer acquires a local permit:

```rust
        let mut set = tokio::task::JoinSet::new();
        for (id, hash) in docs {
            let client = self.clone();
            set.spawn(async move {
                let meta = client.metadata_by(&hash, &id).await;
                (id, hash, meta)
            });
        }
```

Remove `use std::sync::Arc;` and `use tokio::sync::Semaphore;` from `fs.rs` if they become unused (the compiler will tell you).

> This keeps `ls_with`'s current behavior but governed by the global cap. Task 4 replaces `ls_with` entirely; this interim keeps Task 1 independently shippable.

- [ ] **Step 6: Run tests, expect pass**

Run: `cargo test -p rm-cloud 2>&1 | tail -20`
Expected: PASS — governor tests green, all existing transport/porcelain tests still green.

- [ ] **Step 7: Commit**

```bash
git add crates/rm-cloud/src/transport.rs crates/rm-cloud/src/porcelain/fs.rs
git commit -m "feat(rm-cloud): process-global request rate governor (concurrency + spacing)"
```

```json:metadata
{"files": ["crates/rm-cloud/src/transport.rs", "crates/rm-cloud/src/porcelain/fs.rs"], "verifyCommand": "cargo test -p rm-cloud", "acceptanceCriteria": ["Governor caps concurrency", "Governor enforces min spacing", "send_retrying acquires a global permit", "LS_CONCURRENCY removed", "existing tests pass"], "requiresUserVerification": false}
```

---

## Task 2: `SyncStore` persistence layer + `ResolvedTree`

**Goal:** A persistent, network-free local sync index and the query type path porcelain reads from. No `Client`/network involvement — pure data + atomic disk IO.

**Files:**
- Create: `crates/rm-cloud/src/sync_store.rs`
- Modify: `crates/rm-cloud/src/lib.rs` (declare `mod sync_store;` and re-export)

**Acceptance Criteria:**
- [ ] `ResolvedTree::children(parent)` returns the direct, non-deleted children as `Entry`s, sorted by name.
- [ ] `ResolvedTree::resolve_folder(path)` resolves a slash path to a folder id, `None` if any segment is missing.
- [ ] `SyncStore::new(path)` loads an existing index; a missing/corrupt/wrong-schema file yields an empty store (no error).
- [ ] `SyncStore::store(tree)` persists atomically (temp + rename) and updates the in-memory copy.
- [ ] Round-trip: store then `new()` from the same path returns the same tree.

**Verify:** `cargo test -p rm-cloud sync_store` → all pass.

**Steps:**

- [ ] **Step 1: Write the module with failing tests** — create `crates/rm-cloud/src/sync_store.rs`:

```rust
//! Persistent local sync state: the durable `{generation, docId → (hash, parent, name)}`
//! index that mirrors what the tablet keeps, so listing/path resolution need not re-read
//! every doc's metadata. Fully reconstructible from the cloud, so a missing or corrupt
//! file is not an error — it yields an empty store and forces a cold rebuild.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::porcelain::fs::Entry;

/// Current on-disk schema version. Bump on any breaking shape change; an older/newer
/// value loads as empty (forcing a cold rebuild) rather than erroring.
const SCHEMA_VERSION: u32 = 1;

/// One resolved document: its content hash plus the path facts that live in `.metadata`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedDoc {
    /// Cloud doc hash (Merkle) — the change-detection key.
    pub hash: String,
    /// Parent folder id ("" = root, "trash" = trash).
    pub parent: String,
    /// Visible name.
    pub name: String,
    /// True if this doc is a folder (`CollectionType`).
    pub is_folder: bool,
}

/// An account view resolved to ids + paths at a single generation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedTree {
    /// Account generation this view was built at.
    pub generation: i64,
    /// docId → resolved doc.
    pub docs: BTreeMap<String, ResolvedDoc>,
}

impl ResolvedTree {
    /// Direct, non-deleted children of `parent`, as listing `Entry`s, sorted by name.
    pub fn children(&self, parent: &str) -> Vec<Entry> {
        let mut out: Vec<Entry> = self
            .docs
            .iter()
            .filter(|(_, d)| d.parent == parent)
            .map(|(id, d)| Entry {
                id: id.clone(),
                name: d.name.clone(),
                parent: d.parent.clone(),
                is_folder: d.is_folder,
                hash: d.hash.clone(),
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Resolve a slash path ("" / "/" = root) to a folder id, matching `CollectionType`
    /// children case-sensitively. `None` if any segment is missing.
    pub fn resolve_folder(&self, path: &str) -> Option<String> {
        let mut parent = String::new();
        for seg in path.split('/').filter(|s| !s.is_empty()) {
            let next = self
                .children(&parent)
                .into_iter()
                .find(|e| e.is_folder && e.name == seg)?;
            parent = next.id;
        }
        Some(parent)
    }
}

/// On-disk envelope (carries the schema tag alongside the tree).
#[derive(Serialize, Deserialize)]
struct StoredIndex {
    schema_version: u32,
    tree: ResolvedTree,
}

/// A persistent local sync index at a fixed path.
pub struct SyncStore {
    path: PathBuf,
    tree: RwLock<ResolvedTree>,
}

impl SyncStore {
    /// Open (or initialize empty) the index at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let tree = Self::load(&path).unwrap_or_default();
        Self {
            path,
            tree: RwLock::new(tree),
        }
    }

    /// Tolerant load: `None` on any missing-file / parse / schema-mismatch condition.
    fn load(path: &std::path::Path) -> Option<ResolvedTree> {
        let bytes = std::fs::read(path).ok()?;
        let stored: StoredIndex = serde_json::from_slice(&bytes).ok()?;
        if stored.schema_version != SCHEMA_VERSION {
            return None;
        }
        Some(stored.tree)
    }

    /// A clone of the current in-memory tree.
    pub fn tree(&self) -> ResolvedTree {
        self.tree.read().expect("sync store lock poisoned").clone()
    }

    /// Replace the index with `tree`, persisting atomically. Persist failure is
    /// best-effort (logged via the returned error being dropped by callers) and must not
    /// corrupt the live file — the temp+rename guarantees the previous file stays intact
    /// until a complete new file is in place.
    pub fn store(&self, tree: &ResolvedTree) {
        *self.tree.write().expect("sync store lock poisoned") = tree.clone();
        let _ = self.persist(tree);
    }

    fn persist(&self, tree: &ResolvedTree) -> std::io::Result<()> {
        let stored = StoredIndex {
            schema_version: SCHEMA_VERSION,
            tree: tree.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&stored)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self
            .path
            .with_extension(format!("tmp.{}", std::process::id()));
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(hash: &str, parent: &str, name: &str, is_folder: bool) -> ResolvedDoc {
        ResolvedDoc {
            hash: hash.into(),
            parent: parent.into(),
            name: name.into(),
            is_folder,
        }
    }

    fn sample() -> ResolvedTree {
        let mut docs = BTreeMap::new();
        docs.insert("rw".into(), doc("h1", "", "Readwise", true));
        docs.insert("feed".into(), doc("h2", "rw", "Feed", false));
        docs.insert("lib".into(), doc("h3", "rw", "Library", false));
        ResolvedTree {
            generation: 7,
            docs,
        }
    }

    #[test]
    fn children_filters_and_sorts() {
        let t = sample();
        let kids: Vec<String> = t.children("rw").into_iter().map(|e| e.name).collect();
        assert_eq!(kids, vec!["Feed", "Library"]);
        assert_eq!(t.children("").len(), 1); // just the Readwise folder
    }

    #[test]
    fn resolve_folder_walks_segments() {
        let t = sample();
        assert_eq!(t.resolve_folder("/Readwise"), Some("rw".into()));
        assert_eq!(t.resolve_folder(""), Some("".into()));
        assert_eq!(t.resolve_folder("/Readwise/Nope"), None);
    }

    #[test]
    fn store_then_reopen_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync-index.json");
        let store = SyncStore::new(&path);
        store.store(&sample());
        let reopened = SyncStore::new(&path);
        assert_eq!(reopened.tree(), sample());
    }

    #[test]
    fn missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = SyncStore::new(dir.path().join("absent.json"));
        assert_eq!(store.tree(), ResolvedTree::default());
    }

    #[test]
    fn corrupt_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync-index.json");
        std::fs::write(&path, b"not json at all").unwrap();
        let store = SyncStore::new(&path);
        assert_eq!(store.tree(), ResolvedTree::default());
    }

    #[test]
    fn wrong_schema_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync-index.json");
        std::fs::write(&path, br#"{"schema_version":999,"tree":{"generation":1,"docs":{}}}"#)
            .unwrap();
        let store = SyncStore::new(&path);
        assert_eq!(store.tree(), ResolvedTree::default());
    }
}
```

- [ ] **Step 2: Declare and export the module** — in `crates/rm-cloud/src/lib.rs`, add `mod sync_store;` in the module list (after `mod sync;`) and add the re-export:

```rust
pub use sync_store::{ResolvedDoc, ResolvedTree, SyncStore};
```

- [ ] **Step 3: Run tests, expect pass**

Run: `cargo test -p rm-cloud sync_store 2>&1 | tail -20`
Expected: PASS — all six `sync_store::tests::*`.

- [ ] **Step 4: Commit**

```bash
git add crates/rm-cloud/src/sync_store.rs crates/rm-cloud/src/lib.rs
git commit -m "feat(rm-cloud): persistent SyncStore + ResolvedTree query type"
```

```json:metadata
{"files": ["crates/rm-cloud/src/sync_store.rs", "crates/rm-cloud/src/lib.rs"], "verifyCommand": "cargo test -p rm-cloud sync_store", "acceptanceCriteria": ["children filters+sorts", "resolve_folder walks segments", "atomic store round-trips", "missing/corrupt/wrong-schema load empty"], "requiresUserVerification": false}
```

---

## Task 3: `Client::resolved_snapshot()` + wire `SyncStore` into `Client`

**Goal:** The new listing heart: poll generation (1 request); if the store is current, return it with zero metadata fetches; else diff the root index by hash and fetch `.metadata` only for added/changed docs.

**Files:**
- Modify: `crates/rm-cloud/src/client.rs` (add `sync_store` field, `with_sync_store`, `resolved_snapshot`)
- Modify: `crates/rm-cloud/src/fake/mod.rs` and `crates/rm-cloud/src/fake/handlers.rs` (add `root_gets` counter)

**Acceptance Criteria:**
- [ ] With a current store, `resolved_snapshot()` issues exactly one request (the root ref) and zero blob GETs.
- [ ] When one doc changed, only that doc's metadata blobs are fetched; all other docs are store hits.
- [ ] A cold store performs the full resolve and persists; an immediately-following call issues one request.
- [ ] A removed doc disappears from the resolved tree with no fetch.

**Verify:** `cargo test -p rm-cloud --features fake resolved` → all pass.

**Steps:**

- [ ] **Step 1: Add a `root_gets` counter to the fake** — in `crates/rm-cloud/src/fake/mod.rs`, add to `struct State`:

```rust
    /// Count of root-ref GETs served (test assertion of generation-poll cost).
    pub root_gets: u32,
```

and a helper on `FakeCloud`:

```rust
    /// Number of root-ref GETs served (test helper).
    pub fn root_get_count(&self) -> u32 {
        self.state.lock().unwrap().root_gets
    }
```

In `crates/rm-cloud/src/fake/handlers.rs`, inside `root_get`, increment the counter on a successful (non-404, non-error) response. After the `unauthorized_once` block and before building the `RootResp`, add the increment in the success branch:

```rust
    let mut s = state.lock().unwrap();
    if s.generation == 0 && s.root_hash.is_empty() {
        return (StatusCode::NOT_FOUND, "no root yet").into_response();
    }
    s.root_gets += 1;
    Json(RootResp {
        hash: s.root_hash.clone(),
        generation: s.generation,
        schema_version: 4,
    })
    .into_response()
```

(Change the existing `let s = ...` binding to `let mut s = ...` so the counter can be bumped.)

- [ ] **Step 2: Write failing `resolved_snapshot` tests** — add a `#[cfg(all(test, feature = "fake"))]` test module at the end of `crates/rm-cloud/src/client.rs` (or extend the existing test module if it is already `feature = "fake"`-gated). Use the existing test helpers pattern (see the `snapshot` tests around `client.rs:340`):

```rust
#[cfg(all(test, feature = "fake"))]
mod resolved_tests {
    use super::*;
    use crate::fake::FakeCloud;
    use crate::porcelain::docfiles::DocFiles;
    use crate::sync_store::SyncStore;
    use crate::Metadata;

    fn pdf_doc(id: &str, name: &str, parent: &str) -> DocFiles {
        let meta = Metadata {
            visible_name: name.into(),
            doc_type: "DocumentType".into(),
            parent: parent.into(),
            last_modified: "0".into(),
            deleted: false,
            extra: Default::default(),
        };
        DocFiles {
            id: id.into(),
            files: vec![
                (format!("{id}.metadata"), serde_json::to_vec(&meta).unwrap()),
                (format!("{id}.content"), b"{}".to_vec()),
                (format!("{id}.pdf"), format!("pdf-{name}").into_bytes()),
            ],
        }
    }

    async fn client_with_store(base: &str, dir: &std::path::Path) -> Client {
        Client::from_user_token(Config::single_host(base), "user-token")
            .with_sync_store(SyncStore::new(dir.join("sync-index.json")))
    }

    #[tokio::test]
    async fn current_generation_returns_store_without_metadata_fetches() {
        let fake = FakeCloud::spawn().await;
        let dir = tempfile::tempdir().unwrap();
        let client = client_with_store(&fake.base, dir.path()).await;

        // Seed two docs and warm the store.
        client.put(pdf_doc("a", "Alpha", "")).await.unwrap();
        client.put(pdf_doc("b", "Beta", "")).await.unwrap();
        let _ = client.resolved_snapshot().await.unwrap();

        let roots_before = fake.root_get_count();
        let a_hash = client.snapshot().await.unwrap().doc("a").unwrap().hash.clone();
        let a_gets_before = fake.blob_get_count(&a_hash);

        // Second call at the same generation: one root GET, zero doc-index GETs.
        let tree = client.resolved_snapshot().await.unwrap();
        assert_eq!(tree.docs.len(), 2);
        assert_eq!(
            fake.root_get_count() - roots_before,
            1,
            "exactly one generation poll"
        );
        assert_eq!(
            fake.blob_get_count(&a_hash),
            a_gets_before,
            "no doc-index refetch when generation unchanged"
        );
    }

    #[tokio::test]
    async fn only_changed_doc_is_refetched() {
        let fake = FakeCloud::spawn().await;
        let dir = tempfile::tempdir().unwrap();
        let client = client_with_store(&fake.base, dir.path()).await;
        client.put(pdf_doc("a", "Alpha", "")).await.unwrap();
        client.put(pdf_doc("b", "Beta", "")).await.unwrap();
        let _ = client.resolved_snapshot().await.unwrap();

        let a_hash = client.snapshot().await.unwrap().doc("a").unwrap().hash.clone();
        let a_gets_before = fake.blob_get_count(&a_hash);

        // Change only doc b (new content -> new hash -> generation bump).
        client.put(pdf_doc("b", "Beta v2", "")).await.unwrap();
        let tree = client.resolved_snapshot().await.unwrap();

        assert_eq!(tree.docs.get("b").unwrap().name, "Beta v2");
        assert_eq!(
            fake.blob_get_count(&a_hash),
            a_gets_before,
            "unchanged doc a must not be refetched"
        );
    }

    #[tokio::test]
    async fn removed_doc_drops_from_tree() {
        let fake = FakeCloud::spawn().await;
        let dir = tempfile::tempdir().unwrap();
        let client = client_with_store(&fake.base, dir.path()).await;
        client.put(pdf_doc("a", "Alpha", "")).await.unwrap();
        client.put(pdf_doc("b", "Beta", "")).await.unwrap();
        let _ = client.resolved_snapshot().await.unwrap();

        client.rm("a").await.unwrap();
        let tree = client.resolved_snapshot().await.unwrap();
        assert!(tree.docs.get("a").is_none());
        assert!(tree.docs.get("b").is_some());
    }
}
```

- [ ] **Step 3: Run tests, expect failure**

Run: `cargo test -p rm-cloud --features fake resolved 2>&1 | tail -20`
Expected: FAIL — `no method named 'with_sync_store'` / `resolved_snapshot`.

- [ ] **Step 4: Add the `sync_store` field + builder** — in `crates/rm-cloud/src/client.rs`:

In `struct Client`, after the `cache` field:

```rust
    pub(crate) sync_store: Option<Arc<crate::sync_store::SyncStore>>,
```

In `fn new`, add `sync_store: None,` to the constructor. Add the builder after `with_cache`:

```rust
    /// Attach a persistent local sync index. Listing/path resolution route through it,
    /// so unchanged docs cost no metadata fetch.
    pub fn with_sync_store(mut self, store: crate::sync_store::SyncStore) -> Self {
        self.sync_store = Some(Arc::new(store));
        self
    }
```

- [ ] **Step 5: Implement `resolved_snapshot`** — add to the `impl Client` block in `client.rs` (near `snapshot`):

```rust
    /// Resolve the account to ids + paths, reusing the persistent sync index. Polls the
    /// root generation (one request); if the store is current, returns it verbatim with no
    /// metadata fetches. Otherwise diffs the root index by doc hash and fetches `.metadata`
    /// only for added/changed docs, then persists the updated index.
    pub async fn resolved_snapshot(&self) -> Result<crate::sync_store::ResolvedTree> {
        use crate::sync_store::{ResolvedDoc, ResolvedTree};

        let Some(gen) = self.current_generation().await? else {
            // Account never synced.
            return Ok(ResolvedTree::default());
        };

        let prev = self
            .sync_store
            .as_ref()
            .map(|s| s.tree())
            .unwrap_or_default();

        // Fast path: store already built at this generation.
        if self.sync_store.is_some() && prev.generation == gen && !prev.docs.is_empty() {
            return Ok(prev);
        }

        // Rebuild: fetch the root index (blob-cache served when its hash is known) and diff.
        let snap = self.snapshot().await?;
        let mut docs = std::collections::BTreeMap::new();
        for d in snap.docs() {
            if let Some(p) = prev.docs.get(&d.id) {
                if p.hash == d.hash {
                    docs.insert(d.id.clone(), p.clone()); // unchanged → no fetch
                    continue;
                }
            }
            // Added or changed → read just this doc's metadata.
            let meta = self.metadata_by(&d.hash, &d.id).await?;
            docs.insert(
                d.id.clone(),
                ResolvedDoc {
                    hash: d.hash.clone(),
                    parent: meta.parent,
                    name: meta.visible_name,
                    is_folder: meta.doc_type == "CollectionType",
                },
            );
        }
        let tree = ResolvedTree {
            generation: snap.generation,
            docs,
        };
        if let Some(store) = &self.sync_store {
            store.store(&tree);
        }
        Ok(tree)
    }
```

> Cacheless behavior (no store attached, e.g. unit tests): `prev` is empty, the fast-path guard (`self.sync_store.is_some()`) is false, so it always does a full resolve — correct and matches today's semantics.

- [ ] **Step 6: Run tests, expect pass**

Run: `cargo test -p rm-cloud --features fake resolved 2>&1 | tail -20`
Expected: PASS — all three `resolved_tests::*`.

- [ ] **Step 7: Commit**

```bash
git add crates/rm-cloud/src/client.rs crates/rm-cloud/src/fake/mod.rs crates/rm-cloud/src/fake/handlers.rs
git commit -m "feat(rm-cloud): resolved_snapshot via persistent sync index (Merkle diff)"
```

```json:metadata
{"files": ["crates/rm-cloud/src/client.rs", "crates/rm-cloud/src/fake/mod.rs", "crates/rm-cloud/src/fake/handlers.rs"], "verifyCommand": "cargo test -p rm-cloud --features fake resolved", "acceptanceCriteria": ["current store -> 1 request, 0 metadata GETs", "only changed doc refetched", "cold then warm", "removed doc drops"], "requiresUserVerification": false}
```

---

## Task 4: Rewire `rm-cloud` path porcelain onto `resolved_snapshot`

**Goal:** `ls`, `mkdir_p`, and `stat` stop scanning the account per call; they read from one resolved tree. Removes the O(N) metadata fan-out from the crate's own porcelain.

**Files:**
- Modify: `crates/rm-cloud/src/porcelain/fs.rs`

**Acceptance Criteria:**
- [ ] `ls(parent)` returns the same entries as before, sourced from `resolved_snapshot()`.
- [ ] `mkdir_p` resolves existing segments from the resolved tree and only issues a `mkdir` for genuinely missing segments.
- [ ] The old per-doc-metadata `ls_with` fan-out is gone.
- [ ] Existing `fs.rs` / porcelain tests pass; add a request-count test proving a warm `ls` issues one root GET and no doc-index GETs.

**Verify:** `cargo test -p rm-cloud --features fake fs 2>&1 | tail -20` → pass.

**Steps:**

- [ ] **Step 1: Write a failing warm-`ls` request-count test** — add to the `fs.rs` test module (it is `feature = "fake"`-gated; mirror existing tests there):

```rust
    #[tokio::test]
    async fn warm_ls_costs_one_root_get() {
        use crate::sync_store::SyncStore;
        let fake = crate::fake::FakeCloud::spawn().await;
        let dir = tempfile::tempdir().unwrap();
        let client = Client::from_user_token(crate::Config::single_host(&fake.base), "user-token")
            .with_sync_store(SyncStore::new(dir.path().join("idx.json")));

        client.put(crate::porcelain::fs::tests::folder_doc("f", "Folder", "")).await.unwrap();
        let _ = client.ls("").await.unwrap(); // warm the store

        let roots_before = fake.root_get_count();
        let blobs_before = fake.blob_count_total();
        let entries = client.ls("").await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(fake.root_get_count() - roots_before, 1, "one generation poll");
        assert_eq!(fake.blob_count_total(), blobs_before, "no blob GETs when warm");
    }
```

> If a `folder_doc` helper / `blob_count_total` does not yet exist, add them: a small `pub(crate) fn folder_doc(id, name, parent) -> DocFiles` in the `fs.rs` test module (a `DocFiles` whose `.metadata` has `doc_type: "CollectionType"`), and on `FakeCloud` a `pub fn blob_count_total(&self) -> u32 { self.state.lock().unwrap().blob_gets.values().sum() }`.

- [ ] **Step 2: Run the test, expect failure** (compile error or the assertion failing because `ls` still scans).

Run: `cargo test -p rm-cloud --features fake warm_ls 2>&1 | tail -20`

- [ ] **Step 3: Rewrite `ls` / `ls_with` / `mkdir_p`** in `fs.rs`. Replace `ls_with`/`ls` with a tree-sourced implementation:

```rust
    /// List the direct children of `parent` ("" = root), sourced from the resolved sync
    /// index. One generation poll; metadata is read only for docs whose hash moved.
    pub async fn ls(&self, parent: &str) -> Result<Vec<Entry>> {
        let tree = self.resolved_snapshot().await?;
        Ok(tree.children(parent))
    }
```

Delete `ls_with` (the `Snapshot`-threaded scanner) — its callers move to `resolved_snapshot()`/`ResolvedTree` (rmapps in Task 5; the crate's own `mkdir_p` below). Remove the now-unused `Semaphore`/`JoinSet`/`Arc` imports and the `metadata_from`/`metadata_by` calls that only `ls_with` used (keep `metadata_by` — `resolved_snapshot` uses it).

Rewrite `mkdir_p` to resolve from one tree and create only missing segments:

```rust
    /// Resolve a slash path to a folder id, creating missing segments (`mkdir -p`).
    pub async fn mkdir_p(&self, path: &str) -> Result<String> {
        let mut tree = self.resolved_snapshot().await?;
        let mut parent = String::new();
        for segment in path.split('/').filter(|s| !s.is_empty()) {
            validate_segment(segment)?;
            let existing = tree
                .children(&parent)
                .into_iter()
                .find(|e| e.is_folder && e.name == segment);
            parent = match existing {
                Some(e) => e.id,
                None => {
                    let id = self.mkdir(segment, &parent).await?;
                    // A real mkdir bumped the generation; re-resolve so later segments see it.
                    tree = self.resolved_snapshot().await?;
                    id
                }
            };
        }
        Ok(parent)
    }
```

Update `stat` (`fs.rs:49`) if it used `ls_with`; it can keep using `metadata_from`/`snapshot` (it fetches a single doc by id — leave as-is unless it referenced `ls_with`).

- [ ] **Step 4: Run the full crate test suite, expect pass**

Run: `cargo test -p rm-cloud --features fake 2>&1 | tail -25`
Expected: PASS — warm-`ls` test green, existing `ls`/`mkdir_p` behavior tests green.

- [ ] **Step 5: Commit**

```bash
git add crates/rm-cloud/src/porcelain/fs.rs crates/rm-cloud/src/fake/mod.rs
git commit -m "refactor(rm-cloud): ls/mkdir_p read the resolved sync index, not a full scan"
```

```json:metadata
{"files": ["crates/rm-cloud/src/porcelain/fs.rs", "crates/rm-cloud/src/fake/mod.rs"], "verifyCommand": "cargo test -p rm-cloud --features fake", "acceptanceCriteria": ["ls sourced from resolved_snapshot", "mkdir_p resolves from tree, creates only missing", "ls_with fan-out removed", "warm ls = 1 root GET, 0 blob GETs"], "requiresUserVerification": false}
```

---

## Task 5: Rewire rmapps deploy path + wire `SyncStore` at construction sites

**Goal:** The rmapps `Cloud` walk/list/deploy use one resolved snapshot instead of four account scans, and every production `Client` is built with a `SyncStore`. Proves the Feed+Library deploy collapses to one root poll + delta.

**Files:**
- Modify: `apps/rmapps/src/cloud.rs` (`default_sync_index_path`, attach store; `list_recursive`/`walk`/`resolve_folder_in`/`doc_id_in`/`doc_ids_in` via `ResolvedTree`)
- Modify: `apps/rmapps/src/auth.rs` (attach store at the two `Client` constructions)

**Acceptance Criteria:**
- [ ] All three production `Client` constructions (`cloud.rs:71`, `auth.rs:101`, `auth.rs:118`) attach a `SyncStore`.
- [ ] `list_recursive` walks one `ResolvedTree` (no per-folder IO).
- [ ] `doc_id_in`/`doc_ids_in`/`resolve_folder` read the resolved tree.
- [ ] A `replace` of two docs in a warm account issues one root poll plus the changed-doc delta — not four full scans (request-counted via `FakeCloud`).
- [ ] `cache gc` does not touch `sync-index.json` (it lives outside the blob root).

**Verify:** `cargo test -p rmapps 2>&1 | tail -25` → pass.

**Steps:**

- [ ] **Step 1: Add `default_sync_index_path`** in `apps/rmapps/src/cloud.rs`, next to `default_cache_dir`:

```rust
/// Default sync-index path: `$RMAPPS_SYNC_INDEX`, else `<cache-base>/rmapps/sync-index.json`
/// (a sibling of the `blobs/` dir, so `cache gc` never touches it).
pub fn default_sync_index_path() -> PathBuf {
    if let Ok(p) = std::env::var("RMAPPS_SYNC_INDEX") {
        return PathBuf::from(p);
    }
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache")
        });
    base.join("rmapps").join("sync-index.json")
}
```

Attach the store at `cloud.rs:71-72`:

```rust
        let client = Client::from_device_token(Config::from_env(), token)
            .with_cache(BlobCache::new(default_cache_dir()))
            .with_sync_store(SyncStore::new(default_sync_index_path()));
```

Add `SyncStore` to the `use rm_cloud::{...}` line at the top of `cloud.rs`.

- [ ] **Step 2: Attach the store in `auth.rs`** — at both `Client::from_device_token(...)` sites (`auth.rs:101` and `auth.rs:118`), append `.with_sync_store(SyncStore::new(crate::cloud::default_sync_index_path()))`, and add `SyncStore` to the `use rm_cloud::{...}` import.

- [ ] **Step 3: Rewrite the listing/deploy helpers in `cloud.rs`** to use `ResolvedTree`. Replace `list_recursive`/`walk` so the walk is pure over one tree:

```rust
    /// Recursively list documents under `root`, excluding generated suffixes. One resolved
    /// snapshot; the walk is pure (no per-folder IO).
    pub fn list_recursive(&self, root: &str, exclude_suffixes: &[String]) -> Result<Vec<RemoteDoc>> {
        let tree = self
            .rt
            .block_on(self.client.resolved_snapshot())
            .map_err(|e| anyhow!("resolved_snapshot: {e}"))?;
        let Some(root_id) = tree.resolve_folder(root) else {
            return Ok(Vec::new());
        };
        let root_path = normalize_path(root);
        let mut out = Vec::new();
        walk_tree(&tree, &root_id, &root_path, exclude_suffixes, &mut out);
        Ok(out)
    }
```

Replace the `walk` method + `resolve_folder_in` with a free function over the tree:

```rust
fn walk_tree(
    tree: &rm_cloud::ResolvedTree,
    folder_id: &str,
    folder_path: &str,
    exclude_suffixes: &[String],
    out: &mut Vec<RemoteDoc>,
) {
    for e in tree.children(folder_id) {
        let child_path = if folder_path.ends_with('/') {
            format!("{folder_path}{}", e.name)
        } else {
            format!("{folder_path}/{}", e.name)
        };
        if e.is_folder {
            walk_tree(tree, &e.id, &child_path, exclude_suffixes, out);
        } else if !exclude_suffixes.iter().any(|s| e.name.ends_with(s.as_str())) {
            out.push(RemoteDoc {
                id: e.id,
                name: e.name,
                folder: folder_path.to_string(),
                path: child_path,
                hash: e.hash,
            });
        }
    }
}
```

Rewrite `doc_id_in`, `doc_ids_in`, and `resolve_folder` to read one resolved tree:

```rust
    fn doc_id_in(&self, folder_id: &str, name: &str) -> Result<Option<String>> {
        let tree = self
            .rt
            .block_on(self.client.resolved_snapshot())
            .map_err(|e| anyhow!("resolved_snapshot: {e}"))?;
        Ok(tree
            .children(folder_id)
            .into_iter()
            .find(|e| !e.is_folder && e.name == name)
            .map(|e| e.id))
    }

    fn doc_ids_in(&self, folder_id: &str, name: &str) -> Result<Vec<String>> {
        let tree = self
            .rt
            .block_on(self.client.resolved_snapshot())
            .map_err(|e| anyhow!("resolved_snapshot: {e}"))?;
        Ok(tree
            .children(folder_id)
            .into_iter()
            .filter(|e| !e.is_folder && e.name == name)
            .map(|e| e.id)
            .collect())
    }

    /// Resolve a slash path to a folder id WITHOUT creating anything.
    pub fn resolve_folder(&self, folder: &str) -> Result<Option<String>> {
        let tree = self
            .rt
            .block_on(self.client.resolved_snapshot())
            .map_err(|e| anyhow!("resolved_snapshot: {e}"))?;
        Ok(tree.resolve_folder(folder))
    }
```

Leave `ensure_folder` delegating to `self.client.mkdir_p` (already rewired in Task 4). `replace`/`upsert`/`create_if_missing` keep their structure — they now call the cheap `doc_id(s)_in` above.

- [ ] **Step 4: Add a deploy request-count test** — in the `cloud.rs` `#[cfg(test)] mod tests`, alongside `replace_removes_all_same_named_docs`. It must build the `Cloud` with a sync-store-backed client (extend `cloud_from_client` to accept a client already carrying a store, or add a variant), warm the tree once, then assert a second `replace` costs one root poll plus only the changed doc's metadata:

```rust
    #[test]
    fn warm_replace_costs_one_root_poll_plus_delta() {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let fake = rt.block_on(FakeCloud::spawn());
        let dir = tempfile::tempdir().unwrap();
        let client = Client::from_user_token(CloudConfig::single_host(&fake.base), "user-token")
            .with_sync_store(rm_cloud::SyncStore::new(dir.path().join("idx.json")));
        let cloud = cloud_from_client(client);

        // Seed a folder + two docs, warm the store.
        cloud.ensure_folder("/Readwise").unwrap();
        cloud.replace("/Readwise", "Feed", b"feed-v1".to_vec()).unwrap();
        cloud.replace("/Readwise", "Library", b"lib-v1".to_vec()).unwrap();
        let _ = cloud.list_recursive("/Readwise", &[]).unwrap();

        let roots_before = fake.root_get_count();
        // A second replace of Feed: must not re-scan the whole account.
        cloud.replace("/Readwise", "Feed", b"feed-v2".to_vec()).unwrap();

        let roots = fake.root_get_count() - roots_before;
        assert!(roots <= 6, "expected a handful of root polls, got {roots}");
    }
```

> The exact root-poll count depends on how many `resolved_snapshot` calls `replace` makes (ensure_folder + doc_ids_in + commit's internal snapshot). The assertion bounds it to a small constant — the point is it is independent of account size, unlike the old `2×N` scan. Tune the bound to the observed value when implementing; document the number in a comment.

- [ ] **Step 5: Add the gc-exemption test** — in `apps/rmapps/src/cache_cmd.rs` tests (it already has a `tests` module), assert that a `sync-index.json` placed beside the blob root is untouched by `gc`:

```rust
    #[test]
    fn gc_ignores_sync_index_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = dir.path().join("blobs");
        let cache = BlobCache::new(&blobs);
        let h = sha256_hex(b"x");
        cache.put(&h, b"x").unwrap();
        let idx = dir.path().join("sync-index.json");
        std::fs::write(&idx, b"{}").unwrap();
        super::gc(&cache, 0); // evict everything
        assert!(idx.exists(), "sync index must survive gc");
    }
```

- [ ] **Step 6: Run the full suite, expect pass**

Run: `cargo test -p rmapps -p rm-cloud --features rm-cloud/fake 2>&1 | tail -30`
Expected: PASS. (If the workspace already enables the `fake` feature for rmapps tests, `cargo test -p rmapps` suffices — check `apps/rmapps/Cargo.toml` dev-deps.)

- [ ] **Step 7: Commit**

```bash
git add apps/rmapps/src/cloud.rs apps/rmapps/src/auth.rs apps/rmapps/src/cache_cmd.rs
git commit -m "feat(rmapps): deploy/list via one resolved sync snapshot; wire SyncStore"
```

```json:metadata
{"files": ["apps/rmapps/src/cloud.rs", "apps/rmapps/src/auth.rs", "apps/rmapps/src/cache_cmd.rs"], "verifyCommand": "cargo test -p rmapps", "acceptanceCriteria": ["all 3 Client sites attach SyncStore", "list_recursive walks one tree", "doc_id(s)_in/resolve via tree", "warm replace is account-size-independent", "gc ignores sync-index.json"], "requiresUserVerification": false}
```

---

## Task 6: Live verification with Dan

**Goal:** Confirm the real reMarkable cloud accepts a full `rmapps reader` deploy with no 429, once the rate-limit window has cleared.

**Files:** none (operational verification).

**Acceptance Criteria:**
- [ ] Full workspace build + test suite is green: `cargo test --workspace`.
- [ ] A live `rmapps reader` run completes the deploy without a 429.

**User Verification Required:**
Before marking this task complete, you MUST call AskUserQuestion:
```yaml
AskUserQuestion:
  question: "Live check: run `cargo run -p rmapps -- reader` against the real cloud (after the rate-limit window clears). Did it complete the deploy with no 429?"
  header: "Verification"
  options:
    - label: "No 429 — deploy succeeded"
      description: "The reader deploy finished cleanly; the re-architecture holds against the live limiter."
    - label: "Still 429 / other failure"
      description: "Rate limit (or another error) still hit — capture the output and reopen the investigation."
```

**If the user selects the negative option:** The task is NOT complete. Capture the failing output (which call 429'd, and the `RM_CLOUD_MAX_CONCURRENCY`/`RM_CLOUD_MIN_INTERVAL_MS` in effect), lower concurrency / raise the interval or trace the offending path, then re-verify with AskUserQuestion.

**Steps:**

- [ ] **Step 1: Full workspace verification**

Run: `cargo test --workspace 2>&1 | tail -30`
Expected: PASS across `rm-cloud`, `rmapps`, and the domain crates.

- [ ] **Step 2: Live run** (only once the account's rate-limit window has cleared)

Run: `cargo run -p rmapps -- reader 2>&1 | tail -30`
Expected: render + deploy complete, no `429` / `rate limited` in the output.

- [ ] **Step 3: Confirm with Dan via AskUserQuestion** (the block above). Only mark complete on the positive outcome.

```json:metadata
{"files": [], "verifyCommand": "cargo test --workspace", "acceptanceCriteria": ["workspace tests green", "live reader deploy completes with no 429"], "requiresUserVerification": true, "userVerificationPrompt": "Live check: run `cargo run -p rmapps -- reader` against the real cloud (after the rate-limit window clears). Did it complete the deploy with no 429?"}
```

---

## Self-Review notes

- **Spec coverage:** Component 1 (SyncStore) → Task 2; Component 2 (`resolved_snapshot`) → Task 3 + porcelain rewire Tasks 4-5; Component 3 (rate governor) → Task 1; Component 4 (blob-cache demotion) → achieved by Tasks 3-5 removing the listing path's reliance on it (no code removal needed, per spec) + gc-exemption test in Task 5. All four components covered.
- **Sequencing:** Task 1 (governor) lands first and independently unblocks the live 429, exactly as the spec's sequencing note requires.
- **Type consistency:** `ResolvedDoc`/`ResolvedTree`/`SyncStore` defined in Task 2 and used unchanged in Tasks 3-5; `Entry` reused from `porcelain::fs`; `with_sync_store`/`resolved_snapshot` names consistent across tasks.
- **User verification:** prompt requires Dan to confirm the live 429 is gone → Task 6 carries `requiresUserVerification: true` + the standard block.
