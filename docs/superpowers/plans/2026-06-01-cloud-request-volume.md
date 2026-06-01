# Cut reMarkable Cloud Request Volume — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the recurring full-library re-downloads and the O(folders×docs) `ls` fan-out that drive reMarkable cloud 429s, via a content-addressed disk blob cache, snapshot generation-keying, and wiring the dead digest cheap-skip.

**Architecture:** Every content-addressed read flows through one chokepoint, `Client::get_blob(hash, name)`. We add a hash-keyed disk cache there (immutable blobs, verified by re-hash), memoize the mutable snapshot keyed by the cheap `current_generation()` poll, and surface each doc's hash so the digest pipeline can skip unchanged documents before downloading them.

**Tech Stack:** Rust, tokio, reqwest, axum (the in-process `FakeCloud` test harness behind feature `fake`), sha2, serde.

**User Verification:** NO — no user verification required. This is a performance/caching change with no human-in-the-loop outcome to confirm. (Operational note only: live `rmapps digest`/`sync` and the gated `*_live.rs` tests must not be run until the account's rate-limit window clears; the automated suite is FakeCloud-only and safe to run now.)

---

## File Structure

| File | Responsibility | Tasks |
|---------------------------------------------------|----------------------------------------------------------|---------|
| `crates/rm-cloud/src/porcelain/fs.rs`             | Add `hash` to `Entry`; `ls_with(&snap, …)` variant       | 1, 7    |
| `apps/rmapps/src/cloud.rs`                         | Add `hash` to `RemoteDoc`; populate it in `walk`         | 1, 7    |
| `apps/rmapps/src/cloud_adapters.rs`               | Populate `CloudDoc.version` from the doc hash            | 1       |
| `crates/rmdigest/src/generate.rs`                 | Skip `fetch` for unchanged docs (cheap-skip)             | 2       |
| `crates/rm-cloud/src/fake/mod.rs`, `fake/handlers.rs` | Per-hash blob-GET counter for cache assertions       | 3       |
| `crates/rm-cloud/src/cache.rs` (new)              | Content-addressed disk store (`BlobCache`)               | 4       |
| `crates/rm-cloud/src/client.rs`                   | Cache field + read-through/write-through; snapshot memo  | 5, 6    |
| `crates/rm-cloud/src/lib.rs`                       | Export `BlobCache`                                       | 4       |
| `apps/rmapps/src/cache_cmd.rs` (new), `main.rs`   | `rmapps cache {gc,info,clear}`; wire default cache dir   | 8       |

---

### Task 1: Surface each document's hash through the listing path

**Goal:** Carry the cloud doc hash (already present in the snapshot) out through `ls` → `RemoteDoc` → `CloudDoc.version`, so the digest pipeline (Task 2) can compare it. No behavior change yet.

**Files:**
- Modify: `crates/rm-cloud/src/porcelain/fs.rs` (add `hash` to `Entry`, set it in `ls`)
- Modify: `apps/rmapps/src/cloud.rs` (add `hash` to `RemoteDoc`, set it in `walk`)
- Modify: `apps/rmapps/src/cloud_adapters.rs` (`version: Some(doc.hash)`)

**Acceptance Criteria:**
- [ ] `Entry` has a `hash: String` field populated from the snapshot doc hash.
- [ ] `RemoteDoc` has a `hash: String` field populated in `walk`.
- [ ] `CloudBackend::list` sets `version: Some(d.hash)` (no longer `None`).
- [ ] `cargo build -p rm-cloud -p rmapps` succeeds.

**Verify:** `cargo build -p rm-cloud -p rmapps` → builds clean.

**Steps:**

- [ ] **Step 1: Add `hash` to `Entry` and populate it in `ls`**

In `crates/rm-cloud/src/porcelain/fs.rs`, add the field to the struct:

```rust
/// One entry in a directory listing.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Document id.
    pub id: String,
    /// Visible name.
    pub name: String,
    /// Parent id ("" = root, "trash" = trash).
    pub parent: String,
    /// True if a folder (`CollectionType`).
    pub is_folder: bool,
    /// Cloud content hash of the document (changes when any blob changes).
    pub hash: String,
}
```

In `ls`, the spawned task already has `hash` in scope (it iterates `(id, hash)` from `snap.docs()`). Thread it through the joined result. Change the spawn closure and the collection loop:

```rust
        for (id, hash) in docs {
            let client = self.clone();
            let sem = sem.clone();
            set.spawn(async move {
                let _permit = sem.acquire_owned().await.expect("semaphore not closed");
                let meta = client.metadata_by(&hash, &id).await;
                (id, hash, meta)
            });
        }

        let mut out = Vec::new();
        while let Some(joined) = set.join_next().await {
            let (id, hash, meta) =
                joined.map_err(|e| crate::error::Error::Http(format!("ls join: {e}")))?;
            // Skip docs whose metadata can't be read rather than failing the whole listing.
            let Ok(meta) = meta else { continue };
            if meta.deleted || meta.parent != parent {
                continue;
            }
            out.push(Entry {
                id,
                name: meta.visible_name,
                parent: meta.parent,
                is_folder: meta.doc_type == "CollectionType",
                hash,
            });
        }
```

- [ ] **Step 2: Add `hash` to `RemoteDoc` and populate it in `walk`**

In `apps/rmapps/src/cloud.rs`, add the field:

```rust
pub struct RemoteDoc {
    /// Document id (UUID).
    pub id: String,
    /// Visible name (leaf).
    pub name: String,
    /// Parent folder path, e.g. `/Books/Author`.
    pub folder: String,
    /// Full path, e.g. `/Books/Author/Title`.
    pub path: String,
    /// Cloud content hash (used by the digest cheap-skip).
    pub hash: String,
}
```

In `walk`, set it when pushing the doc:

```rust
            } else if !exclude_suffixes.iter().any(|s| e.name.ends_with(s.as_str())) {
                out.push(RemoteDoc {
                    id: e.id,
                    name: e.name,
                    folder: folder_path.to_string(),
                    path: child_path,
                    hash: e.hash,
                });
            }
```

- [ ] **Step 3: Populate `CloudDoc.version` in the adapter**

In `apps/rmapps/src/cloud_adapters.rs`, change `CloudBackend::list`:

```rust
            .map(|d| CloudDoc {
                path: d.path,
                name: d.name,
                folder: d.folder,
                version: Some(d.hash),
            })
```

- [ ] **Step 4: Build**

Run: `cargo build -p rm-cloud -p rmapps`
Expected: builds clean (fix any other `Entry { … }` / `RemoteDoc { … }` literals the compiler flags — e.g. test fixtures — by adding `hash: String::new()` / `hash: "…".into()`).

- [ ] **Step 5: Commit**

```bash
git add crates/rm-cloud/src/porcelain/fs.rs apps/rmapps/src/cloud.rs apps/rmapps/src/cloud_adapters.rs
git commit -m "feat(rm-cloud): surface doc hash through ls/RemoteDoc/CloudDoc.version"
```

```json:metadata
{"files": ["crates/rm-cloud/src/porcelain/fs.rs", "apps/rmapps/src/cloud.rs", "apps/rmapps/src/cloud_adapters.rs"], "verifyCommand": "cargo build -p rm-cloud -p rmapps", "acceptanceCriteria": ["Entry has hash field", "RemoteDoc has hash field", "CloudBackend::list sets version: Some(hash)", "builds clean"], "requiresUserVerification": false}
```

---

### Task 2: Wire the digest cheap-skip (skip download for unchanged docs)

**Goal:** In `process_doc`, return before `backend.fetch` when the doc's hash matches the last successfully-processed hash — eliminating the full-bundle download for unchanged documents. This is the single biggest recurring win.

**Files:**
- Modify: `crates/rmdigest/src/generate.rs` (`process_doc`)
- Test: `crates/rmdigest/src/generate.rs` (`#[cfg(test)]` module — add a fetch-counting backend)

**Acceptance Criteria:**
- [ ] When `doc.version == prev.cloud_version` and `prev.page_hashes` is non-empty, `process_doc` returns `Ok(())` without calling `backend.fetch`.
- [ ] A first-sight doc (no prior state) still fetches and processes.
- [ ] A changed doc (different `version`) still fetches and processes.
- [ ] State persists `cloud_version` so the next run can skip.

**Verify:** `cargo test -p rmdigest cheap_skip` → 3 tests pass.

**Steps:**

- [ ] **Step 1: Write the failing tests (fetch-counting backend)**

Add to the `#[cfg(test)] mod tests` in `crates/rmdigest/src/generate.rs` (create the module if absent). The backend counts `fetch` calls and serves a tiny single-page bundle from a fixture dir; reuse the existing test fixture helpers in the crate if present, otherwise point `fetch` at a prebuilt `.rmdoc` under `tests/` — check `crates/rmdigest/src/ingest.rs` tests for the existing fixture path and reuse it.

```rust
#[cfg(test)]
mod cheap_skip_tests {
    use super::*;
    use crate::deploy::{Backend, CloudDoc};
    use std::cell::Cell;
    use std::path::{Path, PathBuf};

    /// Backend that counts fetches and serves a fixed bundle for any doc.
    struct CountingBackend {
        bundle: PathBuf,
        fetches: Cell<u32>,
    }
    impl Backend for CountingBackend {
        fn list(&self, _root: &str, _ex: &[String]) -> anyhow::Result<Vec<CloudDoc>> {
            Ok(vec![])
        }
        fn fetch(&self, _doc: &CloudDoc) -> anyhow::Result<Option<PathBuf>> {
            self.fetches.set(self.fetches.get() + 1);
            Ok(Some(self.bundle.clone()))
        }
        fn put(&self, _pdf: &Path, _folder: &str, _name: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn fixture_bundle() -> PathBuf {
        // Reuse the same single-page .rmdoc fixture ingest tests use.
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/one-page.rmdoc")
    }

    fn test_cfg() -> Config {
        // Minimal config; mirror the constructor ingest/render tests already use.
        Config::default()
    }

    #[test]
    fn cheap_skip_avoids_fetch_when_version_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let state_path = tmp.path().join("state.json");
        let cfg = test_cfg();
        let backend = CountingBackend { bundle: fixture_bundle(), fetches: Cell::new(0) };
        let doc = CloudDoc {
            path: "/Books/A".into(), name: "A".into(), folder: "/Books".into(),
            version: Some("hash-v1".into()),
        };
        let opts = Opts { dry_run: false, local_output: None };

        // First run: fetches + processes, persists cloud_version=hash-v1.
        run_one(&cfg, &backend, &state_path, &opts, &doc).unwrap();
        assert_eq!(backend.fetches.get(), 1, "first run must fetch");

        // Second run, same version: must NOT fetch.
        run_one(&cfg, &backend, &state_path, &opts, &doc).unwrap();
        assert_eq!(backend.fetches.get(), 1, "unchanged doc must be skipped before fetch");
    }

    #[test]
    fn changed_version_refetches() {
        let tmp = tempfile::tempdir().unwrap();
        let state_path = tmp.path().join("state.json");
        let cfg = test_cfg();
        let backend = CountingBackend { bundle: fixture_bundle(), fetches: Cell::new(0) };
        let opts = Opts { dry_run: false, local_output: None };
        let mut doc = CloudDoc {
            path: "/Books/A".into(), name: "A".into(), folder: "/Books".into(),
            version: Some("hash-v1".into()),
        };
        run_one(&cfg, &backend, &state_path, &opts, &doc).unwrap();
        doc.version = Some("hash-v2".into());
        run_one(&cfg, &backend, &state_path, &opts, &doc).unwrap();
        assert_eq!(backend.fetches.get(), 2, "changed doc must refetch");
    }

    #[test]
    fn first_sight_fetches() {
        let tmp = tempfile::tempdir().unwrap();
        let state_path = tmp.path().join("state.json");
        let cfg = test_cfg();
        let backend = CountingBackend { bundle: fixture_bundle(), fetches: Cell::new(0) };
        let opts = Opts { dry_run: false, local_output: None };
        let doc = CloudDoc {
            path: "/Books/A".into(), name: "A".into(), folder: "/Books".into(),
            version: Some("hash-v1".into()),
        };
        run_one(&cfg, &backend, &state_path, &opts, &doc).unwrap();
        assert_eq!(backend.fetches.get(), 1);
    }
}
```

Note: if `Config::default()` or the fixture path differs, adapt to the crate's existing test conventions (grep `crates/rmdigest/src/ingest.rs` and `generate.rs` for how other tests build a `Config` and locate fixtures). The behavioral assertions (fetch counts) are the contract.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p rmdigest cheap_skip`
Expected: FAIL — `cheap_skip_avoids_fetch_when_version_unchanged` fails with `fetches == 2` (current code always fetches).

- [ ] **Step 3: Add the cheap-skip guard in `process_doc`**

In `crates/rmdigest/src/generate.rs`, at the top of `process_doc`, before `backend.fetch`:

```rust
fn process_doc(
    cfg: &Config,
    backend: &dyn Backend,
    doc: &CloudDoc,
    state: &mut State,
    state_path: &Path,
    opts: &Opts,
) -> anyhow::Result<()> {
    let prev = state.docs.entry(doc.path.clone()).or_default();

    // Cheap-skip: if the cloud doc hash matches the last successfully-processed
    // hash and we have prior page state, nothing changed — skip the (expensive)
    // bundle download entirely. `version: None` (backends with no hash) never skips.
    if doc.version.is_some()
        && prev.cloud_version == doc.version
        && !prev.page_hashes.is_empty()
    {
        eprintln!("rmdigest: {} unchanged (cheap-skip, no fetch), skipping", doc.path);
        return Ok(());
    }

    let bundle_path = match backend.fetch(doc)? {
        Some(p) => p,
        None => {
            eprintln!("rmdigest: fetch returned None for {}, skipping", doc.path);
            return Ok(());
        }
    };
    // … unchanged below …
```

Confirm the existing post-upload line `prev.cloud_version = doc.version.clone();` is present (it is). Because `prev` is a mutable borrow taken at the top, ensure the early-return drops it cleanly (it does — the borrow ends at return).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p rmdigest cheap_skip`
Expected: PASS (3 tests).

- [ ] **Step 5: Full crate test + commit**

```bash
cargo test -p rmdigest
git add crates/rmdigest/src/generate.rs
git commit -m "feat(rmdigest): cheap-skip unchanged docs before bundle download"
```

```json:metadata
{"files": ["crates/rmdigest/src/generate.rs"], "verifyCommand": "cargo test -p rmdigest cheap_skip", "acceptanceCriteria": ["unchanged doc skips before fetch", "changed doc refetches", "first-sight fetches", "cloud_version persisted"], "requiresUserVerification": false}
```

---

### Task 3: Add a per-hash blob-GET counter to `FakeCloud`

**Goal:** Let cache tests assert "the second read of a hash hits disk, not the network." Add a counter to the fake's blob-GET handler and a `FakeCloud::blob_get_count(hash)` helper.

**Files:**
- Modify: `crates/rm-cloud/src/fake/mod.rs` (`State` field + helper)
- Modify: `crates/rm-cloud/src/fake/handlers.rs` (increment in blob GET)

**Acceptance Criteria:**
- [ ] `State` has `blob_gets: HashMap<String, u32>`.
- [ ] The blob GET handler increments `blob_gets[hash]` on each served GET.
- [ ] `FakeCloud::blob_get_count(&self, hash: &str) -> u32` returns the count (0 if absent).
- [ ] Existing fake-backed tests still pass.

**Verify:** `cargo test -p rm-cloud --features fake` → existing suite still green.

**Steps:**

- [ ] **Step 1: Add the counter field**

In `crates/rm-cloud/src/fake/mod.rs`, add to `State`:

```rust
    /// Per-hash count of blob GETs served (test assertion of cache effectiveness).
    pub blob_gets: HashMap<String, u32>,
```

(`HashMap` is already imported.) Add the helper to `impl FakeCloud`:

```rust
    /// Number of blob GETs served for `hash` (test helper).
    pub fn blob_get_count(&self, hash: &str) -> u32 {
        self.state.lock().unwrap().blob_gets.get(hash).copied().unwrap_or(0)
    }
```

- [ ] **Step 2: Increment in the blob GET handler**

In `crates/rm-cloud/src/fake/handlers.rs`, find the GET handler for a blob by hash (the route serving `config.blob(hash)`). Immediately before returning the blob bytes, increment:

```rust
    {
        let mut st = state.lock().unwrap();
        *st.blob_gets.entry(hash.clone()).or_insert(0) += 1;
    }
```

Match the handler's existing variable names for `state` and the path-extracted `hash` (grep the file for the blob-GET route to get exact identifiers).

- [ ] **Step 3: Verify existing suite**

Run: `cargo test -p rm-cloud --features fake`
Expected: PASS (no regressions).

- [ ] **Step 4: Commit**

```bash
git add crates/rm-cloud/src/fake/mod.rs crates/rm-cloud/src/fake/handlers.rs
git commit -m "test(rm-cloud): per-hash blob-GET counter in FakeCloud"
```

```json:metadata
{"files": ["crates/rm-cloud/src/fake/mod.rs", "crates/rm-cloud/src/fake/handlers.rs"], "verifyCommand": "cargo test -p rm-cloud --features fake", "acceptanceCriteria": ["State.blob_gets exists", "GET handler increments", "blob_get_count helper", "no regressions"], "requiresUserVerification": false}
```

---

### Task 4: Implement the `BlobCache` content-addressed disk store

**Goal:** A standalone, hash-keyed disk store: `get` (read + sha256 verify, discard corrupt), `put` (atomic write), sharded layout. Pure unit tests with tempdir; no `Client` wiring yet.

**Files:**
- Create: `crates/rm-cloud/src/cache.rs`
- Modify: `crates/rm-cloud/src/lib.rs` (`mod cache; pub use cache::BlobCache;`)
- Modify: `crates/rm-cloud/Cargo.toml` (ensure `sha2` is a dependency — check first; it is likely already present for hashing)

**Acceptance Criteria:**
- [ ] `BlobCache::new(root: PathBuf)` creates the store rooted at `root`.
- [ ] `put(hash, &bytes)` writes atomically to `<root>/<first2hex>/<hash>`.
- [ ] `get(hash) -> Option<Vec<u8>>` returns bytes on hit, `None` on miss.
- [ ] `get` returns `None` and removes the file when stored bytes' sha256 ≠ hash.
- [ ] `total_size()` and `entries()` report disk usage for the gc CLI.

**Verify:** `cargo test -p rm-cloud cache::` → all pass.

**Steps:**

- [ ] **Step 1: Confirm `sha2` is available**

Run: `grep -n "sha2" crates/rm-cloud/Cargo.toml crates/rm-cloud/src/plumbing/index.rs`
Expected: `sha2` already used (the crate computes `sha256_hex` in `plumbing/index.rs`). If `index.rs` exposes a `sha256_hex(&[u8]) -> String`, reuse it; otherwise use `sha2::Sha256` directly. The steps below assume a local `fn sha256_hex`.

- [ ] **Step 2: Write the failing tests**

Create `crates/rm-cloud/src/cache.rs` with tests first:

```rust
//! Content-addressed on-disk blob cache. Blobs are immutable-by-hash, so the hash is a
//! perfect cache key and a stored entry is verified by re-hashing on read.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// A content-addressed blob store under a single root directory.
#[derive(Debug, Clone)]
pub struct BlobCache {
    root: PathBuf,
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

impl BlobCache {
    /// Create a cache rooted at `root` (created on first write).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Path of the entry for `hash`: `<root>/<first-2-hex>/<hash>`.
    fn path_for(&self, hash: &str) -> PathBuf {
        let shard = if hash.len() >= 2 { &hash[0..2] } else { "00" };
        self.root.join(shard).join(hash)
    }

    /// Read the blob for `hash`. Returns `None` on miss or if the stored bytes are
    /// corrupt (sha256 ≠ hash), removing the corrupt entry.
    pub fn get(&self, hash: &str) -> Option<Vec<u8>> {
        let path = self.path_for(hash);
        let bytes = fs::read(&path).ok()?;
        if sha256_hex(&bytes) == hash {
            Some(bytes)
        } else {
            let _ = fs::remove_file(&path);
            None
        }
    }

    /// Write `bytes` under `hash` atomically (temp file + rename within the shard dir).
    pub fn put(&self, hash: &str, bytes: &[u8]) -> std::io::Result<()> {
        let path = self.path_for(hash);
        let dir = path.parent().expect("entry path always has a parent");
        fs::create_dir_all(dir)?;
        let tmp = dir.join(format!(".{hash}.tmp"));
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(bytes)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Total bytes stored (for `cache info`/`gc`).
    pub fn total_size(&self) -> u64 {
        self.entries().iter().map(|(_, len, _)| *len).sum()
    }

    /// All entries as `(path, len_bytes, modified)` for gc/info. Skips temp files.
    pub fn entries(&self) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
        let mut out = Vec::new();
        let Ok(shards) = fs::read_dir(&self.root) else { return out };
        for shard in shards.flatten() {
            let Ok(files) = fs::read_dir(shard.path()) else { continue };
            for f in files.flatten() {
                let name = f.file_name();
                if name.to_string_lossy().ends_with(".tmp") {
                    continue;
                }
                if let Ok(md) = f.metadata() {
                    let mtime = md.modified().unwrap_or(std::time::UNIX_EPOCH);
                    out.push((f.path(), md.len(), mtime));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache() -> (tempfile::TempDir, BlobCache) {
        let dir = tempfile::tempdir().unwrap();
        let c = BlobCache::new(dir.path());
        (dir, c)
    }

    #[test]
    fn put_then_get_round_trips() {
        let (_d, c) = cache();
        let bytes = b"hello world".to_vec();
        let hash = sha256_hex(&bytes);
        c.put(&hash, &bytes).unwrap();
        assert_eq!(c.get(&hash), Some(bytes));
    }

    #[test]
    fn miss_returns_none() {
        let (_d, c) = cache();
        assert_eq!(c.get(&sha256_hex(b"absent")), None);
    }

    #[test]
    fn corrupt_entry_is_rejected_and_removed() {
        let (_d, c) = cache();
        let bytes = b"correct".to_vec();
        let hash = sha256_hex(&bytes);
        c.put(&hash, &bytes).unwrap();
        // Corrupt the stored file in place.
        let path = c.path_for(&hash);
        std::fs::write(&path, b"tampered").unwrap();
        assert_eq!(c.get(&hash), None, "corrupt entry must miss");
        assert!(!path.exists(), "corrupt entry must be removed");
    }

    #[test]
    fn total_size_sums_entries() {
        let (_d, c) = cache();
        for s in [b"a".as_slice(), b"bb", b"ccc"] {
            let h = sha256_hex(s);
            c.put(&h, s).unwrap();
        }
        assert_eq!(c.total_size(), 6);
    }
}
```

- [ ] **Step 3: Export from the crate**

In `crates/rm-cloud/src/lib.rs`, add near the other module declarations / re-exports:

```rust
mod cache;
pub use cache::BlobCache;
```

Confirm `hex` is a dependency (used above and elsewhere in the crate — `index.rs` uses hex encoding). If `sha256_hex` already exists in `plumbing::index` and is `pub(crate)`, you may import it instead of redefining; the local copy here is self-contained and acceptable.

- [ ] **Step 4: Run tests**

Run: `cargo test -p rm-cloud cache::`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/rm-cloud/src/cache.rs crates/rm-cloud/src/lib.rs crates/rm-cloud/Cargo.toml
git commit -m "feat(rm-cloud): content-addressed BlobCache disk store"
```

```json:metadata
{"files": ["crates/rm-cloud/src/cache.rs", "crates/rm-cloud/src/lib.rs"], "verifyCommand": "cargo test -p rm-cloud cache::", "acceptanceCriteria": ["put/get round-trips", "miss returns None", "corrupt entry rejected+removed", "total_size sums"], "requiresUserVerification": false}
```

---

### Task 5: Wire `BlobCache` into `Client::get_blob` / `put_blob`

**Goal:** Add an optional cache to `Client`; `get_blob` reads through it (serve on hit, write-through on miss), `put_blob` writes through. Default constructors stay cacheless; add a builder to attach a cache.

**Files:**
- Modify: `crates/rm-cloud/src/client.rs` (cache field, `with_cache`, `get_blob`/`put_blob`)
- Test: `crates/rm-cloud/src/client.rs` (`#[cfg(all(test, feature = "fake"))]`)

**Acceptance Criteria:**
- [ ] `Client` holds `cache: Option<Arc<BlobCache>>` (cheap to `Clone`).
- [ ] `Client::with_cache(self, BlobCache) -> Self` attaches a cache.
- [ ] `get_blob` returns a cached hit without a network GET; on miss it fetches and writes through.
- [ ] `put_blob` writes the uploaded bytes to the cache.
- [ ] Cacheless clients behave exactly as before.

**Verify:** `cargo test -p rm-cloud --features fake cache_integration` → passes.

**Steps:**

- [ ] **Step 1: Add the cache field and builder**

In `crates/rm-cloud/src/client.rs`, add to the struct and constructor:

```rust
use crate::cache::BlobCache;

#[derive(Clone)]
pub struct Client {
    pub(crate) http: reqwest::Client,
    pub(crate) config: Config,
    pub(crate) creds: Arc<RwLock<Credentials>>,
    pub(crate) cache: Option<Arc<BlobCache>>,
}
```

In `fn new`, initialize `cache: None`. Add the builder in `impl Client`:

```rust
    /// Attach a content-addressed disk cache. All blob reads/writes route through it.
    pub fn with_cache(mut self, cache: BlobCache) -> Self {
        self.cache = Some(Arc::new(cache));
        self
    }
```

- [ ] **Step 2: Route `get_blob` / `put_blob` through the cache**

Replace the two methods in `client.rs`:

```rust
    /// GET a blob by hash and logical filename, served from the disk cache when present.
    pub(crate) async fn get_blob(&self, hash: &str, name: &str) -> Result<Vec<u8>> {
        if let Some(cache) = &self.cache {
            if let Some(bytes) = cache.get(hash) {
                return Ok(bytes);
            }
        }
        let token = self.user_token().await?;
        let bytes = get_blob(&self.http, &self.config.blob(hash), &token, name).await?;
        if let Some(cache) = &self.cache {
            // Best-effort: a cache write failure must not fail the request.
            let _ = cache.put(hash, &bytes);
        }
        Ok(bytes)
    }

    /// PUT a blob under `hash`; also write-through to the disk cache.
    pub(crate) async fn put_blob(&self, hash: &str, name: &str, bytes: Vec<u8>) -> Result<()> {
        let token = self.user_token().await?;
        put_blob(&self.http, &self.config.blob(hash), &token, name, bytes.clone()).await?;
        if let Some(cache) = &self.cache {
            let _ = cache.put(hash, &bytes);
        }
        Ok(())
    }
```

- [ ] **Step 3: Write the failing integration test**

Add to `client.rs`:

```rust
#[cfg(all(test, feature = "fake"))]
mod cache_integration {
    use super::*;
    use crate::cache::BlobCache;
    use crate::fake::FakeCloud;

    #[tokio::test]
    async fn second_read_hits_cache_not_network() {
        let fake = FakeCloud::spawn().await;
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::for_base(&fake.base); // construct Config pointing at the fake
        let client = Client::from_user_token(cfg, "test-token")
            .with_cache(BlobCache::new(dir.path()));

        // Seed a blob in the fake and read it twice through the client.
        let bytes = b"blobby".to_vec();
        let hash = crate::plumbing::index::sha256_hex(&bytes);
        fake.state.lock().unwrap().blobs.insert(hash.clone(), bytes.clone());

        let a = client.get_blob(&hash, "x").await.unwrap();
        let b = client.get_blob(&hash, "x").await.unwrap();
        assert_eq!(a, bytes);
        assert_eq!(b, bytes);
        assert_eq!(fake.blob_get_count(&hash), 1, "second read must be served from cache");
    }
}
```

Adapt `Config::for_base` to however the existing fake-backed tests point `Config` at `fake.base` (grep `crates/rm-cloud/src/` tests for `fake.base` / `Config` construction and reuse that exact pattern). Use the crate's real `sha256_hex` path.

- [ ] **Step 4: Run — fail then pass**

Run: `cargo test -p rm-cloud --features fake cache_integration`
Expected first: compile/fail until Steps 1-2 are in; after: PASS, `blob_get_count == 1`.

- [ ] **Step 5: Full suite + commit**

```bash
cargo test -p rm-cloud --features fake
git add crates/rm-cloud/src/client.rs
git commit -m "feat(rm-cloud): route get_blob/put_blob through optional BlobCache"
```

```json:metadata
{"files": ["crates/rm-cloud/src/client.rs"], "verifyCommand": "cargo test -p rm-cloud --features fake cache_integration", "acceptanceCriteria": ["Client has cache field", "with_cache builder", "cache hit avoids GET", "put write-through", "cacheless unchanged"], "requiresUserVerification": false}
```

---

### Task 6: Memoize the snapshot, keyed by generation

**Goal:** Avoid re-downloading the root-index blob when the account hasn't changed. `snapshot()` polls the cheap `current_generation()`; if it matches the cached snapshot's generation, reuse it.

**Files:**
- Modify: `crates/rm-cloud/src/client.rs` (cached-snapshot field + `snapshot()` logic)
- Test: `crates/rm-cloud/src/client.rs` (`#[cfg(all(test, feature = "fake"))]`)

**Acceptance Criteria:**
- [ ] `Client` holds a cached snapshot (`Arc<RwLock<Option<Snapshot>>>`).
- [ ] `snapshot()` reuses the cached snapshot when `current_generation()` equals its generation.
- [ ] When the generation changed (or no cache), `snapshot()` rebuilds and stores it.
- [ ] Two `snapshot()` calls at the same generation fetch the root-index blob only once.

**Verify:** `cargo test -p rm-cloud --features fake snapshot_memo` → passes.

**Steps:**

- [ ] **Step 1: Add the cached-snapshot field**

In `client.rs`, add to the struct and `new`:

```rust
    pub(crate) snap_cache: Arc<RwLock<Option<Snapshot>>>,
```

Initialize `snap_cache: Arc::new(RwLock::new(None))` in `fn new`.

- [ ] **Step 2: Rework `snapshot()` to be generation-keyed**

Replace `snapshot()`:

```rust
    /// Fetch the current account snapshot, reusing a cached one when the account's
    /// generation is unchanged (cheap root-ref poll), else rebuilding it.
    pub async fn snapshot(&self) -> Result<Snapshot> {
        let current_gen = self.current_generation().await?;

        // Reuse cache when the generation matches.
        if let Some(gen) = current_gen {
            if let Some(snap) = self.snap_cache.read().await.as_ref() {
                if snap.generation == gen {
                    return Ok(snap.clone());
                }
            }
        }

        // Rebuild from the root ref. `None` => account never synced.
        let root = match self.get_root_ref().await {
            Err(Error::Unauthorized) => {
                self.force_refresh().await?;
                self.get_root_ref().await?
            }
            other => other?,
        };
        let Some(root) = root else {
            return Ok(Snapshot::empty());
        };
        let bytes = self.get_blob(&root.hash, "root.docSchema").await?;
        let snap = Snapshot::from_root_index(root.generation, root.hash, &bytes)?;
        *self.snap_cache.write().await = Some(snap.clone());
        Ok(snap)
    }
```

Note: `current_generation()` already does the 401-refresh dance; the second `get_root_ref` here covers the race where the gen poll succeeded but the cache missed. The root-index blob fetch is itself cache-backed (Task 5), so an unchanged-but-evicted snapshot still avoids the network when the blob is on disk.

- [ ] **Step 3: Write the failing test**

```rust
#[cfg(all(test, feature = "fake"))]
mod snapshot_memo {
    use super::*;
    use crate::cache::BlobCache;
    use crate::fake::FakeCloud;
    use crate::porcelain::docfiles::DocFiles;

    #[tokio::test]
    async fn unchanged_generation_reuses_snapshot() {
        let fake = FakeCloud::spawn().await;
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::for_base(&fake.base);
        let client = Client::from_user_token(cfg, "test-token")
            .with_cache(BlobCache::new(dir.path()));

        // Create one doc so a real root index exists.
        client.put(DocFiles::new_pdf("Doc", "", b"%PDF-1.4\n".to_vec())).await.unwrap();

        let s1 = client.snapshot().await.unwrap();
        let root_hash = s1.root_hash.clone();
        let gets_after_first = fake.blob_get_count(&root_hash);

        let s2 = client.snapshot().await.unwrap();
        assert_eq!(s1.generation, s2.generation);
        assert_eq!(
            fake.blob_get_count(&root_hash), gets_after_first,
            "unchanged generation must not refetch the root-index blob"
        );
    }
}
```

- [ ] **Step 4: Run — fail then pass**

Run: `cargo test -p rm-cloud --features fake snapshot_memo`
Expected: PASS after Steps 1-2.

- [ ] **Step 5: Full suite + commit**

```bash
cargo test -p rm-cloud --features fake
git add crates/rm-cloud/src/client.rs
git commit -m "feat(rm-cloud): generation-keyed snapshot memoization"
```

```json:metadata
{"files": ["crates/rm-cloud/src/client.rs"], "verifyCommand": "cargo test -p rm-cloud --features fake snapshot_memo", "acceptanceCriteria": ["snap_cache field", "reuse on matching generation", "rebuild on change", "root-index fetched once when unchanged"], "requiresUserVerification": false}
```

---

### Task 7: Snapshot-once recursive walk

**Goal:** Stop re-snapshotting inside every `ls` during a recursive listing. Add `ls_with(&snap, parent)` and refactor `walk`/`list_recursive` to take one snapshot and reuse it across all folders — turning N generation-polls into 1 and (with Task 5) serving all metadata from cache.

**Files:**
- Modify: `crates/rm-cloud/src/porcelain/fs.rs` (`ls_with`)
- Modify: `apps/rmapps/src/cloud.rs` (`list_recursive`/`walk` snapshot once)
- Test: `crates/rm-cloud/src/porcelain/fs.rs` or an rmapps test against the fake

**Acceptance Criteria:**
- [ ] `Client::ls_with(&self, snap: &Snapshot, parent: &str) -> Result<Vec<Entry>>` exists; public `ls` delegates (`let snap = self.snapshot().await?; self.ls_with(&snap, parent).await`).
- [ ] `list_recursive`/`walk` snapshot once and pass `&Snapshot` to each folder listing.
- [ ] A recursive list over an F-folder tree triggers one snapshot's worth of root-index reads, not F.
- [ ] Listing results are unchanged (same entries, same order).

**Verify:** `cargo test -p rm-cloud --features fake ls_with && cargo test -p rmapps` → pass.

**Steps:**

- [ ] **Step 1: Extract `ls_with` from `ls`**

In `crates/rm-cloud/src/porcelain/fs.rs`, split `ls` so the snapshot is a parameter:

```rust
    /// List direct children of `parent` against an already-fetched snapshot.
    pub async fn ls_with(&self, snap: &Snapshot, parent: &str) -> Result<Vec<Entry>> {
        let docs: Vec<(String, String)> = snap
            .docs()
            .map(|d| (d.id.clone(), d.hash.clone()))
            .collect();

        let sem = Arc::new(Semaphore::new(LS_CONCURRENCY));
        let mut set = tokio::task::JoinSet::new();
        for (id, hash) in docs {
            let client = self.clone();
            let sem = sem.clone();
            set.spawn(async move {
                let _permit = sem.acquire_owned().await.expect("semaphore not closed");
                let meta = client.metadata_by(&hash, &id).await;
                (id, hash, meta)
            });
        }

        let mut out = Vec::new();
        while let Some(joined) = set.join_next().await {
            let (id, hash, meta) =
                joined.map_err(|e| crate::error::Error::Http(format!("ls join: {e}")))?;
            let Ok(meta) = meta else { continue };
            if meta.deleted || meta.parent != parent {
                continue;
            }
            out.push(Entry {
                id, name: meta.visible_name, parent: meta.parent,
                is_folder: meta.doc_type == "CollectionType", hash,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// List the direct children of `parent` ("" = root). Snapshots, then delegates.
    pub async fn ls(&self, parent: &str) -> Result<Vec<Entry>> {
        let snap = self.snapshot().await?;
        self.ls_with(&snap, parent).await
    }
```

Add `use crate::plumbing::snapshot::Snapshot;` to `fs.rs` if not already imported.

- [ ] **Step 2: Snapshot once in `walk`/`list_recursive`**

In `apps/rmapps/src/cloud.rs`, thread one snapshot through `walk`. Since `walk` runs on the blocking runtime via `block_on`, fetch the snapshot once at the top of `list_recursive` and pass `&Snapshot` down, calling `ls_with`:

```rust
    pub fn list_recursive(&self, root: &str, exclude_suffixes: &[String]) -> Result<Vec<RemoteDoc>> {
        let snap = self
            .rt
            .block_on(self.client.snapshot())
            .map_err(|e| anyhow!("snapshot: {e}"))?;
        let Some(root_id) = self.resolve_folder_in(&snap, root)? else {
            return Ok(Vec::new());
        };
        let root_path = normalize_path(root);
        let mut out = Vec::new();
        self.walk(&snap, &root_id, &root_path, exclude_suffixes, &mut out)?;
        Ok(out)
    }

    fn walk(
        &self,
        snap: &rm_cloud::Snapshot,
        folder_id: &str,
        folder_path: &str,
        exclude_suffixes: &[String],
        out: &mut Vec<RemoteDoc>,
    ) -> Result<()> {
        let entries = self
            .rt
            .block_on(self.client.ls_with(snap, folder_id))
            .map_err(|e| anyhow!("ls {folder_path}: {e}"))?;
        for e in entries {
            let child_path = if folder_path.ends_with('/') {
                format!("{folder_path}{}", e.name)
            } else {
                format!("{folder_path}/{}", e.name)
            };
            if e.is_folder {
                self.walk(snap, &e.id, &child_path, exclude_suffixes, out)?;
            } else if !exclude_suffixes.iter().any(|s| e.name.ends_with(s.as_str())) {
                out.push(RemoteDoc {
                    id: e.id, name: e.name, folder: folder_path.to_string(),
                    path: child_path, hash: e.hash,
                });
            }
        }
        Ok(())
    }
```

Add a snapshot-reusing folder resolver used by `list_recursive` (leave the public `resolve_folder` as-is for other callers):

```rust
    /// Resolve a slash path to a folder id against an existing snapshot (no extra root fetch).
    fn resolve_folder_in(&self, snap: &rm_cloud::Snapshot, folder: &str) -> Result<Option<String>> {
        let mut parent = String::new();
        for seg in folder.split('/').filter(|s| !s.is_empty()) {
            let entries = self
                .rt
                .block_on(self.client.ls_with(snap, &parent))
                .map_err(|e| anyhow!("ls {parent:?}: {e}"))?;
            match entries.into_iter().find(|e| e.is_folder && e.name == seg) {
                Some(e) => parent = e.id,
                None => return Ok(None),
            }
        }
        Ok(Some(parent))
    }
```

Ensure `rm_cloud::Snapshot` is exported (it is — `pub use` in `lib.rs`; if not, add it).

- [ ] **Step 3: Write the test (one root-index read across a multi-folder tree)**

Add a fake-backed test (in `crates/rm-cloud/src/porcelain/fs.rs` under `#[cfg(all(test, feature = "fake"))]`, or in `apps/rmapps/tests/`). Build a tree (root → FolderA, FolderB; a doc in each), then assert that across the recursive walk the root-index blob is fetched once:

```rust
#[cfg(all(test, feature = "fake"))]
mod ls_with_tests {
    use crate::cache::BlobCache;
    use crate::client::{Client, Config};
    use crate::fake::FakeCloud;
    use crate::porcelain::docfiles::DocFiles;

    #[tokio::test]
    async fn recursive_listing_reuses_one_snapshot() {
        let fake = FakeCloud::spawn().await;
        let dir = tempfile::tempdir().unwrap();
        let client = Client::from_user_token(Config::for_base(&fake.base), "t")
            .with_cache(BlobCache::new(dir.path()));

        let a = client.mkdir("FolderA", "").await.unwrap();
        let b = client.mkdir("FolderB", "").await.unwrap();
        client.put(DocFiles::new_pdf("DocA", &a, b"%PDF\n".to_vec())).await.unwrap();
        client.put(DocFiles::new_pdf("DocB", &b, b"%PDF\n".to_vec())).await.unwrap();

        let snap = client.snapshot().await.unwrap();
        let root_hash = snap.root_hash.clone();
        let before = fake.blob_get_count(&root_hash);

        // Walk: list root, then each folder — all against the one snapshot.
        let _ = client.ls_with(&snap, "").await.unwrap();
        let _ = client.ls_with(&snap, &a).await.unwrap();
        let _ = client.ls_with(&snap, &b).await.unwrap();

        assert_eq!(
            fake.blob_get_count(&root_hash), before,
            "ls_with must not refetch the root index per folder"
        );
    }
}
```

- [ ] **Step 4: Run — fail then pass**

Run: `cargo test -p rm-cloud --features fake ls_with && cargo build -p rmapps && cargo test -p rmapps`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rm-cloud/src/porcelain/fs.rs apps/rmapps/src/cloud.rs
git commit -m "perf(rm-cloud): snapshot-once recursive walk via ls_with"
```

```json:metadata
{"files": ["crates/rm-cloud/src/porcelain/fs.rs", "apps/rmapps/src/cloud.rs"], "verifyCommand": "cargo test -p rm-cloud --features fake ls_with", "acceptanceCriteria": ["ls_with(&snap,parent) exists", "ls delegates", "walk snapshots once", "no per-folder root refetch", "results unchanged"], "requiresUserVerification": false}
```

---

### Task 8: Wire the default cache dir + `rmapps cache` CLI

**Goal:** Construct the `rmapps` cloud client with a disk cache at `~/.cache/rmapps/blobs`, and add a `rmapps cache {info,gc,clear}` subcommand for visibility and lazy eviction.

**Files:**
- Modify: `apps/rmapps/src/cloud.rs` (`from_device_token` attaches the cache)
- Create: `apps/rmapps/src/cache_cmd.rs` (`info`/`gc`/`clear`)
- Modify: `apps/rmapps/src/main.rs` (register the subcommand)

**Acceptance Criteria:**
- [ ] `Cloud::from_device_token` builds the `Client` with `.with_cache(BlobCache::new(<cache_dir>))`.
- [ ] Cache dir defaults to `$XDG_CACHE_HOME/rmapps/blobs` else `~/.cache/rmapps/blobs`; overridable via `RMAPPS_CACHE_DIR`.
- [ ] `rmapps cache info` prints entry count + total size.
- [ ] `rmapps cache gc --max-size <BYTES>` evicts oldest-by-mtime entries until under the cap; prints bytes/▢entries freed.
- [ ] `rmapps cache clear` removes the store.

**Verify:** `cargo test -p rmapps cache_cmd && cargo build -p rmapps` → pass; `rmapps cache info` runs.

**Steps:**

- [ ] **Step 1: Cache-dir helper + attach to the client**

In `apps/rmapps/src/cloud.rs`:

```rust
use rm_cloud::BlobCache;

/// Default blob-cache directory: `$RMAPPS_CACHE_DIR`, else `$XDG_CACHE_HOME/rmapps/blobs`,
/// else `~/.cache/rmapps/blobs`.
pub fn default_cache_dir() -> PathBuf {
    if let Ok(d) = std::env::var("RMAPPS_CACHE_DIR") {
        return PathBuf::from(d);
    }
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache")
        });
    base.join("rmapps").join("blobs")
}
```

In `from_device_token`, attach the cache:

```rust
        let client = Client::from_device_token(Config::from_env(), token)
            .with_cache(BlobCache::new(default_cache_dir()));
```

- [ ] **Step 2: Implement `cache_cmd.rs`**

Create `apps/rmapps/src/cache_cmd.rs`:

```rust
//! `rmapps cache` — inspect and prune the content-addressed blob cache.

use anyhow::Result;
use clap::{Args, Subcommand};
use rm_cloud::BlobCache;

use crate::cloud::default_cache_dir;

#[derive(Args)]
pub struct CacheArgs {
    #[command(subcommand)]
    cmd: CacheCmd,
}

#[derive(Subcommand)]
enum CacheCmd {
    /// Print entry count and total size.
    Info,
    /// Evict oldest entries until the store is under --max-size bytes.
    Gc {
        /// Maximum total size in bytes (default 3 GiB).
        #[arg(long, default_value_t = 3 * 1024 * 1024 * 1024)]
        max_size: u64,
    },
    /// Remove the entire cache.
    Clear,
}

pub fn run(args: CacheArgs) -> Result<()> {
    let dir = default_cache_dir();
    let cache = BlobCache::new(&dir);
    match args.cmd {
        CacheCmd::Info => {
            let entries = cache.entries();
            let total: u64 = entries.iter().map(|(_, len, _)| *len).sum();
            println!("cache: {}\n  entries: {}\n  size: {:.1} MiB",
                dir.display(), entries.len(), total as f64 / (1024.0 * 1024.0));
        }
        CacheCmd::Gc { max_size } => {
            let mut entries = cache.entries();
            let mut total: u64 = entries.iter().map(|(_, len, _)| *len).sum();
            // Oldest first (LRU by mtime).
            entries.sort_by_key(|(_, _, mtime)| *mtime);
            let mut freed = 0u64;
            let mut removed = 0usize;
            for (path, len, _) in entries {
                if total <= max_size {
                    break;
                }
                if std::fs::remove_file(&path).is_ok() {
                    total -= len;
                    freed += len;
                    removed += 1;
                }
            }
            println!("gc: freed {:.1} MiB across {} entries",
                freed as f64 / (1024.0 * 1024.0), removed);
        }
        CacheCmd::Clear => {
            if dir.exists() {
                std::fs::remove_dir_all(&dir)?;
            }
            println!("cache cleared: {}", dir.display());
        }
    }
    Ok(())
}
```

- [ ] **Step 3: Register the subcommand in `main.rs`**

In `apps/rmapps/src/main.rs`, add `mod cache_cmd;`, add a `Cache(cache_cmd::CacheArgs)` variant to the top-level command enum, and dispatch `Commands::Cache(a) => cache_cmd::run(a)`. Match the existing clap structure exactly (grep `main.rs` for the `enum Commands`/`match` to copy the surrounding pattern).

- [ ] **Step 4: Write the gc test**

Add a `#[cfg(test)] mod tests` in `cache_cmd.rs` exercising eviction directly against `BlobCache` (no CLI parsing needed):

```rust
#[cfg(test)]
mod tests {
    use rm_cloud::BlobCache;
    use sha2::{Digest, Sha256};

    fn h(b: &[u8]) -> String { hex::encode(Sha256::digest(b)) }

    #[test]
    fn gc_evicts_down_to_cap() {
        let dir = tempfile::tempdir().unwrap();
        let cache = BlobCache::new(dir.path());
        // Three 100-byte blobs = 300 bytes.
        for i in 0u8..3 {
            let bytes = vec![i; 100];
            cache.put(&h(&bytes), &bytes).unwrap();
        }
        assert_eq!(cache.total_size(), 300);

        // Reuse the gc logic by calling the same eviction the CLI does.
        let mut entries = cache.entries();
        let mut total = cache.total_size();
        entries.sort_by_key(|(_, _, m)| *m);
        for (path, len, _) in entries {
            if total <= 150 { break; }
            std::fs::remove_file(&path).unwrap();
            total -= len;
        }
        assert!(cache.total_size() <= 150);
    }
}
```

(If you prefer to avoid duplicating the eviction loop, extract it into a `pub fn gc(cache: &BlobCache, max_size: u64) -> (u64, usize)` in `cache_cmd.rs` and call that from both the CLI arm and the test. Either is acceptable; the assertion — store ends ≤ cap — is the contract.)

- [ ] **Step 5: Run + manual smoke**

Run: `cargo test -p rmapps cache_cmd && cargo build -p rmapps`
Then: `cargo run -p rmapps -- cache info` → prints the (empty) cache path/size without error.
Expected: tests pass; `cache info` runs. (No real cloud contacted.)

- [ ] **Step 6: Commit**

```bash
git add apps/rmapps/src/cloud.rs apps/rmapps/src/cache_cmd.rs apps/rmapps/src/main.rs
git commit -m "feat(rmapps): default blob cache dir + rmapps cache {info,gc,clear}"
```

```json:metadata
{"files": ["apps/rmapps/src/cloud.rs", "apps/rmapps/src/cache_cmd.rs", "apps/rmapps/src/main.rs"], "verifyCommand": "cargo test -p rmapps cache_cmd && cargo build -p rmapps", "acceptanceCriteria": ["client built with cache", "cache dir default + RMAPPS_CACHE_DIR override", "cache info", "gc evicts to cap", "cache clear"], "requiresUserVerification": false}
```

---

## Final Verification (after all tasks)

```bash
cargo build --workspace
cargo test --workspace                      # offline; FakeCloud only
cargo test -p rm-cloud --features fake      # cache + snapshot + ls_with suites
cargo clippy --workspace --all-targets
```

Do NOT run live cloud tests or `rmapps digest`/`sync` against the real account until the rate-limit window has cleared. First real digest run afterward pays one cold-cache cost; every subsequent run skips unchanged docs entirely and serves repeated metadata from disk.

## Self-Review notes

- **Spec coverage:** Component 1 (cache) → Tasks 3-5, 8; Component 2 (snapshot memo + walk) → Tasks 6-7; Component 3 (cheap-skip) → Tasks 1-2; Component 4 (cache CLI) → Task 8. All covered.
- **Sequencing:** cheap-skip (Tasks 1-2) lands first per the spec's sequencing note.
- **Type consistency:** `Entry.hash`, `RemoteDoc.hash`, `CloudDoc.version`, `BlobCache::{new,get,put,entries,total_size}`, `Client::{with_cache,ls_with,snapshot}`, `FakeCloud::blob_get_count` used consistently across tasks.
- **User verification:** spec requires none → no verification task (correct).
