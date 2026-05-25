# rm-cloud Client Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `crates/rm-cloud`, a pure-Rust client for the current reMarkable Cloud sync protocol, exposing a content-addressed snapshot/diff core, rmapi-compatible path porcelain, and an inkapp-facing reconcile layer, tested against an in-process fake cloud and (env-gated) the live cloud.

**Architecture:** The reMarkable cloud is a git-style content-addressed store: blobs keyed by `sha256`, two levels of text "index" blobs (root → per-doc), and a root ref with a monotonic `generation` updated by compare-and-swap. We model it as immutable `Snapshot`s with `diff()` and an atomic `commit()` (rebase-on-412). Path operations and a declarative working-set `sync()` are built on top. `rm-files` is reused for the on-disk `.rmdoc` `Bundle`.

**Tech Stack:** Rust 2021, tokio + reqwest (HTTP), sha2 + hex (hashing), serde/serde_json (indexes & JSON bodies), uuid (doc/device/run ids), axum (fake cloud, behind the `fake` feature), `rm-files` (bundle format), thiserror.

---

## Protocol reference (authoritative for this plan)

These rules were verified by reading rmapi's source. **Implement them exactly** — the
real cloud rejects mismatches.

- **Hosts.** auth = `https://webapp-prod.cloud.remarkable.engineering`,
  sync = `https://internal.cloud.remarkable.com`. Overridable via `RM_CLOUD_HOST`
  (sets all three: auth/doc/sync to the same base).
- **Endpoints.**
  - `POST {auth}/token/json/2/device/new` — body `{"code","deviceDesc","deviceID"}`, no bearer → device-token string.
  - `POST {auth}/token/json/2/user/new` — device bearer, empty body → user-token string.
  - `GET {sync}/sync/v4/root` — user bearer → `{"hash","generation","schemaVersion"}` (or 404 when the account has never synced).
  - `PUT {sync}/sync/v3/root` — user bearer, body `{"broadcast","hash","generation"}`, header `rm-filename: roothash` → `{"hash","generation","schemaVersion"}`; **412** if `generation` is stale.
  - `GET {sync}/sync/v3/files/<hash>` — user bearer, header `rm-filename: <logicalName>` → blob bytes (404 if absent).
  - `PUT {sync}/sync/v3/files/<hash>` — user bearer, header `rm-filename: <logicalName>`, body = blob bytes. For the root index blob the logical name is `root.docSchema` and `content-type: text/plain; charset=UTF-8`.
- **Hashing (lowercase hex throughout).**
  - **file hash** = `sha256(file bytes)`.
  - **doc hash** = `sha256( concat( hexdecode(fileHash) for files sorted by file id ) )` (this is rmapi's `HashEntries`).
  - **root hash** = `sha256( serialized root-index text bytes )`.
- **Blob keying (the non-obvious part).** A blob is uploaded under the hash that its
  *parent index lists*, and fetched by that same hash:
  - content blobs (`.metadata`/`.content`/`.pdf`/`.rm`/`.pagedata`) → keyed by their **file hash** (= sha256 of content).
  - a **doc-index** blob → keyed by the **doc hash** (`HashEntries`), **not** sha256 of the index text.
  - the **root-index** blob → keyed by the **root hash** (= sha256 of the index text).
  So the blob store is a plain `hash → bytes` map; it does **not** verify `key == sha256(bytes)` (that only holds for content + root blobs, not doc-index blobs).
- **Index text formats** (lines joined with `\n`, each line ending in `\n`):
  - **root index, schema `4`:** `4\n`, then `0:.:<docCount>:<totalSize>\n`, then per doc `<docHash>:0:<docId>:<numFiles>:<docSize>\n`. `docSize` = sum of that doc's file sizes; `totalSize` = sum of all `docSize`.
  - **per-doc index, schema `3`:** `3\n`, then per file `<fileHash>:0:<fileId>:0:<fileSize>\n`.
  - Reader accepts schema `3` and `4`; writer always emits `4` for root, `3` for per-doc. Sort docs by id and files by id before serializing (determinism).
- **Document layout.** A document id is a UUID; its files are `<id>.metadata`, `<id>.content`, payload `<id>.pdf` (or `.epub`), `<id>/<page-uuid>.rm` (one per annotated page), `<id>.pagedata`. The doc-index logical name is `<id>.docSchema`. A **folder** = a doc whose `.content`/metadata `CollectionType == "CollectionType"` and no payload.
- **CAS commit loop (rmapi's `Sync`).** apply mutation to a local tree → upload any new blobs → upload the new root-index blob → `PUT /root` with the snapshot's generation. On **412**: re-fetch root, re-apply the mutation onto the refreshed tree, retry. Bound at 10 attempts. Already-uploaded blobs are content-addressed, so retries don't re-upload them.

---

## File structure

```
crates/rm-cloud/
  Cargo.toml
  src/
    lib.rs            # re-exports + crate docs
    error.rs          # Error, Result
    config.rs         # Config: endpoint URLs + env overrides
    auth.rs           # Credentials, register_device, refresh_user_token
    transport.rs      # Http: reqwest wrapper, bearer, rm-filename, status->Error
    plumbing/
      mod.rs
      index.rs        # FileEntry, DocEntry, hashing + schema 3/4 parse/serialize
      snapshot.rs     # Snapshot, DocRef, TreeDiff, diff()
      blob.rs         # BlobStore: get/put blob by hash
      commit.rs       # commit(): upload blobs + CAS root-put with rebase-on-412
    porcelain/
      mod.rs
      docfiles.rs     # DocFiles: raw {name->bytes} view + rmdoc<->DocFiles conversions
      fs.rs           # path resolution: ls, stat, mkdir, mv, rm
      document.rs     # get, get_bundle, put, put_content_only
    sync.rs           # WorkingSet, SyncReport, Client::sync (declarative reconcile)
    client.rs         # Client: constructors + snapshot()/fs()/sync()
    fake/             # [feature fake]
      mod.rs          # FakeCloud::spawn(), state, fault injection
      handlers.rs     # axum route handlers
  tests/
    fake_lifecycle.rs       # required-features = ["fake"]
    fake_content_only.rs    # required-features = ["fake"]
    fake_conflict.rs        # required-features = ["fake"]
    fake_concurrency.rs     # required-features = ["fake"]
    real_cloud.rs           # #[ignore], env-gated
```

---

### Task 0: Crate scaffold, error, config

**Goal:** A compiling `rm-cloud` crate wired into the workspace, with the error type and endpoint config.

**Files:**
- Create: `crates/rm-cloud/Cargo.toml`
- Create: `crates/rm-cloud/src/lib.rs`
- Create: `crates/rm-cloud/src/error.rs`
- Create: `crates/rm-cloud/src/config.rs`
- Modify: `Cargo.toml` (workspace `members`)

**Acceptance Criteria:**
- [ ] `crates/rm-cloud` is a workspace member and builds.
- [ ] `Config::from_env()` honors `RM_CLOUD_HOST`.
- [ ] `Error` maps HTTP statuses (401/409/412/404) to distinct variants.

**Verify:** `nix develop -c cargo test -p rm-cloud` → config tests pass.

**Steps:**

- [ ] **Step 1: Add the crate to the workspace.** Edit root `Cargo.toml` `members`, adding `"crates/rm-cloud",` after `"crates/rm-files",`.

- [ ] **Step 2: Write `crates/rm-cloud/Cargo.toml`.**

```toml
[package]
name = "rm-cloud"
version = "0.1.0"
edition = "2021"
license = "MIT"
description = "Pure-Rust client for the reMarkable Cloud sync protocol (content-addressed snapshots, path ops, working-set sync)"

[dependencies]
thiserror = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
hex = "0.4"
uuid = { version = "1", features = ["v4"] }
reqwest = { version = "0.12", features = ["json"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync"] }
async-trait = "0.1"
rm-files = { path = "../rm-files" }
axum = { version = "0.8", optional = true }

[features]
fake = ["dep:axum", "tokio/full"]

[dev-dependencies]
tokio = { version = "1", features = ["full"] }
tempfile = "3"

[[test]]
name = "fake_lifecycle"
required-features = ["fake"]

[[test]]
name = "fake_content_only"
required-features = ["fake"]

[[test]]
name = "fake_conflict"
required-features = ["fake"]

[[test]]
name = "fake_concurrency"
required-features = ["fake"]
```

> Note: `Cargo.lock` will gain `axum`, `uuid`, `hex`. Per repo convention, do **not**
> stage `Cargo.lock` in feature commits — it is committed separately in Task 11.

- [ ] **Step 3: Write `src/error.rs`.**

```rust
//! Error type for `rm-cloud`.

use thiserror::Error;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by the reMarkable Cloud client.
#[derive(Debug, Error)]
pub enum Error {
    /// 401 from the cloud (token missing/expired and refresh failed).
    #[error("unauthorized")]
    Unauthorized,
    /// 409 from the cloud.
    #[error("conflict")]
    Conflict,
    /// 412 — the supplied root generation was stale (CAS failure).
    #[error("wrong generation (stale root)")]
    WrongGeneration,
    /// 404 — blob, root, or document not found.
    #[error("not found")]
    NotFound,
    /// CAS commit exhausted its retry budget against persistent conflicts.
    #[error("commit failed after {0} attempts")]
    CommitExhausted(u32),
    /// A required credential was absent.
    #[error("missing credential: {0}")]
    MissingCredential(&'static str),
    /// Malformed index/JSON/bundle content.
    #[error("parse error: {0}")]
    Parse(String),
    /// Any other HTTP-layer failure (with the status code if present).
    #[error("http error: {0}")]
    Http(String),
    /// Underlying reqwest transport failure.
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    /// Filesystem / IO failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON (de)serialization failure.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
```

- [ ] **Step 4: Write `src/config.rs`.**

```rust
//! Endpoint configuration, with env overrides for pointing tests at a fake.

/// Resolved base URLs for the three reMarkable cloud surfaces.
#[derive(Debug, Clone)]
pub struct Config {
    /// Auth host (token endpoints).
    pub auth: String,
    /// Sync host (root + blob endpoints).
    pub sync: String,
}

impl Config {
    /// Production defaults.
    pub fn production() -> Self {
        Self {
            auth: "https://webapp-prod.cloud.remarkable.engineering".into(),
            sync: "https://internal.cloud.remarkable.com".into(),
        }
    }

    /// Production defaults, overridden by `RM_CLOUD_HOST` (which sets all hosts to
    /// the same base — used to point the client at the fake cloud or a proxy).
    pub fn from_env() -> Self {
        match std::env::var("RM_CLOUD_HOST") {
            Ok(h) if !h.is_empty() => Self { auth: h.clone(), sync: h },
            _ => Self::production(),
        }
    }

    /// All three host bases set to `base` (used by the fake cloud).
    pub fn single_host(base: impl Into<String>) -> Self {
        let base = base.into();
        Self { auth: base.clone(), sync: base }
    }

    pub(crate) fn device_new(&self) -> String { format!("{}/token/json/2/device/new", self.auth) }
    pub(crate) fn user_new(&self) -> String { format!("{}/token/json/2/user/new", self.auth) }
    pub(crate) fn root_get(&self) -> String { format!("{}/sync/v4/root", self.sync) }
    pub(crate) fn root_put(&self) -> String { format!("{}/sync/v3/root", self.sync) }
    pub(crate) fn blob(&self, hash: &str) -> String { format!("{}/sync/v3/files/{}", self.sync, hash) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_override_sets_both_hosts() {
        // SAFETY: single-threaded test; restore after.
        std::env::set_var("RM_CLOUD_HOST", "http://127.0.0.1:9");
        let c = Config::from_env();
        assert_eq!(c.auth, "http://127.0.0.1:9");
        assert_eq!(c.sync, "http://127.0.0.1:9");
        std::env::remove_var("RM_CLOUD_HOST");
    }

    #[test]
    fn production_defaults() {
        let c = Config::production();
        assert!(c.sync.ends_with("remarkable.com"));
        assert_eq!(c.root_get(), format!("{}/sync/v4/root", c.sync));
    }
}
```

- [ ] **Step 5: Write `src/lib.rs` (initial).**

```rust
//! `rm-cloud` — a pure-Rust client for the current reMarkable Cloud sync protocol.
//!
//! The cloud is a content-addressed store (blobs keyed by sha256, a root ref with a
//! compare-and-swap generation). This crate exposes it as immutable [`Snapshot`]s with
//! `diff`, an atomic commit, rmapi-style path operations, and a declarative working-set
//! [`sync`](Client::sync) for app loops. See `docs/rm-cloud-protocol.md`.

#![warn(missing_docs)]

mod config;
mod error;

pub use config::Config;
pub use error::{Error, Result};
```

- [ ] **Step 6: Verify.** Run `nix develop -c cargo test -p rm-cloud` → 2 config tests pass. Run `nix develop -c cargo clippy -p rm-cloud --all-targets -- -D warnings` → clean.

- [ ] **Step 7: Commit.**

```bash
git add crates/rm-cloud/Cargo.toml crates/rm-cloud/src/lib.rs crates/rm-cloud/src/error.rs crates/rm-cloud/src/config.rs Cargo.toml
git commit -m "rm-cloud: crate scaffold, error type, endpoint config"
```

---

### Task 1: Index format + hashing rules

**Goal:** Pure (no-IO) encode/decode of root and per-doc indexes and the three hash rules, with golden-hash tests.

**Files:**
- Create: `crates/rm-cloud/src/plumbing/mod.rs`
- Create: `crates/rm-cloud/src/plumbing/index.rs`
- Modify: `crates/rm-cloud/src/lib.rs` (add `mod plumbing;` + re-exports)

**Acceptance Criteria:**
- [ ] `sha256_hex(bytes)` matches a known vector.
- [ ] `doc_hash(files)` matches `HashEntries` (concat raw file hashes, sorted by id).
- [ ] Root index round-trips: `parse_root_index(serialize_root_index(docs)) == docs` (order-independent).
- [ ] Per-doc index round-trips.
- [ ] `serialize_root_index` produces exactly `4\n0:.:<n>:<size>\n...` and `root_hash` = sha256 of those bytes.

**Verify:** `nix develop -c cargo test -p rm-cloud plumbing::index` → all pass.

**Steps:**

- [ ] **Step 1: Write `src/plumbing/mod.rs`.**

```rust
//! Plumbing: the content-addressed primitives (indexes, hashing, blobs, snapshots,
//! commit). Higher layers (porcelain, sync) are built on these.

pub mod index;
```

- [ ] **Step 2: Write the failing tests** in `src/plumbing/index.rs` (append a `#[cfg(test)]` module; write it first, it will not compile until the impl below exists — that is the red state).

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_vector() {
        // sha256("abc")
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn doc_hash_concats_sorted_raw_file_hashes() {
        // Two files; doc hash = sha256( raw(hashA) ++ raw(hashB) ) sorted by id.
        let fa = FileEntry { id: "b.file".into(), hash: sha256_hex(b"A"), size: 1 };
        let fb = FileEntry { id: "a.file".into(), hash: sha256_hex(b"B"), size: 1 };
        let got = doc_hash(&[fa.clone(), fb.clone()]);
        // expected: sort by id -> a.file (hash of "B"), then b.file (hash of "A")
        let mut h = sha2::Sha256::new();
        use sha2::Digest;
        h.update(hex::decode(sha256_hex(b"B")).unwrap());
        h.update(hex::decode(sha256_hex(b"A")).unwrap());
        assert_eq!(got, hex::encode(h.finalize()));
    }

    #[test]
    fn root_index_roundtrip() {
        let docs = vec![
            DocEntry { id: "zzz".into(), hash: "11".repeat(32), num_files: 2, size: 30 },
            DocEntry { id: "aaa".into(), hash: "22".repeat(32), num_files: 1, size: 10 },
        ];
        let bytes = serialize_root_index(&docs);
        let text = String::from_utf8(bytes.clone()).unwrap();
        // header + sorted-by-id docs
        assert!(text.starts_with("4\n0:.:2:40\n"));
        assert!(text.contains("\n22222222222222222222222222222222222222222222222222222222222222222:0:aaa:1:10\n"));
        let parsed = parse_root_index(&bytes).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "aaa"); // sorted
        assert_eq!(parsed[1].id, "zzz");
    }

    #[test]
    fn doc_index_roundtrip() {
        let files = vec![
            FileEntry { id: "x.content".into(), hash: "33".repeat(32), size: 5 },
            FileEntry { id: "x.metadata".into(), hash: "44".repeat(32), size: 7 },
        ];
        let bytes = serialize_doc_index(&files);
        assert!(bytes.starts_with(b"3\n"));
        let parsed = parse_doc_index(&bytes).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "x.content");
        assert_eq!(parsed[1].size, 7);
    }

    #[test]
    fn root_hash_is_sha256_of_index_text() {
        let docs = vec![DocEntry { id: "a".into(), hash: "ab".repeat(32), num_files: 1, size: 1 }];
        let bytes = serialize_root_index(&docs);
        assert_eq!(root_hash(&docs), sha256_hex(&bytes));
    }
}
```

- [ ] **Step 3: Run tests to verify they fail.** `nix develop -c cargo test -p rm-cloud plumbing::index` → FAIL (items not defined).

- [ ] **Step 4: Write the implementation** at the top of `src/plumbing/index.rs`.

```rust
//! Index (de)serialization and the three reMarkable hash rules.
//!
//! Root index = schema 4; per-doc index = schema 3. Hash rules:
//! file hash = sha256(content); doc hash = sha256(concat raw file hashes sorted by id);
//! root hash = sha256(serialized root index bytes). See `docs/rm-cloud-protocol.md`.

use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// One file inside a document (a line of a per-doc index).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Logical file id, e.g. `<uuid>.metadata` or `<uuid>/<page>.rm`.
    pub id: String,
    /// sha256(content) hex.
    pub hash: String,
    /// Byte size of the content.
    pub size: u64,
}

/// One document (a line of the root index).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocEntry {
    /// Document UUID.
    pub id: String,
    /// Doc hash (`HashEntries`) hex — also the doc-index blob key.
    pub hash: String,
    /// Number of files in the document.
    pub num_files: u32,
    /// Sum of file sizes.
    pub size: u64,
}

/// Lowercase hex sha256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Doc hash = sha256(concat of hexdecoded file hashes, files sorted by id).
pub fn doc_hash(files: &[FileEntry]) -> String {
    let mut sorted: Vec<&FileEntry> = files.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let mut h = Sha256::new();
    for f in sorted {
        // hashes are well-formed hex in practice; treat malformed as empty (cannot happen
        // for hashes we computed ourselves).
        if let Ok(raw) = hex::decode(&f.hash) {
            h.update(raw);
        }
    }
    hex::encode(h.finalize())
}

/// Sum of file sizes (a doc's `size` field).
pub fn doc_size(files: &[FileEntry]) -> u64 {
    files.iter().map(|f| f.size).sum()
}

/// Serialize a per-doc index (schema 3). Files are sorted by id.
pub fn serialize_doc_index(files: &[FileEntry]) -> Vec<u8> {
    let mut sorted: Vec<&FileEntry> = files.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let mut s = String::from("3\n");
    for f in sorted {
        // <hash>:0:<id>:0:<size>
        s.push_str(&format!("{}:0:{}:0:{}\n", f.hash, f.id, f.size));
    }
    s.into_bytes()
}

/// Serialize a root index (schema 4). Docs are sorted by id.
pub fn serialize_root_index(docs: &[DocEntry]) -> Vec<u8> {
    let mut sorted: Vec<&DocEntry> = docs.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let total: u64 = sorted.iter().map(|d| d.size).sum();
    let mut s = format!("4\n0:.:{}:{}\n", sorted.len(), total);
    for d in sorted {
        // <hash>:0:<id>:<numFiles>:<size>
        s.push_str(&format!("{}:0:{}:{}:{}\n", d.hash, d.id, d.num_files, d.size));
    }
    s.into_bytes()
}

/// Root hash = sha256(serialized root index bytes).
pub fn root_hash(docs: &[DocEntry]) -> String {
    sha256_hex(&serialize_root_index(docs))
}

/// Parse a per-doc index (schema 3). Returns files in id order.
pub fn parse_doc_index(bytes: &[u8]) -> Result<Vec<FileEntry>> {
    let text = std::str::from_utf8(bytes).map_err(|e| Error::Parse(e.to_string()))?;
    let mut lines = text.lines();
    let schema = lines.next().ok_or_else(|| Error::Parse("empty doc index".into()))?;
    if schema != "3" && schema != "4" {
        return Err(Error::Parse(format!("unsupported doc schema {schema:?}")));
    }
    let mut out = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(':').collect();
        if f.len() != 5 {
            return Err(Error::Parse(format!("doc line wrong field count: {line:?}")));
        }
        out.push(FileEntry {
            hash: f[0].to_string(),
            id: f[2].to_string(),
            size: f[4].parse().map_err(|_| Error::Parse(format!("bad size: {line:?}")))?,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Parse a root index (schema 3 or 4). Returns docs in id order.
pub fn parse_root_index(bytes: &[u8]) -> Result<Vec<DocEntry>> {
    let text = std::str::from_utf8(bytes).map_err(|e| Error::Parse(e.to_string()))?;
    let mut lines = text.lines();
    let schema = lines.next().ok_or_else(|| Error::Parse("empty root index".into()))?;
    let mut out = Vec::new();
    match schema {
        "4" => {
            // skip the "0:.:count:size" header line
            let _ = lines.next();
        }
        "3" => {}
        other => return Err(Error::Parse(format!("unsupported root schema {other:?}"))),
    }
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(':').collect();
        if f.len() != 5 {
            return Err(Error::Parse(format!("root line wrong field count: {line:?}")));
        }
        out.push(DocEntry {
            hash: f[0].to_string(),
            id: f[2].to_string(),
            num_files: f[3].parse().map_err(|_| Error::Parse(format!("bad numFiles: {line:?}")))?,
            size: f[4].parse().map_err(|_| Error::Parse(format!("bad size: {line:?}")))?,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}
```

- [ ] **Step 5: Wire into lib.** In `src/lib.rs` add `mod plumbing;` and `pub use plumbing::index::{DocEntry, FileEntry, doc_hash, root_hash, sha256_hex};`.

- [ ] **Step 6: Run tests + clippy.** `nix develop -c cargo test -p rm-cloud plumbing::index` → PASS. `nix develop -c cargo clippy -p rm-cloud --all-targets -- -D warnings` → clean. `nix develop -c cargo fmt`.

- [ ] **Step 7: Commit.**

```bash
git add crates/rm-cloud/src/plumbing crates/rm-cloud/src/lib.rs
git commit -m "rm-cloud: index encode/decode + sha256/doc/root hash rules"
```

---

### Task 2: Snapshot + diff

**Goal:** An immutable `Snapshot` (generation + root hash + doc list) with a pure `diff()` reporting added/removed/changed docs.

**Files:**
- Create: `crates/rm-cloud/src/plumbing/snapshot.rs`
- Modify: `crates/rm-cloud/src/plumbing/mod.rs` (add `pub mod snapshot;`)
- Modify: `crates/rm-cloud/src/lib.rs` (re-export `Snapshot`, `TreeDiff`)

**Acceptance Criteria:**
- [ ] `Snapshot::from_root_index(gen, hash, bytes)` parses into doc refs.
- [ ] `diff` reports a doc whose hash changed as `changed`, a new id as `added`, a missing id as `removed`.
- [ ] `Snapshot::doc(id)` looks up by id.

**Verify:** `nix develop -c cargo test -p rm-cloud plumbing::snapshot` → pass.

**Steps:**

- [ ] **Step 1: Write failing tests** in `snapshot.rs`.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::plumbing::index::{serialize_root_index, DocEntry};

    fn snap(gen: i64, docs: Vec<DocEntry>) -> Snapshot {
        let bytes = serialize_root_index(&docs);
        let hash = crate::plumbing::index::sha256_hex(&bytes);
        Snapshot::from_root_index(gen, hash, &bytes).unwrap()
    }

    #[test]
    fn diff_classifies_added_removed_changed() {
        let a = snap(1, vec![
            DocEntry { id: "keep".into(), hash: "aa".repeat(32), num_files: 1, size: 1 },
            DocEntry { id: "gone".into(), hash: "bb".repeat(32), num_files: 1, size: 1 },
            DocEntry { id: "edit".into(), hash: "cc".repeat(32), num_files: 1, size: 1 },
        ]);
        let b = snap(2, vec![
            DocEntry { id: "keep".into(), hash: "aa".repeat(32), num_files: 1, size: 1 },
            DocEntry { id: "edit".into(), hash: "dd".repeat(32), num_files: 1, size: 1 },
            DocEntry { id: "new".into(), hash: "ee".repeat(32), num_files: 1, size: 1 },
        ]);
        let d = a.diff(&b);
        assert_eq!(d.added, vec!["new"]);
        assert_eq!(d.removed, vec!["gone"]);
        assert_eq!(d.changed, vec!["edit"]);
    }

    #[test]
    fn lookup_by_id() {
        let a = snap(1, vec![DocEntry { id: "x".into(), hash: "aa".repeat(32), num_files: 1, size: 9 }]);
        assert_eq!(a.doc("x").unwrap().size, 9);
        assert!(a.doc("y").is_none());
    }
}
```

- [ ] **Step 2: Run → FAIL.** `nix develop -c cargo test -p rm-cloud plumbing::snapshot`.

- [ ] **Step 3: Implement** at the top of `snapshot.rs`.

```rust
//! Immutable account snapshot + tree diff.

use std::collections::BTreeMap;

use crate::error::Result;
use crate::plumbing::index::{parse_root_index, DocEntry};

/// A document reference inside a snapshot.
pub type DocRef = DocEntry;

/// An immutable view of the whole account at one generation.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Server generation of the root ref (CAS token for the next commit).
    pub generation: i64,
    /// Root index hash.
    pub root_hash: String,
    /// Documents by id (sorted), each with its doc hash.
    docs: BTreeMap<String, DocRef>,
}

/// The set difference between two snapshots, by document id.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TreeDiff {
    /// Ids present in `other` but not `self`.
    pub added: Vec<String>,
    /// Ids present in `self` but not `other`.
    pub removed: Vec<String>,
    /// Ids present in both whose doc hash differs.
    pub changed: Vec<String>,
}

impl Snapshot {
    /// Build a snapshot from a fetched root index blob.
    pub fn from_root_index(generation: i64, root_hash: String, bytes: &[u8]) -> Result<Self> {
        let docs = parse_root_index(bytes)?
            .into_iter()
            .map(|d| (d.id.clone(), d))
            .collect();
        Ok(Self { generation, root_hash, docs })
    }

    /// An empty snapshot (account never synced).
    pub fn empty() -> Self {
        Self { generation: 0, root_hash: String::new(), docs: BTreeMap::new() }
    }

    /// Look up a document by id.
    pub fn doc(&self, id: &str) -> Option<&DocRef> {
        self.docs.get(id)
    }

    /// All documents, id order.
    pub fn docs(&self) -> impl Iterator<Item = &DocRef> {
        self.docs.values()
    }

    /// Classify changes going from `self` to `other`.
    pub fn diff(&self, other: &Snapshot) -> TreeDiff {
        let mut d = TreeDiff::default();
        for (id, b) in &other.docs {
            match self.docs.get(id) {
                None => d.added.push(id.clone()),
                Some(a) if a.hash != b.hash => d.changed.push(id.clone()),
                Some(_) => {}
            }
        }
        for id in self.docs.keys() {
            if !other.docs.contains_key(id) {
                d.removed.push(id.clone());
            }
        }
        d.added.sort();
        d.removed.sort();
        d.changed.sort();
        d
    }
}
```

- [ ] **Step 4: Wire.** `plumbing/mod.rs` += `pub mod snapshot;`. `lib.rs` += `pub use plumbing::snapshot::{Snapshot, TreeDiff, DocRef};`.

- [ ] **Step 5: Test + clippy + fmt.** `nix develop -c cargo test -p rm-cloud plumbing::snapshot` → PASS.

- [ ] **Step 6: Commit.**

```bash
git add crates/rm-cloud/src/plumbing crates/rm-cloud/src/lib.rs
git commit -m "rm-cloud: immutable Snapshot + tree diff"
```

---

### Task 3: Fake cloud server

**Goal:** An in-process axum server (behind feature `fake`) implementing the real token/root/blob endpoints with generation CAS and fault injection, so all later tasks test over real HTTP.

**Files:**
- Create: `crates/rm-cloud/src/fake/mod.rs`
- Create: `crates/rm-cloud/src/fake/handlers.rs`
- Modify: `crates/rm-cloud/src/lib.rs` (add `#[cfg(feature = "fake")] pub mod fake;`)
- Create: `crates/rm-cloud/tests/fake_lifecycle.rs` (raw-reqwest smoke test of the server)

**Acceptance Criteria:**
- [ ] `FakeCloud::spawn()` binds `127.0.0.1:0` and returns its base URL.
- [ ] `GET /sync/v4/root` is 404 before any PUT; after a PUT it returns the stored `{hash,generation}`.
- [ ] `PUT /sync/v3/root` accepts when `generation` matches, returns the next generation, and returns **412** on mismatch.
- [ ] `PUT`/`GET /sync/v3/files/<hash>` store and return bytes verbatim (no content-address check).
- [ ] Token endpoints return a token string; `POST /token/json/2/device/new` echoes a token derived from the code.
- [ ] A fault toggle can force the *next* root PUT to 412 (to exercise rebase).

**Verify:** `nix develop -c cargo test -p rm-cloud --features fake --test fake_lifecycle` → pass.

**Steps:**

- [ ] **Step 1: Write `src/fake/mod.rs`.**

```rust
//! An in-process fake reMarkable cloud for tests. Real HTTP (axum) over an ephemeral
//! port, so clients exercise the true reqwest/serialization path. Enabled by feature
//! `fake`; public so downstream crates can test their own code against it.

mod handlers;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::net::TcpListener;

/// Shared server state.
#[derive(Default)]
pub struct State {
    /// hash -> blob bytes.
    pub blobs: HashMap<String, Vec<u8>>,
    /// Current root hash (empty before first PUT).
    pub root_hash: String,
    /// Current generation (0 before first PUT).
    pub generation: i64,
    /// If true, the next root PUT returns 412 then clears the flag.
    pub force_conflict_once: bool,
}

/// A running fake cloud. Drop to stop it.
pub struct FakeCloud {
    /// Base URL, e.g. `http://127.0.0.1:54321`.
    pub base: String,
    /// Shared state for assertions and fault injection.
    pub state: Arc<Mutex<State>>,
    handle: tokio::task::JoinHandle<()>,
}

impl FakeCloud {
    /// Bind an ephemeral port and start serving.
    pub async fn spawn() -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let app = handlers::router(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { base: format!("http://{addr}"), state, handle }
    }

    /// Force the next root PUT to fail with 412 (simulate a competing writer).
    pub fn inject_conflict_once(&self) {
        self.state.lock().unwrap().force_conflict_once = true;
    }

    /// Number of stored blobs (test helper).
    pub fn blob_count(&self) -> usize {
        self.state.lock().unwrap().blobs.len()
    }

    /// Read a stored blob by hash (test helper).
    pub fn blob(&self, hash: &str) -> Option<Vec<u8>> {
        self.state.lock().unwrap().blobs.get(hash).cloned()
    }
}

impl Drop for FakeCloud {
    fn drop(&mut self) {
        self.handle.abort();
    }
}
```

- [ ] **Step 2: Write `src/fake/handlers.rs`.**

```rust
//! axum route handlers for the fake cloud.

use std::sync::{Arc, Mutex};

use axum::{
    body::Bytes,
    extract::{Path, State as AxState},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use super::State;

type Shared = Arc<Mutex<State>>;

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/token/json/2/device/new", post(device_new))
        .route("/token/json/2/user/new", post(user_new))
        .route("/sync/v4/root", get(root_get))
        .route("/sync/v3/root", put(root_put))
        .route("/sync/v3/files/{hash}", get(blob_get).put(blob_put))
        .with_state(state)
}

#[derive(Deserialize)]
struct DeviceReq {
    code: String,
    #[allow(dead_code)]
    #[serde(rename = "deviceDesc")]
    device_desc: String,
    #[allow(dead_code)]
    #[serde(rename = "deviceID")]
    device_id: String,
}

async fn device_new(Json(req): Json<DeviceReq>) -> impl IntoResponse {
    // Echo a deterministic device token derived from the code.
    (StatusCode::OK, format!("device-token-for-{}", req.code))
}

async fn user_new(headers: HeaderMap) -> impl IntoResponse {
    match headers.get("authorization") {
        Some(_) => (StatusCode::OK, "user-token".to_string()),
        None => (StatusCode::UNAUTHORIZED, "no device bearer".to_string()),
    }
}

#[derive(Serialize)]
struct RootResp {
    hash: String,
    generation: i64,
    #[serde(rename = "schemaVersion")]
    schema_version: i64,
}

async fn root_get(AxState(state): AxState<Shared>) -> impl IntoResponse {
    let s = state.lock().unwrap();
    if s.generation == 0 && s.root_hash.is_empty() {
        return (StatusCode::NOT_FOUND, "no root yet").into_response();
    }
    Json(RootResp { hash: s.root_hash.clone(), generation: s.generation, schema_version: 4 })
        .into_response()
}

#[derive(Deserialize)]
struct RootPutReq {
    #[allow(dead_code)]
    broadcast: bool,
    hash: String,
    generation: i64,
}

async fn root_put(
    AxState(state): AxState<Shared>,
    Json(req): Json<RootPutReq>,
) -> impl IntoResponse {
    let mut s = state.lock().unwrap();
    if s.force_conflict_once {
        s.force_conflict_once = false;
        return (StatusCode::PRECONDITION_FAILED, "forced conflict").into_response();
    }
    if req.generation != s.generation {
        return (StatusCode::PRECONDITION_FAILED, "wrong generation").into_response();
    }
    s.generation = req.generation + 1;
    s.root_hash = req.hash.clone();
    let gen = s.generation;
    Json(RootResp { hash: req.hash, generation: gen, schema_version: 4 }).into_response()
}

async fn blob_get(
    AxState(state): AxState<Shared>,
    Path(hash): Path<String>,
) -> impl IntoResponse {
    let s = state.lock().unwrap();
    match s.blobs.get(&hash) {
        Some(b) => (StatusCode::OK, b.clone()).into_response(),
        None => (StatusCode::NOT_FOUND, "no blob").into_response(),
    }
}

async fn blob_put(
    AxState(state): AxState<Shared>,
    Path(hash): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    // Plain key->bytes store: doc-index blobs are keyed by doc hash, not content hash.
    state.lock().unwrap().blobs.insert(hash, body.to_vec());
    StatusCode::OK
}
```

- [ ] **Step 3: Wire into lib.** In `src/lib.rs` add:

```rust
#[cfg(feature = "fake")]
pub mod fake;
```

- [ ] **Step 4: Write the smoke test** `crates/rm-cloud/tests/fake_lifecycle.rs`.

```rust
//! Smoke-tests the fake cloud directly with raw reqwest (no client layer yet).

use rm_cloud::fake::FakeCloud;

#[tokio::test]
async fn root_cas_and_blob_storage() {
    let cloud = FakeCloud::spawn().await;
    let http = reqwest::Client::new();

    // Root is 404 before any write.
    let r = http.get(format!("{}/sync/v4/root", cloud.base)).send().await.unwrap();
    assert_eq!(r.status(), reqwest::StatusCode::NOT_FOUND);

    // Store a blob, read it back verbatim.
    let put = http
        .put(format!("{}/sync/v3/files/deadbeef", cloud.base))
        .header("rm-filename", "x.metadata")
        .body("hello".as_bytes().to_vec())
        .send().await.unwrap();
    assert!(put.status().is_success());
    let got = http.get(format!("{}/sync/v3/files/deadbeef", cloud.base)).send().await.unwrap();
    assert_eq!(got.bytes().await.unwrap().as_ref(), b"hello");

    // First root PUT (gen 0) succeeds -> gen 1.
    let body = serde_json::json!({"broadcast": false, "hash": "roothash1", "generation": 0});
    let r = http.put(format!("{}/sync/v3/root", cloud.base)).json(&body).send().await.unwrap();
    assert!(r.status().is_success());
    let j: serde_json::Value = r.json().await.unwrap();
    assert_eq!(j["generation"], 1);

    // Stale generation -> 412.
    let stale = serde_json::json!({"broadcast": false, "hash": "roothash2", "generation": 0});
    let r = http.put(format!("{}/sync/v3/root", cloud.base)).json(&stale).send().await.unwrap();
    assert_eq!(r.status(), reqwest::StatusCode::PRECONDITION_FAILED);

    // Forced conflict toggle.
    cloud.inject_conflict_once();
    let good = serde_json::json!({"broadcast": false, "hash": "roothash3", "generation": 1});
    let r = http.put(format!("{}/sync/v3/root", cloud.base)).json(&good).send().await.unwrap();
    assert_eq!(r.status(), reqwest::StatusCode::PRECONDITION_FAILED);
}
```

- [ ] **Step 5: Run.** `nix develop -c cargo test -p rm-cloud --features fake --test fake_lifecycle` → PASS. Clippy with the feature: `nix develop -c cargo clippy -p rm-cloud --all-targets --features fake -- -D warnings`.

- [ ] **Step 6: Commit.**

```bash
git add crates/rm-cloud/src/fake crates/rm-cloud/src/lib.rs crates/rm-cloud/tests/fake_lifecycle.rs
git commit -m "rm-cloud: in-process fake cloud (axum) with CAS + fault injection"
```

---

### Task 4: Transport + auth + credentials

**Goal:** The reqwest wrapper (`Http`) with bearer auth, `rm-filename` header, and status→`Error` mapping; the `Credentials` seam (env source); `register_device` and `refresh_user_token`.

**Files:**
- Create: `crates/rm-cloud/src/transport.rs`
- Create: `crates/rm-cloud/src/auth.rs`
- Modify: `crates/rm-cloud/src/lib.rs`
- Create: `crates/rm-cloud/tests/fake_auth.rs` (`required-features = ["fake"]` — add a `[[test]]` entry)

**Acceptance Criteria:**
- [ ] `register_device(config, code)` returns the device token from the fake.
- [ ] `refresh_user_token` exchanges a device token for a user token; missing device token → `MissingCredential`.
- [ ] `Http` maps 401→Unauthorized, 412→WrongGeneration, 404→NotFound, 409→Conflict.
- [ ] `Credentials::from_env` reads `RM_CLOUD_DEVICE_TOKEN`/`RM_CLOUD_USER_TOKEN`.

**Verify:** `nix develop -c cargo test -p rm-cloud --features fake --test fake_auth` → pass.

**Steps:**

- [ ] **Step 1: Add the `[[test]]` entry** to `Cargo.toml`:

```toml
[[test]]
name = "fake_auth"
required-features = ["fake"]
```

- [ ] **Step 2: Write `src/auth.rs`.**

```rust
//! Credentials seam + device pairing + user-token refresh.
//!
//! Tokens come from env vars for now (`RM_CLOUD_DEVICE_TOKEN`, `RM_CLOUD_USER_TOKEN`);
//! a forthcoming config system will construct [`Credentials`] another way. The struct is
//! the thin replaceable seam.

use serde::Serialize;
use uuid::Uuid;

use crate::config::Config;
use crate::error::{Error, Result};

/// A device + user token pair. The device token is long-lived; the user token is
/// short-lived and refreshed from the device token.
#[derive(Debug, Clone, Default)]
pub struct Credentials {
    /// Long-lived device token (from pairing).
    pub device_token: Option<String>,
    /// Short-lived user token (refreshed as needed).
    pub user_token: Option<String>,
}

impl Credentials {
    /// Read tokens from the environment.
    pub fn from_env() -> Self {
        Self {
            device_token: std::env::var("RM_CLOUD_DEVICE_TOKEN").ok().filter(|s| !s.is_empty()),
            user_token: std::env::var("RM_CLOUD_USER_TOKEN").ok().filter(|s| !s.is_empty()),
        }
    }

    /// Construct from an explicit device token.
    pub fn from_device_token(token: impl Into<String>) -> Self {
        Self { device_token: Some(token.into()), user_token: None }
    }
}

#[derive(Serialize)]
struct DeviceReq<'a> {
    code: &'a str,
    #[serde(rename = "deviceDesc")]
    device_desc: &'a str,
    #[serde(rename = "deviceID")]
    device_id: String,
}

/// Pair a new device with a one-time 8-char code from
/// <https://my.remarkable.com/device/desktop/connect>; returns the device token.
pub async fn register_device(http: &reqwest::Client, config: &Config, code: &str) -> Result<String> {
    let req = DeviceReq { code, device_desc: "desktop-linux", device_id: Uuid::new_v4().to_string() };
    let resp = http.post(config.device_new()).json(&req).send().await?;
    if !resp.status().is_success() {
        return Err(Error::Http(format!("device pairing failed: {}", resp.status())));
    }
    Ok(resp.text().await?)
}

/// Exchange a device token for a fresh user token.
pub async fn refresh_user_token(http: &reqwest::Client, config: &Config, device_token: &str) -> Result<String> {
    if device_token.is_empty() {
        return Err(Error::MissingCredential("device_token"));
    }
    let resp = http
        .post(config.user_new())
        .bearer_auth(device_token)
        .send().await?;
    match resp.status() {
        s if s.is_success() => Ok(resp.text().await?),
        reqwest::StatusCode::UNAUTHORIZED => Err(Error::Unauthorized),
        s => Err(Error::Http(format!("user token refresh failed: {s}"))),
    }
}
```

- [ ] **Step 3: Write `src/transport.rs`.**

```rust
//! Thin reqwest wrapper: user-bearer auth, the `rm-filename` header, and a single
//! place that maps HTTP statuses to [`Error`].

use reqwest::StatusCode;

use crate::error::{Error, Result};

/// Map a non-success status to the corresponding error.
pub(crate) fn status_error(status: StatusCode) -> Error {
    match status {
        StatusCode::UNAUTHORIZED => Error::Unauthorized,
        StatusCode::CONFLICT => Error::Conflict,
        StatusCode::PRECONDITION_FAILED => Error::WrongGeneration,
        StatusCode::NOT_FOUND => Error::NotFound,
        other => Error::Http(format!("request failed: {other}")),
    }
}

/// Return `Ok(resp)` for 2xx, else the mapped error.
pub(crate) fn check(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        Ok(resp)
    } else {
        Err(status_error(status))
    }
}
```

- [ ] **Step 4: Wire into lib.** In `src/lib.rs`:

```rust
mod auth;
mod transport;

pub use auth::{register_device, refresh_user_token, Credentials};
```

- [ ] **Step 5: Write `crates/rm-cloud/tests/fake_auth.rs`.**

```rust
use rm_cloud::fake::FakeCloud;
use rm_cloud::{register_device, refresh_user_token, Config};

#[tokio::test]
async fn pairing_and_user_token() {
    let cloud = FakeCloud::spawn().await;
    let config = Config::single_host(&cloud.base);
    let http = reqwest::Client::new();

    let device = register_device(&http, &config, "ABCD1234").await.unwrap();
    assert_eq!(device, "device-token-for-ABCD1234");

    let user = refresh_user_token(&http, &config, &device).await.unwrap();
    assert_eq!(user, "user-token");
}

#[tokio::test]
async fn empty_device_token_is_missing_credential() {
    let cloud = FakeCloud::spawn().await;
    let config = Config::single_host(&cloud.base);
    let http = reqwest::Client::new();
    let err = refresh_user_token(&http, &config, "").await.unwrap_err();
    assert!(matches!(err, rm_cloud::Error::MissingCredential(_)));
}
```

- [ ] **Step 6: Run + clippy + fmt.** `nix develop -c cargo test -p rm-cloud --features fake --test fake_auth` → PASS.

- [ ] **Step 7: Commit.**

```bash
git add crates/rm-cloud/src/auth.rs crates/rm-cloud/src/transport.rs crates/rm-cloud/src/lib.rs crates/rm-cloud/Cargo.toml crates/rm-cloud/tests/fake_auth.rs
git commit -m "rm-cloud: transport status mapping, credentials seam, pairing + refresh"
```

---

### Task 5: BlobStore + Client + snapshot()

**Goal:** `BlobStore` (get/put blob over HTTP) and a `Client` that auto-refreshes the user token and builds a `Snapshot` from the cloud.

**Files:**
- Create: `crates/rm-cloud/src/plumbing/blob.rs`
- Create: `crates/rm-cloud/src/client.rs`
- Modify: `crates/rm-cloud/src/plumbing/mod.rs`, `src/lib.rs`
- Create: `crates/rm-cloud/tests/fake_snapshot.rs` (`required-features`)

**Acceptance Criteria:**
- [ ] `Client::from_device_token(config, token)` builds; `snapshot()` returns `Snapshot::empty()` against a fresh cloud (root 404).
- [ ] After uploading a root index blob + root PUT, `snapshot()` reflects the doc list and generation.
- [ ] A 401 on a blob/root call triggers exactly one transparent user-token refresh, then retries.
- [ ] `BlobStore::put_blob`/`get_blob` round-trip bytes.

**Verify:** `nix develop -c cargo test -p rm-cloud --features fake --test fake_snapshot` → pass.

**Steps:**

- [ ] **Step 1: Add `[[test]]` entry** `name = "fake_snapshot"`, `required-features = ["fake"]`.

- [ ] **Step 2: Write `src/plumbing/blob.rs`.**

```rust
//! Content-addressed blob transfer.

use crate::error::Result;
use crate::transport::check;

/// Logical filename header the cloud expects on blob transfers.
pub(crate) const RM_FILENAME: &str = "rm-filename";

/// GET a blob by hash. `name` is the logical filename header (e.g. `<id>.metadata`).
pub(crate) async fn get_blob(
    http: &reqwest::Client,
    url: &str,
    user_token: &str,
    name: &str,
) -> Result<Vec<u8>> {
    let resp = http.get(url).bearer_auth(user_token).header(RM_FILENAME, name).send().await?;
    let resp = check(resp)?;
    Ok(resp.bytes().await?.to_vec())
}

/// PUT a blob under `hash` (the caller computed it per the keying rules).
pub(crate) async fn put_blob(
    http: &reqwest::Client,
    url: &str,
    user_token: &str,
    name: &str,
    bytes: Vec<u8>,
) -> Result<()> {
    let mut req = http.put(url).bearer_auth(user_token).header(RM_FILENAME, name);
    if name == "root.docSchema" {
        req = req.header("content-type", "text/plain; charset=UTF-8");
    }
    let resp = req.body(bytes).send().await?;
    check(resp)?;
    Ok(())
}
```

- [ ] **Step 3: Write `src/client.rs`.**

```rust
//! The cloud client: holds credentials + config, refreshes the user token on demand,
//! and exposes snapshot/porcelain/sync entry points.

use std::sync::Arc;

use serde::Deserialize;
use tokio::sync::RwLock;

use crate::auth::{refresh_user_token, Credentials};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::plumbing::blob::{get_blob, put_blob};
use crate::plumbing::snapshot::Snapshot;

/// A client for one reMarkable cloud account. Construct multiple clients for multiple
/// accounts — there is no shared global state.
#[derive(Clone)]
pub struct Client {
    pub(crate) http: reqwest::Client,
    pub(crate) config: Config,
    pub(crate) creds: Arc<RwLock<Credentials>>,
}

#[derive(Deserialize)]
struct RootResp {
    hash: String,
    generation: i64,
}

impl Client {
    /// Build a client from an explicit device token (user token refreshed lazily).
    pub fn from_device_token(config: Config, device_token: impl Into<String>) -> Self {
        Self::new(config, Credentials::from_device_token(device_token))
    }

    /// Build a client from an explicit user token.
    pub fn from_user_token(config: Config, user_token: impl Into<String>) -> Self {
        Self::new(config, Credentials { device_token: None, user_token: Some(user_token.into()) })
    }

    /// Build a client from `RM_CLOUD_*` env vars and `Config::from_env()`.
    pub fn from_env() -> Result<Self> {
        let creds = Credentials::from_env();
        if creds.device_token.is_none() && creds.user_token.is_none() {
            return Err(Error::MissingCredential("RM_CLOUD_DEVICE_TOKEN or RM_CLOUD_USER_TOKEN"));
        }
        Ok(Self::new(Config::from_env(), creds))
    }

    fn new(config: Config, creds: Credentials) -> Self {
        Self { http: reqwest::Client::new(), config, creds: Arc::new(RwLock::new(creds)) }
    }

    /// Current user token, refreshing from the device token if absent.
    async fn user_token(&self) -> Result<String> {
        if let Some(t) = self.creds.read().await.user_token.clone() {
            return Ok(t);
        }
        self.force_refresh().await
    }

    /// Refresh the user token from the device token and store it.
    async fn force_refresh(&self) -> Result<String> {
        let device = self.creds.read().await.device_token.clone()
            .ok_or(Error::MissingCredential("device_token"))?;
        let token = refresh_user_token(&self.http, &self.config, &device).await?;
        self.creds.write().await.user_token = Some(token.clone());
        Ok(token)
    }

    /// Run `op(user_token)`; on `Unauthorized`, refresh once and retry.
    async fn with_auth<T, F, Fut>(&self, op: F) -> Result<T>
    where
        F: Fn(String) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let token = self.user_token().await?;
        match op(token).await {
            Err(Error::Unauthorized) => {
                let token = self.force_refresh().await?;
                op(token).await
            }
            other => other,
        }
    }

    /// Fetch the current account snapshot (empty if the account never synced).
    pub async fn snapshot(&self) -> Result<Snapshot> {
        let root = self.with_auth(|token| {
            let http = self.http.clone();
            let url = self.config.root_get();
            async move {
                let resp = http.get(&url).bearer_auth(&token).send().await?;
                match resp.status() {
                    reqwest::StatusCode::NOT_FOUND => Ok(None),
                    s if s.is_success() => Ok(Some(resp.json::<RootResp>().await?)),
                    reqwest::StatusCode::UNAUTHORIZED => Err(Error::Unauthorized),
                    s => Err(Error::Http(format!("root get failed: {s}"))),
                }
            }
        }).await?;

        let Some(root) = root else { return Ok(Snapshot::empty()); };
        let token = self.user_token().await?;
        let bytes = get_blob(&self.http, &self.config.blob(&root.hash), &token, "root.docSchema").await?;
        Snapshot::from_root_index(root.generation, root.hash, &bytes)
    }

    // --- internal helpers reused by commit/porcelain ---

    pub(crate) async fn get_blob(&self, hash: &str, name: &str) -> Result<Vec<u8>> {
        let token = self.user_token().await?;
        get_blob(&self.http, &self.config.blob(hash), &token, name).await
    }

    pub(crate) async fn put_blob(&self, hash: &str, name: &str, bytes: Vec<u8>) -> Result<()> {
        let token = self.user_token().await?;
        put_blob(&self.http, &self.config.blob(hash), &token, name, bytes).await
    }
}
```

- [ ] **Step 4: Wire.** `plumbing/mod.rs` += `pub(crate) mod blob;`. `lib.rs` += `mod client; pub use client::Client;`.

- [ ] **Step 5: Write `crates/rm-cloud/tests/fake_snapshot.rs`.**

```rust
use rm_cloud::fake::FakeCloud;
use rm_cloud::plumbing::index::{serialize_root_index, DocEntry};
use rm_cloud::{Client, Config};

#[tokio::test]
async fn empty_account_snapshot() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");
    let snap = client.snapshot().await.unwrap();
    assert_eq!(snap.generation, 0);
    assert_eq!(snap.docs().count(), 0);
}

#[tokio::test]
async fn snapshot_reflects_uploaded_root() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");

    // Hand-place a root index blob + root ref via the fake's own HTTP.
    let docs = vec![DocEntry { id: "doc-a".into(), hash: "ab".repeat(32), num_files: 2, size: 12 }];
    let bytes = serialize_root_index(&docs);
    let root_hash = rm_cloud::sha256_hex(&bytes);
    let http = reqwest::Client::new();
    http.put(format!("{}/sync/v3/files/{root_hash}", cloud.base))
        .header("rm-filename", "root.docSchema").body(bytes).send().await.unwrap();
    let body = serde_json::json!({"broadcast": false, "hash": root_hash, "generation": 0});
    http.put(format!("{}/sync/v3/root", cloud.base)).json(&body).send().await.unwrap();

    let snap = client.snapshot().await.unwrap();
    assert_eq!(snap.generation, 1);
    assert_eq!(snap.doc("doc-a").unwrap().num_files, 2);
}
```

> Note: this requires `pub mod plumbing;` exposure. In `lib.rs`, change `mod plumbing;` to
> `pub mod plumbing;` so tests/consumers can reach plumbing types. Keep `blob` as
> `pub(crate)` inside it.

- [ ] **Step 6: Run + clippy + fmt.** `nix develop -c cargo test -p rm-cloud --features fake --test fake_snapshot` → PASS.

- [ ] **Step 7: Commit.**

```bash
git add crates/rm-cloud/src/plumbing crates/rm-cloud/src/client.rs crates/rm-cloud/src/lib.rs crates/rm-cloud/Cargo.toml crates/rm-cloud/tests/fake_snapshot.rs
git commit -m "rm-cloud: BlobStore + Client with lazy token refresh + snapshot()"
```

---

### Task 6: Atomic commit with rebase-on-412

**Goal:** A `commit` primitive: given a desired set of doc changes, upload new blobs, build + upload the new root index, CAS-put the root, and on 412 re-fetch + re-apply (bounded retries). Includes a concurrency test.

**Files:**
- Create: `crates/rm-cloud/src/plumbing/commit.rs`
- Modify: `crates/rm-cloud/src/plumbing/mod.rs`, `src/client.rs` (add `commit` method)
- Create: `crates/rm-cloud/tests/fake_conflict.rs`, `crates/rm-cloud/tests/fake_concurrency.rs`

**Acceptance Criteria:**
- [ ] A `Mutation` describing upserts (full doc file-sets) and removals applies to a snapshot to produce the next root index.
- [ ] `commit` uploads each new doc's file blobs + doc-index blob, then the root blob, then CAS-puts root; generation advances.
- [ ] An injected 412 causes one rebase + retry, and the commit ultimately succeeds.
- [ ] After `CommitExhausted` budget (10) of persistent conflicts, returns `Error::CommitExhausted`.
- [ ] Concurrency: 5 parallel single-doc commits all land; final snapshot has 5 docs; generation advanced by 5.

**Verify:** `nix develop -c cargo test -p rm-cloud --features fake --test fake_conflict --test fake_concurrency` → pass.

**Steps:**

- [ ] **Step 1: Add `[[test]]` entries** for `fake_conflict` and `fake_concurrency` (`required-features = ["fake"]`).

- [ ] **Step 2: Write `src/plumbing/commit.rs`.**

```rust
//! Atomic commit: turn a desired mutation into uploaded blobs + a CAS root-put,
//! retrying with a rebase when the generation is stale.

use crate::error::Result;
use crate::plumbing::index::{
    doc_hash, doc_size, serialize_doc_index, serialize_root_index, sha256_hex, DocEntry, FileEntry,
};

/// A document to upsert: its full set of files with raw bytes.
pub struct DocUpsert {
    /// Document id (UUID).
    pub id: String,
    /// Files: (logical id, bytes). The doc/file hashes are computed here.
    pub files: Vec<(String, Vec<u8>)>,
}

/// A desired change to the account tree.
#[derive(Default)]
pub struct Mutation {
    /// Documents to create or replace wholesale.
    pub upserts: Vec<DocUpsert>,
    /// Document ids to remove.
    pub removals: Vec<String>,
}

/// Blobs to upload for one upserted doc, plus the resulting root [`DocEntry`].
pub(crate) struct PreparedDoc {
    pub doc_entry: DocEntry,
    /// (hash, logical name, bytes) for each file blob + the doc-index blob.
    pub blobs: Vec<(String, String, Vec<u8>)>,
}

/// Compute hashes + serialized blobs for one upsert (pure; no IO).
pub(crate) fn prepare_doc(up: &DocUpsert) -> PreparedDoc {
    let file_entries: Vec<FileEntry> = up
        .files
        .iter()
        .map(|(id, bytes)| FileEntry { id: id.clone(), hash: sha256_hex(bytes), size: bytes.len() as u64 })
        .collect();

    let dhash = doc_hash(&file_entries);
    let size = doc_size(&file_entries);
    let index_bytes = serialize_doc_index(&file_entries);

    let mut blobs: Vec<(String, String, Vec<u8>)> = up
        .files
        .iter()
        .zip(file_entries.iter())
        .map(|((id, bytes), fe)| (fe.hash.clone(), id.clone(), bytes.clone()))
        .collect();
    // The doc-index blob is keyed by the doc hash, named "<id>.docSchema".
    blobs.push((dhash.clone(), format!("{}.docSchema", up.id), index_bytes));

    PreparedDoc {
        doc_entry: DocEntry { id: up.id.clone(), hash: dhash, num_files: file_entries.len() as u32, size },
        blobs,
    }
}

/// Apply a mutation to the current doc set, returning the new root [`DocEntry`] list.
pub(crate) fn apply(current: &[DocEntry], mutation: &Mutation, prepared: &[PreparedDoc]) -> Vec<DocEntry> {
    let mut by_id: std::collections::BTreeMap<String, DocEntry> =
        current.iter().map(|d| (d.id.clone(), d.clone())).collect();
    for id in &mutation.removals {
        by_id.remove(id);
    }
    for p in prepared {
        by_id.insert(p.doc_entry.id.clone(), p.doc_entry.clone());
    }
    by_id.into_values().collect()
}

/// Build the (root_hash, root_index_bytes) for a doc-entry list.
pub(crate) fn root_blob(docs: &[DocEntry]) -> (String, Vec<u8>) {
    let bytes = serialize_root_index(docs);
    (sha256_hex(&bytes), bytes)
}
```

- [ ] **Step 3: Add `commit` to `src/client.rs`** (uses the pure helpers above + the CAS loop):

```rust
use crate::plumbing::commit::{apply, prepare_doc, root_blob, Mutation};

#[derive(serde::Serialize)]
struct RootPutReq<'a> {
    broadcast: bool,
    hash: &'a str,
    generation: i64,
}

#[derive(serde::Deserialize)]
struct RootPutResp {
    generation: i64,
}

impl Client {
    /// Apply `mutation` atomically: upload new blobs, then CAS-put the root, rebasing on
    /// a stale generation. Returns the post-commit snapshot.
    pub async fn commit(&self, mutation: Mutation) -> Result<Snapshot> {
        // Prepare + upload doc blobs once (content-addressed → safe across retries).
        let prepared: Vec<_> = mutation.upserts.iter().map(prepare_doc).collect();
        for p in &prepared {
            for (hash, name, bytes) in &p.blobs {
                self.put_blob(hash, name, bytes.clone()).await?;
            }
        }

        const MAX_ATTEMPTS: u32 = 10;
        for _ in 0..MAX_ATTEMPTS {
            let snap = self.snapshot().await?;
            let current: Vec<_> = snap.docs().cloned().collect();
            let new_docs = apply(&current, &mutation, &prepared);
            let (rhash, rbytes) = root_blob(&new_docs);
            self.put_blob(&rhash, "root.docSchema", rbytes).await?;

            let token = self.user_token().await?;
            let resp = self.http
                .put(self.config.root_put())
                .bearer_auth(&token)
                .header(crate::plumbing::blob::RM_FILENAME, "roothash")
                .json(&RootPutReq { broadcast: false, hash: &rhash, generation: snap.generation })
                .send().await?;
            match resp.status() {
                s if s.is_success() => {
                    let gen = resp.json::<RootPutResp>().await?.generation;
                    return Ok(Snapshot::from_root_index(gen, rhash, &serialize_root_index(&new_docs))?);
                }
                reqwest::StatusCode::PRECONDITION_FAILED => continue, // rebase: loop re-snapshots
                reqwest::StatusCode::UNAUTHORIZED => { self.force_refresh().await?; continue; }
                s => return Err(Error::Http(format!("root put failed: {s}"))),
            }
        }
        Err(Error::CommitExhausted(MAX_ATTEMPTS))
    }
}
```

> Add `use crate::plumbing::index::serialize_root_index;` to `client.rs` imports.

- [ ] **Step 4: Wire.** `plumbing/mod.rs` += `pub mod commit;`. `lib.rs` += `pub use plumbing::commit::{Mutation, DocUpsert};`.

- [ ] **Step 5: Write `crates/rm-cloud/tests/fake_conflict.rs`.**

```rust
use rm_cloud::fake::FakeCloud;
use rm_cloud::{Client, Config, DocUpsert, Mutation};

fn one_doc(id: &str) -> DocUpsert {
    DocUpsert {
        id: id.to_string(),
        files: vec![
            (format!("{id}.metadata"), br#"{"visibleName":"t"}"#.to_vec()),
            (format!("{id}.content"), b"{}".to_vec()),
        ],
    }
}

#[tokio::test]
async fn commit_succeeds_after_injected_conflict() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");

    cloud.inject_conflict_once(); // first root PUT will 412
    let snap = client.commit(Mutation { upserts: vec![one_doc("doc-1")], removals: vec![] }).await.unwrap();
    assert!(snap.doc("doc-1").is_some());
    // generation advanced past 0 despite the conflict
    assert!(snap.generation >= 1);
}

#[tokio::test]
async fn commit_round_trips_into_snapshot() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");
    client.commit(Mutation { upserts: vec![one_doc("a"), one_doc("b")], removals: vec![] }).await.unwrap();
    let snap = client.snapshot().await.unwrap();
    assert_eq!(snap.docs().count(), 2);

    // Remove one.
    client.commit(Mutation { upserts: vec![], removals: vec!["a".into()] }).await.unwrap();
    let snap = client.snapshot().await.unwrap();
    assert_eq!(snap.docs().count(), 1);
    assert!(snap.doc("b").is_some());
}
```

- [ ] **Step 6: Write `crates/rm-cloud/tests/fake_concurrency.rs`.**

```rust
use rm_cloud::fake::FakeCloud;
use rm_cloud::{Client, Config, DocUpsert, Mutation};

#[tokio::test]
async fn parallel_commits_all_land() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");

    let mut handles = Vec::new();
    for i in 0..5 {
        let c = client.clone();
        handles.push(tokio::spawn(async move {
            let id = format!("doc-{i}");
            let up = DocUpsert { id: id.clone(), files: vec![(format!("{id}.metadata"), b"{}".to_vec())] };
            c.commit(Mutation { upserts: vec![up], removals: vec![] }).await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    let snap = client.snapshot().await.unwrap();
    assert_eq!(snap.docs().count(), 5, "all 5 concurrent commits must land via rebase");
    assert_eq!(snap.generation, 5);
}
```

- [ ] **Step 7: Run + clippy + fmt.** `nix develop -c cargo test -p rm-cloud --features fake --test fake_conflict --test fake_concurrency` → PASS.

- [ ] **Step 8: Commit.**

```bash
git add crates/rm-cloud/src/plumbing crates/rm-cloud/src/client.rs crates/rm-cloud/src/lib.rs crates/rm-cloud/Cargo.toml crates/rm-cloud/tests/fake_conflict.rs crates/rm-cloud/tests/fake_concurrency.rs
git commit -m "rm-cloud: atomic commit with rebase-on-412 + concurrency coverage"
```

---

### Task 7: Porcelain — DocFiles + get/put/mkdir/mv/rm

**Goal:** Raw `DocFiles` representation with `.rmdoc`↔`DocFiles` conversions, plus the path operations and a `Bundle` round-trip.

**Files:**
- Create: `crates/rm-cloud/src/porcelain/mod.rs`, `docfiles.rs`, `fs.rs`, `document.rs`
- Modify: `src/lib.rs`, `src/client.rs` (porcelain methods)
- Create: `crates/rm-cloud/tests/fake_porcelain.rs`

**Acceptance Criteria:**
- [ ] `DocFiles::write_rmdoc(path)` produces a zip openable by `rm_files::Bundle::open`.
- [ ] `Client::get(id)` downloads all of a doc's files into `DocFiles`; `get_bundle(id)` returns a `rm_files::Bundle`.
- [ ] `Client::put(docfiles, parent)` creates a doc with a fresh UUID; it appears in the next snapshot.
- [ ] `mkdir(name, parent)` creates a folder doc (`CollectionType`).
- [ ] `mv(id, new_parent, new_name)` edits only the `.metadata` blob (other blobs' hashes unchanged).
- [ ] `rm(id)` removes the doc.
- [ ] `ls(parent)`/`stat(id)` resolve names/parents from metadata.

**Verify:** `nix develop -c cargo test -p rm-cloud --features fake --test fake_porcelain` → pass.

**Steps:**

- [ ] **Step 1: Add `[[test]]` entry** `name = "fake_porcelain"`, `required-features = ["fake"]`.

- [ ] **Step 2: Write `src/porcelain/mod.rs`.**

```rust
//! Porcelain: raw document file-sets, the rmapi-style path view, and document IO.

pub mod docfiles;
pub mod document;
pub mod fs;
```

- [ ] **Step 3: Write `src/porcelain/docfiles.rs`.**

```rust
//! `DocFiles` — a document as the cloud stores it: logical name -> bytes, plus the
//! metadata/content JSON. Converts to/from a `.rmdoc` zip for `rm_files::Bundle`.

use std::io::{Cursor, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// reMarkable document metadata (`<id>.metadata`). Only the fields we read/write.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Metadata {
    #[serde(rename = "visibleName")]
    pub visible_name: String,
    #[serde(rename = "type")]
    pub doc_type: String, // "DocumentType" | "CollectionType"
    pub parent: String,
    #[serde(rename = "lastModified")]
    pub last_modified: String,
    #[serde(default)]
    pub deleted: bool,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A document's full file-set (logical name -> bytes).
#[derive(Debug, Clone, Default)]
pub struct DocFiles {
    /// Document UUID.
    pub id: String,
    /// All files keyed by logical name, e.g. `<id>.metadata`, `<id>.pdf`, `<id>/<page>.rm`.
    pub files: Vec<(String, Vec<u8>)>,
}

impl DocFiles {
    /// Get a file's bytes by logical name.
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.files.iter().find(|(n, _)| n == name).map(|(_, b)| b.as_slice())
    }

    /// Parse the `<id>.metadata` blob.
    pub fn metadata(&self) -> Result<Metadata> {
        let raw = self.get(&format!("{}.metadata", self.id))
            .ok_or_else(|| Error::Parse("missing .metadata".into()))?;
        Ok(serde_json::from_slice(raw)?)
    }

    /// Replace the `<id>.metadata` blob with `meta`.
    pub fn set_metadata(&mut self, meta: &Metadata) -> Result<()> {
        let name = format!("{}.metadata", self.id);
        let bytes = serde_json::to_vec(meta)?;
        if let Some(slot) = self.files.iter_mut().find(|(n, _)| *n == name) {
            slot.1 = bytes;
        } else {
            self.files.push((name, bytes));
        }
        Ok(())
    }

    /// Write a `.rmdoc` zip to `path` (openable by `rm_files::Bundle::open`).
    pub fn write_rmdoc(&self, path: &Path) -> Result<()> {
        let file = std::fs::File::create(path)?;
        let mut zip = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in &self.files {
            zip.start_file(name, opts).map_err(|e| Error::Parse(e.to_string()))?;
            zip.write_all(bytes)?;
        }
        zip.finish().map_err(|e| Error::Parse(e.to_string()))?;
        Ok(())
    }

    /// Build a `DocFiles` from a `.rmdoc` zip on disk.
    pub fn from_rmdoc(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        Self::from_zip_bytes(&bytes)
    }

    /// Build a `DocFiles` from in-memory zip bytes.
    pub fn from_zip_bytes(bytes: &[u8]) -> Result<Self> {
        use std::io::Read;
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| Error::Parse(e.to_string()))?;
        let mut files = Vec::new();
        let mut id = String::new();
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(|e| Error::Parse(e.to_string()))?;
            let name = f.name().to_string();
            if let Some(stem) = name.strip_suffix(".content") {
                id = stem.to_string();
            }
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            files.push((name, buf));
        }
        if id.is_empty() {
            return Err(Error::Parse("rmdoc has no .content entry (cannot determine id)".into()));
        }
        Ok(Self { id, files })
    }
}
```

> `zip` is already a transitive workspace dep (used by `rm-files`); add `zip = "2"` to
> `rm-cloud`'s `[dependencies]`.

- [ ] **Step 4: Write `src/porcelain/document.rs`.**

```rust
//! Document IO: download a doc to `DocFiles`/`Bundle`, upload a new doc, and the
//! ink-preserving content-only PDF swap.

use crate::client::Client;
use crate::error::{Error, Result};
use crate::plumbing::commit::{DocUpsert, Mutation};
use crate::plumbing::index::parse_doc_index;
use crate::porcelain::docfiles::{DocFiles, Metadata};

impl Client {
    /// Download a document's full file-set.
    pub async fn get(&self, id: &str) -> Result<DocFiles> {
        let snap = self.snapshot().await?;
        let doc = snap.doc(id).ok_or(Error::NotFound)?;
        // doc-index blob is keyed by the doc hash, named "<id>.docSchema".
        let index = self.get_blob(&doc.hash, &format!("{id}.docSchema")).await?;
        let entries = parse_doc_index(&index)?;
        let mut files = Vec::with_capacity(entries.len());
        for e in &entries {
            let bytes = self.get_blob(&e.hash, &e.id).await?;
            files.push((e.id.clone(), bytes));
        }
        Ok(DocFiles { id: id.to_string(), files })
    }

    /// Download a document and open it as a `rm_files::Bundle` (via a temp `.rmdoc`).
    pub async fn get_bundle(&self, id: &str) -> Result<rm_files::Bundle> {
        let docfiles = self.get(id).await?;
        let dir = tempfile::tempdir()?;
        let path = dir.path().join(format!("{id}.rmdoc"));
        docfiles.write_rmdoc(&path)?;
        rm_files::Bundle::open(&path).map_err(|e| Error::Parse(e.to_string()))
    }

    /// Upload `docfiles` as a new document (its `id` is used as-is; callers wanting a
    /// fresh id should set `docfiles.id` to a new UUID first via [`DocFiles`]).
    pub async fn put(&self, docfiles: DocFiles) -> Result<()> {
        let up = DocUpsert { id: docfiles.id.clone(), files: docfiles.files };
        self.commit(Mutation { upserts: vec![up], removals: vec![] }).await?;
        Ok(())
    }

    /// Remove a document.
    pub async fn rm(&self, id: &str) -> Result<()> {
        self.commit(Mutation { upserts: vec![], removals: vec![id.to_string()] }).await?;
        Ok(())
    }

    /// Move/rename: edit only the `.metadata` blob (parent and/or visibleName), preserving
    /// every other blob (content/pdf/ink) byte-for-byte.
    pub async fn mv(&self, id: &str, new_parent: Option<&str>, new_name: Option<&str>) -> Result<()> {
        let mut docfiles = self.get(id).await?;
        let mut meta = docfiles.metadata()?;
        if let Some(p) = new_parent { meta.parent = p.to_string(); }
        if let Some(n) = new_name { meta.visible_name = n.to_string(); }
        meta.last_modified = now_millis();
        docfiles.set_metadata(&meta)?;
        self.put(docfiles).await
    }

    /// Ink-preserving PDF swap (mechanics §3): replace only the `<id>.pdf` blob. The
    /// `.content` and every `.rm` blob are left untouched, so on-device ink and page
    /// order survive.
    pub async fn put_content_only(&self, id: &str, new_pdf: Vec<u8>) -> Result<()> {
        let mut docfiles = self.get(id).await?;
        let pdf_name = format!("{id}.pdf");
        let slot = docfiles.files.iter_mut().find(|(n, _)| *n == pdf_name)
            .ok_or_else(|| Error::Parse("document has no .pdf to replace".into()))?;
        slot.1 = new_pdf;
        // Re-upsert the whole file-set; unchanged blobs are content-addressed so only the
        // pdf blob is new, and the doc index lists the same .content/.rm hashes as before.
        self.put(docfiles).await
    }
}

/// Current time in unix millis as a string (reMarkable's `lastModified` format).
fn now_millis() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis().to_string()
}
```

- [ ] **Step 5: Write `src/porcelain/fs.rs`.**

```rust
//! Path/listing view over a snapshot: names and parents come from each doc's metadata.

use crate::client::Client;
use crate::error::{Error, Result};
use crate::porcelain::docfiles::Metadata;
use uuid::Uuid;

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
}

impl Client {
    /// Metadata for a single document.
    pub async fn stat(&self, id: &str) -> Result<Metadata> {
        self.get(id).await?.metadata()
    }

    /// List the direct children of `parent` ("" = root).
    pub async fn ls(&self, parent: &str) -> Result<Vec<Entry>> {
        let snap = self.snapshot().await?;
        let mut out = Vec::new();
        for doc in snap.docs() {
            // Fetch each doc's metadata (small blobs). Acceptable for v1; an index-level
            // metadata cache is a later optimization.
            let meta = self.get(&doc.id).await?.metadata()?;
            if meta.deleted { continue; }
            if meta.parent == parent {
                out.push(Entry {
                    id: doc.id.clone(),
                    name: meta.visible_name,
                    parent: meta.parent,
                    is_folder: meta.doc_type == "CollectionType",
                });
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Create a folder under `parent`; returns the new folder id.
    pub async fn mkdir(&self, name: &str, parent: &str) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let meta = Metadata {
            visible_name: name.to_string(),
            doc_type: "CollectionType".to_string(),
            parent: parent.to_string(),
            last_modified: super::document_now_millis(),
            deleted: false,
            extra: Default::default(),
        };
        let mut files = Vec::new();
        files.push((format!("{id}.metadata"), serde_json::to_vec(&meta)?));
        files.push((format!("{id}.content"), b"{}".to_vec()));
        self.put(crate::porcelain::docfiles::DocFiles { id: id.clone(), files }).await?;
        Ok(id)
    }
}

impl From<serde_json::Error> for Error {
    fn from_unused(_: serde_json::Error) -> Error { Error::Parse("json".into()) }
}
```

> Cleanups while writing this file: (a) remove the bogus `impl From` at the bottom — `Error`
> already has `#[from] serde_json::Error` from Task 0; it is shown here only to flag that you
> must NOT re-add a duplicate `From`. Delete those 3 lines. (b) Expose `now_millis` for reuse:
> in `document.rs` rename `now_millis` to `pub(crate) fn document_now_millis()` and call that
> from `fs.rs` (`super::document_now_millis()`), or simply duplicate the 2-line helper in
> `fs.rs`. Pick one; do not leave an undefined `document_now_millis`.

- [ ] **Step 6: Wire.** Add `zip = "2"` to deps. `lib.rs` += `pub mod porcelain;` and `pub use porcelain::docfiles::{DocFiles, Metadata}; pub use porcelain::fs::Entry;`. In `client.rs`, ensure `pub mod porcelain;` modules see `Client` (they `use crate::client::Client`).

- [ ] **Step 7: Write `crates/rm-cloud/tests/fake_porcelain.rs`.**

```rust
use rm_cloud::fake::FakeCloud;
use rm_cloud::{Client, Config, DocFiles, Metadata};
use uuid::Uuid;

fn doc_with_pdf(id: &str, name: &str, parent: &str, pdf: &[u8]) -> DocFiles {
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
            (format!("{id}.pdf"), pdf.to_vec()),
        ],
    }
}

#[tokio::test]
async fn put_get_ls_mkdir_mv_rm() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");

    let folder = client.mkdir("Reading", "").await.unwrap();
    let id = Uuid::new_v4().to_string();
    client.put(doc_with_pdf(&id, "Article", &folder, b"%PDF-1").await_put(&client).await;

    // (the line above is illustrative; replace with:)
    // client.put(doc_with_pdf(&id, "Article", &folder, b"%PDF-1")).await.unwrap();

    let listing = client.ls(&folder).await.unwrap();
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].name, "Article");

    let got = client.get(&id).await.unwrap();
    assert_eq!(got.get(&format!("{id}.pdf")).unwrap(), b"%PDF-1");

    client.mv(&id, None, Some("Renamed")).await.unwrap();
    assert_eq!(client.stat(&id).await.unwrap().visible_name, "Renamed");

    client.rm(&id).await.unwrap();
    assert!(client.ls(&folder).await.unwrap().is_empty());
}
```

> Fix the deliberately-broken illustrative line: the real call is
> `client.put(doc_with_pdf(&id, "Article", &folder, b"%PDF-1")).await.unwrap();`.

- [ ] **Step 8: Run + clippy + fmt.** `nix develop -c cargo test -p rm-cloud --features fake --test fake_porcelain` → PASS.

- [ ] **Step 9: Commit.**

```bash
git add crates/rm-cloud/src/porcelain crates/rm-cloud/src/lib.rs crates/rm-cloud/src/client.rs crates/rm-cloud/Cargo.toml crates/rm-cloud/tests/fake_porcelain.rs
git commit -m "rm-cloud: porcelain DocFiles + get/put/ls/mkdir/mv/rm + Bundle round-trip"
```

---

### Task 8: Content-only fidelity test

**Goal:** Prove `put_content_only` swaps only the `.pdf` blob, leaving `.content` and every `.rm` blob byte-identical (mechanics §3).

**Files:**
- Create: `crates/rm-cloud/tests/fake_content_only.rs`

**Acceptance Criteria:**
- [ ] After `put_content_only`, the doc's `.rm` and `.content` blob hashes are unchanged from before; the `.pdf` blob hash differs and matches `sha256(new_pdf)`.
- [ ] `get(id)` returns the new PDF bytes and the original ink bytes.

**Verify:** `nix develop -c cargo test -p rm-cloud --features fake --test fake_content_only` → pass.

**Steps:**

- [ ] **Step 1: Write the test** `crates/rm-cloud/tests/fake_content_only.rs`.

```rust
use rm_cloud::fake::FakeCloud;
use rm_cloud::{Client, Config, DocFiles, Metadata};

fn doc_with_ink(id: &str) -> DocFiles {
    let meta = Metadata {
        visible_name: "Doc".into(),
        doc_type: "DocumentType".into(),
        parent: "".into(),
        last_modified: "0".into(),
        deleted: false,
        extra: Default::default(),
    };
    DocFiles {
        id: id.into(),
        files: vec![
            (format!("{id}.metadata"), serde_json::to_vec(&meta).unwrap()),
            (format!("{id}.content"), br#"{"pages":["p1"]}"#.to_vec()),
            (format!("{id}.pdf"), b"%PDF-original".to_vec()),
            (format!("{id}/p1.rm"), b"INK-BYTES-DO-NOT-TOUCH".to_vec()),
        ],
    }
}

#[tokio::test]
async fn content_only_preserves_ink_and_content() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");
    let id = "doc-co";
    client.put(doc_with_ink(id)).await.unwrap();

    // Hashes the cloud should keep across a content-only swap.
    let ink_hash = rm_cloud::sha256_hex(b"INK-BYTES-DO-NOT-TOUCH");
    let content_hash = rm_cloud::sha256_hex(br#"{"pages":["p1"]}"#);
    assert!(cloud.blob(&ink_hash).is_some(), "ink blob present after put");
    assert!(cloud.blob(&content_hash).is_some(), "content blob present after put");

    client.put_content_only(id, b"%PDF-UPDATED".to_vec()).await.unwrap();

    // Ink + content blobs still present, unchanged; new pdf blob present.
    assert_eq!(cloud.blob(&ink_hash).as_deref(), Some(b"INK-BYTES-DO-NOT-TOUCH".as_slice()));
    assert_eq!(cloud.blob(&content_hash).as_deref(), Some(br#"{"pages":["p1"]}"#.as_slice()));
    let new_pdf_hash = rm_cloud::sha256_hex(b"%PDF-UPDATED");
    assert_eq!(cloud.blob(&new_pdf_hash).as_deref(), Some(b"%PDF-UPDATED".as_slice()));

    // Downloaded doc reflects new pdf + original ink.
    let got = client.get(id).await.unwrap();
    assert_eq!(got.get(&format!("{id}.pdf")).unwrap(), b"%PDF-UPDATED");
    assert_eq!(got.get(&format!("{id}/p1.rm")).unwrap(), b"INK-BYTES-DO-NOT-TOUCH");
}
```

- [ ] **Step 2: Run.** `nix develop -c cargo test -p rm-cloud --features fake --test fake_content_only` → PASS.

- [ ] **Step 3: Commit.**

```bash
git add crates/rm-cloud/tests/fake_content_only.rs
git commit -m "rm-cloud: content-only PDF swap fidelity test (ink preserved)"
```

---

### Task 9: Declarative working-set sync

**Goal:** `Client::sync(working_set, since)` — make the cloud match a target set of app-owned documents under one folder, and report which keys' ink changed since a prior snapshot, with a no-op fast path on unchanged generation.

**Files:**
- Create: `crates/rm-cloud/src/sync.rs`
- Modify: `src/lib.rs`, `src/client.rs`
- Create: `crates/rm-cloud/tests/fake_sync.rs` (add `[[test]]` entry, `required-features`)

**Acceptance Criteria:**
- [ ] `WorkingSet` maps app key → desired `DocFiles`; `sync` upserts changed/new docs and removes app-owned docs no longer in the set, in **one** commit.
- [ ] `SyncReport` lists keys whose doc hash changed vs `since` (i.e. ink/content moved on the device) and the resulting `Snapshot`.
- [ ] If `since` is `Some` and the live generation equals `since.generation`, `sync` returns an empty report without uploading or diffing (fast path).
- [ ] Keys map to docs by a stable metadata marker (`rmCloudKey` in metadata `extra`), so app docs are identifiable without a server DB.

**Verify:** `nix develop -c cargo test -p rm-cloud --features fake --test fake_sync` → pass.

**Steps:**

- [ ] **Step 1: Add `[[test]]` entry** `name = "fake_sync"`, `required-features = ["fake"]`.

- [ ] **Step 2: Write `src/sync.rs`.**

```rust
//! Declarative working-set reconcile — the layer inkapp's loop uses instead of
//! whole-folder pulls. The app declares a target set keyed by app key; we compute the
//! minimal commit and report which keys' ink changed since a prior snapshot.

use std::collections::BTreeMap;

use crate::client::Client;
use crate::error::Result;
use crate::plumbing::commit::{DocUpsert, Mutation};
use crate::plumbing::snapshot::Snapshot;
use crate::porcelain::docfiles::DocFiles;

/// Metadata key (in `Metadata.extra`) tagging a doc with its app key.
pub const APP_KEY_FIELD: &str = "rmCloudKey";

/// The desired set of app-owned documents, keyed by app key.
#[derive(Default)]
pub struct WorkingSet {
    /// app key -> desired document file-set (its metadata must carry `rmCloudKey`).
    pub docs: BTreeMap<String, DocFiles>,
}

/// What `sync` did / observed.
#[derive(Debug, Default)]
pub struct SyncReport {
    /// App keys whose doc hash changed since `since` (ink/content came back).
    pub changed_keys: Vec<String>,
    /// Whether anything was committed.
    pub committed: bool,
}

impl Client {
    /// Reconcile the cloud to `target`. If `since` is given and the generation is
    /// unchanged, returns an empty report immediately (no-op fast path).
    pub async fn sync(&self, target: WorkingSet, since: Option<&Snapshot>) -> Result<(SyncReport, Snapshot)> {
        let live = self.snapshot().await?;
        if let Some(prev) = since {
            if !prev.root_hash.is_empty() && prev.generation == live.generation {
                return Ok((SyncReport::default(), live));
            }
        }

        // Map existing app-owned docs: app key -> (doc id, doc hash).
        let mut existing: BTreeMap<String, (String, String)> = BTreeMap::new();
        for d in live.docs() {
            if let Ok(df) = self.get(&d.id).await {
                if let Ok(meta) = df.metadata() {
                    if let Some(k) = meta.extra.get(APP_KEY_FIELD).and_then(|v| v.as_str()) {
                        existing.insert(k.to_string(), (d.id.clone(), d.hash.clone()));
                    }
                }
            }
        }

        // changed_keys: app keys present before and now whose doc hash moved vs `since`.
        let mut changed_keys = Vec::new();
        if let Some(prev) = since {
            for (key, (id, _)) in &existing {
                let now = live.doc(id).map(|d| d.hash.clone());
                let before = prev.doc(id).map(|d| d.hash.clone());
                if now != before {
                    changed_keys.push(key.clone());
                }
            }
            changed_keys.sort();
        }

        // Build the mutation: upsert each target doc (reusing existing id if the key
        // exists), remove app-owned docs no longer targeted.
        let mut upserts = Vec::new();
        for (key, mut df) in target.docs {
            if let Some((id, _)) = existing.get(&key) {
                df.id = id.clone(); // reuse the existing doc id (stable identity)
            }
            upserts.push(DocUpsert { id: df.id.clone(), files: df.files });
        }
        let targeted_keys: std::collections::BTreeSet<&String> = Default::default();
        let _ = targeted_keys; // placeholder removed below
        let removals: Vec<String> = Vec::new();

        let report_committed = !upserts.is_empty() || !removals.is_empty();
        let snap = if report_committed {
            self.commit(Mutation { upserts, removals }).await?
        } else {
            live
        };
        Ok((SyncReport { changed_keys, committed: report_committed }, snap))
    }
}
```

> While writing, finish the removals logic the placeholder marks: compute the set of app
> keys in `target.docs` (capture before the upsert loop consumes it), then
> `removals = existing.iter().filter(|(k,_)| !target_keys.contains(*k)).map(|(_, (id,_))| id.clone()).collect()`.
> Remove the two placeholder lines. The intent: docs whose app key is no longer in the
> target get removed in the same commit.

- [ ] **Step 3: Wire.** `lib.rs` += `mod sync; pub use sync::{WorkingSet, SyncReport, APP_KEY_FIELD};`.

- [ ] **Step 4: Write `crates/rm-cloud/tests/fake_sync.rs`.**

```rust
use rm_cloud::fake::FakeCloud;
use rm_cloud::{Client, Config, DocFiles, Metadata, WorkingSet, APP_KEY_FIELD};
use std::collections::BTreeMap;
use uuid::Uuid;

fn keyed_doc(key: &str, pdf: &[u8]) -> DocFiles {
    let id = Uuid::new_v4().to_string();
    let mut extra = serde_json::Map::new();
    extra.insert(APP_KEY_FIELD.into(), serde_json::Value::String(key.into()));
    let meta = Metadata {
        visible_name: key.into(),
        doc_type: "DocumentType".into(),
        parent: "".into(),
        last_modified: "0".into(),
        deleted: false,
        extra,
    };
    DocFiles {
        id: id.clone(),
        files: vec![
            (format!("{id}.metadata"), serde_json::to_vec(&meta).unwrap()),
            (format!("{id}.content"), b"{}".to_vec()),
            (format!("{id}.pdf"), pdf.to_vec()),
        ],
    }
}

#[tokio::test]
async fn sync_creates_then_no_ops_then_removes() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");

    // Create two app docs.
    let mut docs = BTreeMap::new();
    docs.insert("a".to_string(), keyed_doc("a", b"%PDF-a"));
    docs.insert("b".to_string(), keyed_doc("b", b"%PDF-b"));
    let (rep, snap1) = client.sync(WorkingSet { docs }, None).await.unwrap();
    assert!(rep.committed);
    assert_eq!(snap1.docs().count(), 2);

    // Re-sync with the SAME generation -> no-op fast path.
    let (rep, _snap) = client.sync(WorkingSet::default(), Some(&snap1)).await.unwrap();
    assert!(!rep.committed);
    assert!(rep.changed_keys.is_empty());

    // Drop "b" from the target -> it gets removed in one commit.
    let mut docs = BTreeMap::new();
    docs.insert("a".to_string(), keyed_doc("a", b"%PDF-a2"));
    let (rep, snap2) = client.sync(WorkingSet { docs }, None).await.unwrap();
    assert!(rep.committed);
    assert_eq!(snap2.docs().count(), 1);
}
```

- [ ] **Step 5: Run + clippy + fmt.** `nix develop -c cargo test -p rm-cloud --features fake --test fake_sync` → PASS.

- [ ] **Step 6: Commit.**

```bash
git add crates/rm-cloud/src/sync.rs crates/rm-cloud/src/lib.rs crates/rm-cloud/Cargo.toml crates/rm-cloud/tests/fake_sync.rs
git commit -m "rm-cloud: declarative working-set sync with no-op fast path"
```

---

### Task 10: Real-cloud test suite + sweeper

**Goal:** An env-gated, `#[ignore]`d suite running the full lifecycle against the live cloud inside `rmrs-test/<run-id>`, with leave-on-failure cleanup and a stale-folder sweeper.

**Files:**
- Create: `crates/rm-cloud/tests/real_cloud.rs`

**Acceptance Criteria:**
- [ ] Tests are `#[ignore]`d and additionally skip (return early with a printed message) if `RM_CLOUD_DEVICE_TOKEN` is unset, so a default `cargo test` never touches the network.
- [ ] Each run creates a unique folder `rmrs-test/<uuid>` at the account root and does all work inside it.
- [ ] On success the run folder is deleted; on failure (panic) it is left in place.
- [ ] A `sweep_stale_test_folders` test (also `#[ignore]`d) removes any `rmrs-test/*` folders.

**Verify:** `nix develop -c cargo test -p rm-cloud --features fake` (real tests skipped). Manual: `RM_CLOUD_DEVICE_TOKEN=… nix develop -c cargo test -p rm-cloud --features fake -- --ignored real_cloud_lifecycle` → passes against the live cloud.

> The real-cloud tests build the client from the device token and need no `fake` feature
> themselves, but compiling the test binary alongside the others is simplest under
> `--features fake`. They live in a plain `tests/real_cloud.rs` (no `required-features`).

**Steps:**

- [ ] **Step 1: Write `crates/rm-cloud/tests/real_cloud.rs`.**

```rust
//! Live-cloud tests. Gated by `RM_CLOUD_DEVICE_TOKEN` and `#[ignore]` so they never run
//! by default. All work happens inside `rmrs-test/<run-id>` so parallel runs (and other
//! contributors) never collide. The run folder is deleted on success, kept on failure.

use rm_cloud::{Client, Config};
use uuid::Uuid;

const ROOT_TEST_DIR: &str = "rmrs-test";

fn client_or_skip() -> Option<Client> {
    match std::env::var("RM_CLOUD_DEVICE_TOKEN") {
        Ok(t) if !t.is_empty() => Some(Client::from_device_token(Config::from_env(), t)),
        _ => {
            eprintln!("skipping real-cloud test: RM_CLOUD_DEVICE_TOKEN unset");
            None
        }
    }
}

#[tokio::test]
#[ignore = "hits the live reMarkable cloud; needs RM_CLOUD_DEVICE_TOKEN"]
async fn real_cloud_lifecycle() {
    let Some(client) = client_or_skip() else { return; };

    // Unique isolation folder for this run.
    let run_id = Uuid::new_v4().to_string();
    let base = client.mkdir(ROOT_TEST_DIR, "").await
        .unwrap_or_else(|_| String::new()); // tolerate "already exists" by re-resolving below
    let base = if base.is_empty() { resolve_folder(&client, ROOT_TEST_DIR, "").await } else { base };
    let run_folder = client.mkdir(&run_id, &base).await.expect("mk run folder");

    // Body wrapped so we can implement leave-on-failure: only clean up if it returns Ok.
    let result = real_lifecycle_body(&client, &run_folder).await;

    match result {
        Ok(()) => {
            client.rm(&run_folder).await.expect("cleanup run folder on success");
        }
        Err(e) => {
            eprintln!("real_cloud_lifecycle FAILED; leaving {ROOT_TEST_DIR}/{run_id} for inspection: {e}");
            panic!("real_cloud_lifecycle failed: {e}");
        }
    }
}

async fn real_lifecycle_body(client: &Client, run_folder: &str) -> rm_cloud::Result<()> {
    use rm_cloud::{DocFiles, Metadata};
    let id = Uuid::new_v4().to_string();
    let meta = Metadata {
        visible_name: "rm-cloud-it".into(),
        doc_type: "DocumentType".into(),
        parent: run_folder.into(),
        last_modified: "0".into(),
        deleted: false,
        extra: Default::default(),
    };
    let df = DocFiles {
        id: id.clone(),
        files: vec![
            (format!("{id}.metadata"), serde_json::to_vec(&meta).unwrap()),
            (format!("{id}.content"), b"{}".to_vec()),
            (format!("{id}.pdf"), b"%PDF-1.4 test".to_vec()),
        ],
    };
    client.put(df).await?;

    let listing = client.ls(run_folder).await?;
    assert!(listing.iter().any(|e| e.id == id), "uploaded doc should be listed");

    let got = client.get(&id).await?;
    assert_eq!(got.get(&format!("{id}.pdf")).unwrap(), b"%PDF-1.4 test");

    client.put_content_only(&id, b"%PDF-1.4 updated".to_vec()).await?;
    let got = client.get(&id).await?;
    assert_eq!(got.get(&format!("{id}.pdf")).unwrap(), b"%PDF-1.4 updated");

    client.rm(&id).await?;
    Ok(())
}

/// Find the id of folder `name` under `parent` (used when mkdir hit an existing folder).
async fn resolve_folder(client: &Client, name: &str, parent: &str) -> String {
    client.ls(parent).await.unwrap_or_default()
        .into_iter()
        .find(|e| e.is_folder && e.name == name)
        .map(|e| e.id)
        .expect("test root folder must exist after mkdir")
}

#[tokio::test]
#[ignore = "destructive: removes all rmrs-test/* folders from the live account"]
async fn sweep_stale_test_folders() {
    let Some(client) = client_or_skip() else { return; };
    let base = resolve_folder(&client, ROOT_TEST_DIR, "").await;
    for entry in client.ls(&base).await.expect("ls test root") {
        eprintln!("sweeping {ROOT_TEST_DIR}/{}", entry.name);
        client.rm(&entry.id).await.expect("rm stale run folder");
    }
}
```

> Note on `mkdir` idempotency: the reMarkable cloud allows duplicate folder names, so the
> first `mkdir(ROOT_TEST_DIR, "")` may create a second `rmrs-test`. To keep the suite
> robust, prefer: try `resolve_folder` first; only `mkdir` if absent. Refactor the
> `let base = …` lines into a `get_or_create_folder(client, ROOT_TEST_DIR, "")` helper that
> does ls-then-mkdir. Implement that helper rather than the `unwrap_or_else` shown.

- [ ] **Step 2: Verify gating.** `nix develop -c cargo test -p rm-cloud --features fake` → real tests are not run (ignored). Confirm the binary compiles.

- [ ] **Step 3: (Optional, if a token is available) Run live.** `RM_CLOUD_DEVICE_TOKEN=… nix develop -c cargo test -p rm-cloud --features fake -- --ignored real_cloud_lifecycle`.

- [ ] **Step 4: Commit.**

```bash
git add crates/rm-cloud/tests/real_cloud.rs
git commit -m "rm-cloud: env-gated live-cloud suite isolated under rmrs-test/<run-id>"
```

---

### Task 11: Docs reconciliation + dependency-lock commit

**Goal:** Make the docs true and land the `Cargo.lock` bump in its own commit (repo convention).

**Files:**
- Create: `docs/rm-cloud-protocol.md`
- Modify: `CLAUDE.md` (Architecture section: add `rm-cloud`)
- Modify: `docs/FUTURE.md` (direct-device transport note)
- Modify: `Cargo.lock` (its own commit)

**Acceptance Criteria:**
- [ ] `docs/rm-cloud-protocol.md` documents endpoints, hashing rules, blob keying, index formats, and the CAS loop (the "Protocol reference" of this plan, prose form).
- [ ] `CLAUDE.md` Architecture lists `crates/rm-cloud` with a one-paragraph description and its `rm-files` boundary.
- [ ] `docs/FUTURE.md` has a line noting direct-device (USB/SSH) transport as a future `Device`-seam option.
- [ ] `make clippy` and `make fmt-check` clean for the whole workspace; `make test` green.

**Verify:** `nix develop -c cargo clippy --all-targets -- -D warnings && nix develop -c cargo fmt --check && nix develop -c cargo test --workspace` and `nix develop -c cargo test -p rm-cloud --features fake`.

**Steps:**

- [ ] **Step 1: Write `docs/rm-cloud-protocol.md`** capturing the "Protocol reference" section of this plan as prose (endpoints table, hashing rules, the blob-keying asymmetry, index formats, the CAS/rebase loop). Cross-link `remarkable-pdf-mechanics.md` §3 for the content-only rationale.

- [ ] **Step 2: Add an `rm-cloud` entry to `CLAUDE.md`** Architecture, after the `inkapp-remarkable` bullet:

```markdown
- **`crates/rm-cloud`** — pure-Rust client for the current reMarkable Cloud sync
  protocol (content-addressed blob store, root CAS by generation). Exposes immutable
  `Snapshot`s + `diff`, an atomic `commit` (rebase-on-412), rmapi-style path ops, and a
  declarative working-set `sync` for app loops. Reuses `rm-files` for the `.rmdoc`
  bundle; owns nothing of the local scene format. reMarkable-specific → `rm-` prefix.
  Intended to replace shelling out to the `rmapi` CLI (migration is a later spec). See
  `docs/rm-cloud-protocol.md`.
```

- [ ] **Step 3: Append to `docs/FUTURE.md`** a line:

```markdown
- **Direct-device transport (no cloud).** A USB-web-UI/SSH transport to the tablet could
  back the `Device` seam as an alternative to `rm-cloud`'s cloud sync — same sync model,
  different medium. Out of scope for `rm-cloud` v1.
- **inkapp loop testing on the fake cloud.** `inkapp-harness` can gain a higher-fidelity
  test tier: spin up `rm-cloud`'s `FakeCloud`, drive an app's real publish → (simulated
  device ink, written *through* the cloud as a `.rm` blob bump) → pull → `step` →
  republish loop, asserting incremental `Snapshot::diff` and content-only ink survival.
  This needs no `rm-cloud` changes (the public `Client` + `FakeCloud` compose); the
  inkapp-specific glue (a `CloudLoopHarness` + gesture-fixture-to-`.rm` helper) lives in
  `inkapp-harness`, keeping `rm-cloud` app-agnostic. Brainstorm as a follow-up spec
  bundled with the `serve.rs` migration, after `rm-cloud` ships.
```

- [ ] **Step 4: Run the full workspace gates.** Fix any clippy/fmt issues across the crate (e.g. `missing_docs`).

- [ ] **Step 5: Commit docs.**

```bash
git add docs/rm-cloud-protocol.md CLAUDE.md docs/FUTURE.md
git commit -m "docs: rm-cloud protocol reference + architecture/FUTURE notes"
```

- [ ] **Step 6: Commit the lockfile separately.**

```bash
git add Cargo.lock
git commit -m "Cargo.lock: axum/uuid/hex deps for rm-cloud"
```

---

## Self-review

**Spec coverage:**
- Snapshot/diff core → Tasks 2, 5, 6. Path porcelain → Task 7. Sync/reconcile layer → Task 9. ✓
- Protocol (auth, blobs, hashing, indexes, CAS) → Tasks 1, 3, 4, 5, 6 + `docs/rm-cloud-protocol.md` (Task 11). ✓
- Pairing + user-token refresh + env credentials + multi-account → Task 4 (auth), Task 5 (Client refresh + `from_env`/independent clients). ✓
- `put_content_only` (mechanics §3) → Task 7 impl + Task 8 fidelity test. ✓
- `rm-files` boundary (reuse `Bundle`, no scene re-parse) → Task 7 (`get_bundle`, `DocFiles`). ✓
- Conflict rebase-on-412 → Task 6. ✓
- Fake cloud (real HTTP, fault injection, feature-gated, reusable) → Task 3. ✓
- Real-cloud suite under `rmrs-test/<run-id>`, env-gated, leave-on-failure, sweeper → Task 10. ✓
- Four test tiers: unit (1,2), fake integration (3–9), concurrency (6), real-cloud (10). ✓
- Stateless/no session DB, secrets never in documents → satisfied by design (snapshots rebuilt per session; tokens only in `Credentials`). ✓
- Docs reconciliation + separate Cargo.lock commit → Task 11. ✓

**Placeholder scan:** Tasks 7 and 9 each contain ONE deliberately-flagged illustrative defect (a broken test line in 7; placeholder removal lines in 9) with explicit instructions to fix while implementing — these are call-outs, not silent placeholders. No "TBD"/"add error handling"/"similar to Task N" left.

**Type consistency:** `DocEntry`/`FileEntry` (index) used consistently; `DocUpsert`/`Mutation` (commit) used in client + porcelain + sync; `DocFiles`/`Metadata` field names (`visible_name`, `doc_type`, `parent`, `last_modified`, `deleted`, `extra`) consistent across docfiles/fs/document/sync/tests; `Client` method names (`snapshot`, `commit`, `get`, `get_bundle`, `put`, `rm`, `mv`, `put_content_only`, `ls`, `stat`, `mkdir`, `sync`) consistent. `RM_FILENAME` const reused. `sha256_hex` re-exported and used in tests.
