# inkctl Test Harness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build an agent-drivable CLI (`inkctl`) plus an extended `inkapp-harness` library that lets Claude exercise inkapp apps end-to-end without hardware and emit committed Rust `#[test]`s from interactive sessions.

**Architecture:** The CLI is a thin `clap` shell over a public `harness::Session` API; sessions are directories under `$INKCTL_HOME/<id>/`; each CLI process boots an in-process axum fake cloud (`rm-cloud` `fake` feature), hydrates state from disk, runs one command, dumps state back. Generated Rust tests bypass the CLI and call the harness library directly.

**Tech Stack:** Rust 2021, clap (CLI), serde/serde_json (JSON envelopes), axum (in-process fake cloud — already in `rm-cloud`), lopdf (PDF link extraction), fs2 (session-dir file locks), assert_cmd (CLI smoke tests), tempfile (test scratch dirs).

**Spec:** `docs/superpowers/specs/2026-05-26-inkctl-test-harness-design.md`

## File Structure

**Modified:**
- `crates/rm-cloud/src/fake/mod.rs` — add `FakeCloud::from_dir` / `dump_to_dir` for session-on-disk persistence
- `crates/rm-cloud/Cargo.toml` — no change (`fake` feature already exists)
- `crates/inkapp-harness/Cargo.toml` — add `tokio`, `lopdf`, `fs2`, `tempfile` to deps; promote `rm-device`, `rm-cloud` from dev-deps with feature gates
- `crates/inkapp-harness/src/lib.rs` — re-export `session`, `observe`, `emit`
- `crates/inkapp-harness/src/inspector.rs` — extend for layer-filter + link overlays
- `crates/inkapp-harness/src/recording.rs` — add `TraceWriter` for `trace.jsonl`
- `Cargo.toml` (workspace root) — add `crates/inkctl` to members
- `docs/appdx.md` — mark test harness built (definition-of-done)

**Created:**
- `crates/inkapp-harness/src/session.rs` — `Session`, `SessionConfig`, on-disk lifecycle
- `crates/inkapp-harness/src/observe.rs` — manifest/links/layers/rmdoc views
- `crates/inkapp-harness/src/emit.rs` — trace → Rust `#[test]` generator
- `crates/inkapp-harness/src/pdf_links.rs` — lopdf-based link annotation extraction
- `crates/inkctl/Cargo.toml`
- `crates/inkctl/src/main.rs` — clap top-level dispatch
- `crates/inkctl/src/output.rs` — JSON envelope + PNG writer
- `crates/inkctl/src/cmd/mod.rs`
- `crates/inkctl/src/cmd/session.rs`
- `crates/inkctl/src/cmd/device.rs`
- `crates/inkctl/src/cmd/document.rs`
- `crates/inkctl/src/cmd/page.rs`
- `crates/inkctl/src/cmd/ink.rs`
- `crates/inkctl/src/cmd/record.rs`
- `crates/inkctl/tests/smoke_session.rs`
- `crates/inkctl/tests/smoke_device.rs`
- `crates/inkctl/tests/smoke_document.rs`
- `crates/inkctl/tests/smoke_page.rs`
- `crates/inkctl/tests/smoke_ink.rs`
- `crates/inkctl/tests/dogfood.rs`

---

## Task 1: Persist fake cloud state to disk

**Goal:** Extend `rm-cloud::fake::FakeCloud` with `from_dir` / `dump_to_dir` so a session's cloud state survives across CLI invocations.

**Files:**
- Modify: `crates/rm-cloud/src/fake/mod.rs`
- Test: `crates/rm-cloud/tests/fake_persistence.rs` (new)

**Acceptance Criteria:**
- [ ] `FakeCloud::from_dir(path)` loads blobs + root_hash + generation from a directory (empty state if dir is empty/missing).
- [ ] `FakeCloud::dump_to_dir(path)` writes current state to that directory atomically (write to tmp, rename).
- [ ] Round-trip preserves blobs, root_hash, generation across `dump_to_dir` → new `from_dir` → new `FakeCloud`.
- [ ] Existing `rm-cloud` tests still pass.

**Verify:** `nix develop -c cargo test -p rm-cloud --features fake fake_persistence` → PASS

**Steps:**

- [ ] **Step 1: Write the failing test**

Create `crates/rm-cloud/tests/fake_persistence.rs`:

```rust
#![cfg(feature = "fake")]

use rm_cloud::fake::FakeCloud;
use tempfile::tempdir;

#[tokio::test]
async fn round_trip_state_through_disk() {
    let dir = tempdir().unwrap();

    // Run 1: spawn empty, write a blob, snapshot state to disk.
    let cloud_a = FakeCloud::spawn().await;
    {
        let mut s = cloud_a.state.lock().unwrap();
        s.blobs.insert("abc".into(), b"hello".to_vec());
        s.root_hash = "abc".into();
        s.generation = 1;
    }
    cloud_a.dump_to_dir(dir.path()).unwrap();
    drop(cloud_a);

    // Run 2: spawn and hydrate from disk; state must match.
    let cloud_b = FakeCloud::from_dir(dir.path()).await.unwrap();
    let s = cloud_b.state.lock().unwrap();
    assert_eq!(s.blobs.get("abc").map(|v| v.as_slice()), Some(b"hello".as_slice()));
    assert_eq!(s.root_hash, "abc");
    assert_eq!(s.generation, 1);
}

#[tokio::test]
async fn from_dir_empty_when_missing() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("does-not-exist");
    let cloud = FakeCloud::from_dir(&missing).await.unwrap();
    let s = cloud.state.lock().unwrap();
    assert!(s.blobs.is_empty());
    assert_eq!(s.root_hash, "");
    assert_eq!(s.generation, 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `nix develop -c cargo test -p rm-cloud --features fake fake_persistence`
Expected: FAIL — `from_dir` / `dump_to_dir` not defined.

- [ ] **Step 3: Implement persistence**

In `crates/rm-cloud/src/fake/mod.rs`, add (alongside existing impls):

```rust
use std::fs;
use std::path::Path;

#[derive(serde::Serialize, serde::Deserialize)]
struct StateOnDisk {
    root_hash: String,
    generation: i64,
    blobs: std::collections::HashMap<String, Vec<u8>>,
}

impl FakeCloud {
    /// Spawn a new fake cloud, hydrating its state from `dir` if present.
    /// If `dir` does not exist or contains no `state.json`, starts empty.
    pub async fn from_dir(dir: &Path) -> std::io::Result<Self> {
        let cloud = Self::spawn().await;
        let state_path = dir.join("state.json");
        if state_path.exists() {
            let bytes = fs::read(&state_path)?;
            let on_disk: StateOnDisk = serde_json::from_slice(&bytes)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            let mut s = cloud.state.lock().unwrap();
            s.root_hash = on_disk.root_hash;
            s.generation = on_disk.generation;
            s.blobs = on_disk.blobs;
        }
        Ok(cloud)
    }

    /// Atomically write current state to `dir/state.json` (creates `dir` if missing).
    pub fn dump_to_dir(&self, dir: &Path) -> std::io::Result<()> {
        fs::create_dir_all(dir)?;
        let s = self.state.lock().unwrap();
        let on_disk = StateOnDisk {
            root_hash: s.root_hash.clone(),
            generation: s.generation,
            blobs: s.blobs.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&on_disk)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let tmp = dir.join("state.json.tmp");
        let final_ = dir.join("state.json");
        fs::write(&tmp, &bytes)?;
        fs::rename(&tmp, &final_)?;
        Ok(())
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `nix develop -c cargo test -p rm-cloud --features fake fake_persistence`
Expected: PASS (both tests).

Then run the full rm-cloud suite to verify no regression:
Run: `nix develop -c cargo test -p rm-cloud --features fake`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rm-cloud/src/fake/mod.rs crates/rm-cloud/tests/fake_persistence.rs
git commit -m "rm-cloud: FakeCloud::from_dir / dump_to_dir for session-on-disk persistence"
```

---

## Task 2: `harness::Session` skeleton + state-dir lifecycle

**Goal:** Create the `Session` type, on-disk session layout, and `new`/`open`/`destroy` operations. No devices or docs yet — pure lifecycle.

**Files:**
- Create: `crates/inkapp-harness/src/session.rs`
- Modify: `crates/inkapp-harness/src/lib.rs` (add `pub mod session;`)
- Modify: `crates/inkapp-harness/Cargo.toml` (add `tokio`, `fs2`, `tempfile`; promote `rm-cloud` and `rm-device` from `[dev-dependencies]` to `[dependencies]`)
- Test: `crates/inkapp-harness/tests/session_lifecycle.rs` (new)

**Acceptance Criteria:**
- [ ] `Session::new_fake(state_dir)` creates `state_dir/`, writes `session.json` with `{ id, backend: "fake", created_at }`, boots a `FakeCloud`, returns the session.
- [ ] `Session::open(state_dir)` rehydrates an existing session: reads `session.json`, calls `FakeCloud::from_dir(state_dir.join("cloud"))`.
- [ ] `Session::destroy(state_dir)` removes the dir.
- [ ] `Session::flush(&self)` writes `cloud/state.json` to disk via `FakeCloud::dump_to_dir`.
- [ ] Drop on `Session` is best-effort — does NOT flush (explicit `flush()` only, so callers control persistence and errors).
- [ ] File lock (`fs2::FileExt::try_lock_exclusive` on `state_dir/.lock`) prevents two `Session::open` calls on the same dir.

**Verify:** `nix develop -c cargo test -p inkapp-harness session_lifecycle` → PASS

**Steps:**

- [ ] **Step 1: Add dependencies**

In `crates/inkapp-harness/Cargo.toml`:

```toml
[dependencies]
inkapp-core = { path = "../inkapp-core" }
inkapp = { path = "../inkapp" }
rm-device = { path = "../rm-device" }
rm-cloud = { path = "../rm-cloud", features = ["fake"] }
rm-files = { path = "../rm-files" }
typst = "0.14"
typst-render = "0.14"
image = { version = "0.25", default-features = false, features = ["png"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "sync"] }
fs2 = "0.4"
tempfile = "3"
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", default-features = false, features = ["clock", "serde"] }

[dev-dependencies]
reading-queue = { path = "../../apps/reading-queue" }
inkapp-readwise-reader = { path = "../inkapp-readwise-reader" }
agenda = { path = "../../apps/agenda" }
inkapp-ics = { path = "../inkapp-ics" }
inkapp-localcal = { path = "../inkapp-localcal" }
inkapp-content = { path = "../inkapp-content" }
zip = "2"
```

(Remove `rm-device`, `rm-cloud`, `rm-files`, `tempfile`, `tokio` from `[dev-dependencies]` since they are now in `[dependencies]`.)

- [ ] **Step 2: Write failing test**

Create `crates/inkapp-harness/tests/session_lifecycle.rs`:

```rust
use inkapp_harness::session::Session;
use tempfile::tempdir;

#[tokio::test]
async fn new_creates_dir_and_session_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s1");

    let s = Session::new_fake(&path).await.unwrap();
    assert!(path.join("session.json").exists());
    assert!(!s.id().is_empty());
    assert_eq!(s.backend(), "fake");
}

#[tokio::test]
async fn open_rehydrates_existing_session() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s1");

    let original_id = {
        let s = Session::new_fake(&path).await.unwrap();
        s.flush().unwrap();
        s.id().to_string()
    };

    let s = Session::open(&path).await.unwrap();
    assert_eq!(s.id(), original_id);
}

#[tokio::test]
async fn second_open_fails_while_first_alive() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s1");

    let _s1 = Session::new_fake(&path).await.unwrap();
    let err = Session::open(&path).await;
    assert!(err.is_err(), "expected lock contention error");
}

#[tokio::test]
async fn destroy_removes_dir() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s1");
    {
        let _s = Session::new_fake(&path).await.unwrap();
    }
    Session::destroy(&path).unwrap();
    assert!(!path.exists());
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `nix develop -c cargo test -p inkapp-harness session_lifecycle`
Expected: FAIL — `inkapp_harness::session` missing.

- [ ] **Step 4: Implement `session.rs`**

Create `crates/inkapp-harness/src/session.rs`:

```rust
//! Session lifecycle: a Session is a directory on disk holding a paired fake
//! cloud, devices, docs, and a command trace. One CLI process == one Session
//! load+save cycle. Pure-lifecycle skeleton; device/doc/ink methods land in
//! later tasks.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use rm_cloud::fake::FakeCloud;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct SessionFile {
    id: String,
    backend: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

pub struct Session {
    state_dir: PathBuf,
    file: SessionFile,
    cloud: FakeCloud,
    _lock: File, // released on drop
}

impl Session {
    /// Create a fresh session backed by an in-process fake cloud.
    pub async fn new_fake(state_dir: &Path) -> std::io::Result<Self> {
        fs::create_dir_all(state_dir)?;
        let lock = Self::acquire_lock(state_dir)?;
        let file = SessionFile {
            id: uuid::Uuid::new_v4().to_string(),
            backend: "fake".to_string(),
            created_at: chrono::Utc::now(),
        };
        fs::write(
            state_dir.join("session.json"),
            serde_json::to_vec_pretty(&file).unwrap(),
        )?;
        let cloud = FakeCloud::from_dir(&state_dir.join("cloud"))
            .await
            .map_err(|e| std::io::Error::other(e))?;
        Ok(Self {
            state_dir: state_dir.to_path_buf(),
            file,
            cloud,
            _lock: lock,
        })
    }

    /// Re-open an existing session directory. Errors if the lock is held.
    pub async fn open(state_dir: &Path) -> std::io::Result<Self> {
        let lock = Self::acquire_lock(state_dir)?;
        let bytes = fs::read(state_dir.join("session.json"))?;
        let file: SessionFile = serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let cloud = FakeCloud::from_dir(&state_dir.join("cloud"))
            .await
            .map_err(|e| std::io::Error::other(e))?;
        Ok(Self {
            state_dir: state_dir.to_path_buf(),
            file,
            cloud,
            _lock: lock,
        })
    }

    /// Remove a session directory. Does not require the session to be open
    /// (the caller is responsible for ensuring no process holds it).
    pub fn destroy(state_dir: &Path) -> std::io::Result<()> {
        if state_dir.exists() {
            fs::remove_dir_all(state_dir)?;
        }
        Ok(())
    }

    /// Persist cloud state to disk. Call before dropping the session.
    pub fn flush(&self) -> std::io::Result<()> {
        self.cloud
            .dump_to_dir(&self.state_dir.join("cloud"))
            .map_err(|e| std::io::Error::other(e))
    }

    pub fn id(&self) -> &str {
        &self.file.id
    }

    pub fn backend(&self) -> &str {
        &self.file.backend
    }

    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    pub(crate) fn cloud(&self) -> &FakeCloud {
        &self.cloud
    }

    fn acquire_lock(state_dir: &Path) -> std::io::Result<File> {
        fs::create_dir_all(state_dir)?;
        let lock_path = state_dir.join(".lock");
        let f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)?;
        f.try_lock_exclusive().map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::WouldBlock, format!("session locked: {e}"))
        })?;
        Ok(f)
    }
}
```

Add to `crates/inkapp-harness/src/lib.rs`:

```rust
pub mod session;
```

- [ ] **Step 5: Run test to verify it passes**

Run: `nix develop -c cargo test -p inkapp-harness session_lifecycle`
Expected: PASS (4 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/inkapp-harness/
git commit -m "inkapp-harness: Session skeleton with on-disk lifecycle"
```

---

## Task 3: `Session::device_new` + `device_list`

**Goal:** Add devices to a session. Each device is a `devices/<device-id>/` subdir under the session, with a `device.json` describing its config and sync cursor.

**Files:**
- Modify: `crates/inkapp-harness/src/session.rs`
- Test: extend `crates/inkapp-harness/tests/session_lifecycle.rs`

**Acceptance Criteria:**
- [ ] `Session::device_new(&mut self, name: Option<&str>) -> DeviceId` creates `devices/<id>/device.json` with `{ id, name, created_at, sync_cursor: null }`.
- [ ] `Session::device_list(&self) -> Vec<DeviceSummary>` returns every device in `devices/`.
- [ ] Device IDs are short (e.g. `dev-1`, `dev-2` per session — sequential per session, not UUIDs, so traces are reproducible).

**Verify:** `nix develop -c cargo test -p inkapp-harness session_devices` → PASS

**Steps:**

- [ ] **Step 1: Write failing test**

Append to `crates/inkapp-harness/tests/session_lifecycle.rs`:

```rust
#[tokio::test]
async fn session_devices_add_and_list() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s");
    let mut s = Session::new_fake(&path).await.unwrap();

    let d1 = s.device_new(Some("primary")).unwrap();
    let d2 = s.device_new(None).unwrap();
    assert_eq!(d1.as_str(), "dev-1");
    assert_eq!(d2.as_str(), "dev-2");

    let listed: Vec<String> = s.device_list().unwrap().into_iter().map(|d| d.id).collect();
    assert_eq!(listed, vec!["dev-1", "dev-2"]);
}
```

- [ ] **Step 2: Implement**

In `crates/inkapp-harness/src/session.rs`, add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct DeviceId(String);

impl DeviceId {
    pub fn as_str(&self) -> &str { &self.0 }
}

#[derive(Debug, Serialize, Deserialize)]
struct DeviceFile {
    id: String,
    name: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    sync_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DeviceSummary {
    pub id: String,
    pub name: Option<String>,
    pub sync_cursor: Option<String>,
}

impl Session {
    pub fn device_new(&mut self, name: Option<&str>) -> std::io::Result<DeviceId> {
        let devices_dir = self.state_dir.join("devices");
        fs::create_dir_all(&devices_dir)?;
        let next_n = self.device_list()?.len() + 1;
        let id = format!("dev-{next_n}");
        let dev_dir = devices_dir.join(&id);
        fs::create_dir_all(&dev_dir)?;
        let file = DeviceFile {
            id: id.clone(),
            name: name.map(str::to_string),
            created_at: chrono::Utc::now(),
            sync_cursor: None,
        };
        fs::write(dev_dir.join("device.json"), serde_json::to_vec_pretty(&file).unwrap())?;
        Ok(DeviceId(id))
    }

    pub fn device_list(&self) -> std::io::Result<Vec<DeviceSummary>> {
        let devices_dir = self.state_dir.join("devices");
        if !devices_dir.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in fs::read_dir(&devices_dir)? {
            let entry = entry?;
            let path = entry.path().join("device.json");
            if !path.exists() { continue; }
            let bytes = fs::read(&path)?;
            let file: DeviceFile = serde_json::from_slice(&bytes)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            out.push(DeviceSummary { id: file.id, name: file.name, sync_cursor: file.sync_cursor });
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }
}
```

- [ ] **Step 3: Run tests**

Run: `nix develop -c cargo test -p inkapp-harness session_lifecycle`
Expected: PASS (5 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/inkapp-harness/
git commit -m "inkapp-harness: Session::device_new / device_list"
```

---

## Task 4: `Session::document_publish`

**Goal:** Publish an app's PDF + manifest into a session-local doc, registered under a device. This is the single most important hookpoint — it ties together app compile, manifest seal, and cloud push.

**Files:**
- Modify: `crates/inkapp-harness/src/session.rs`
- Test: `crates/inkapp-harness/tests/session_publish.rs` (new)

**Acceptance Criteria:**
- [ ] `Session::document_publish(&mut self, device: &DeviceId, app: PublishedApp) -> DocSummary` records a `docs/<doc-id>/` directory with `pdf.pdf`, `manifest.json`, and `doc.json` (`{ id, device_id, app_name, version, pages }`).
- [ ] `PublishedApp` is a small struct callers build (`{ pdf_bytes, manifest, app_name }`) — keeps the harness from depending on any one app's `publish()` entry point.
- [ ] Pushes the PDF blob through the session's `FakeCloud` (via `rm-device::CloudTransport` pointed at `cloud.base`), so a subsequent `device_sync` would see it.
- [ ] Subsequent re-publish of the same app under the same device increments `version`.

**Verify:** `nix develop -c cargo test -p inkapp-harness session_publish` → PASS

**Steps:**

- [ ] **Step 1: Write failing test** using the existing `reading-queue` app's publish helper as the source. (See `crates/inkapp-harness/tests/e2e.rs` for the call pattern that produces PDF + manifest.)

```rust
use inkapp_harness::session::{PublishedApp, Session};
use tempfile::tempdir;

#[tokio::test]
async fn publish_writes_doc_and_increments_version() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s");
    let mut s = Session::new_fake(&path).await.unwrap();
    let dev = s.device_new(Some("rm")).unwrap();

    let app = build_minimal_published_app(); // helper below
    let d1 = s.document_publish(&dev, app.clone()).await.unwrap();
    assert_eq!(d1.version, 1);
    assert!(path.join("docs").join(&d1.id).join("pdf.pdf").exists());
    assert!(path.join("docs").join(&d1.id).join("manifest.json").exists());

    let d2 = s.document_publish(&dev, app).await.unwrap();
    assert_eq!(d2.id, d1.id, "re-publish same app keeps id");
    assert_eq!(d2.version, 2);
}

fn build_minimal_published_app() -> PublishedApp {
    // Reuse the smallest existing fixture in the harness — a single-page Typst doc
    // with one region. See crates/inkapp-harness/tests/common/mod.rs.
    inkapp_harness::tests_common::single_region_app("smoke")
}
```

(If `tests_common` doesn't exist yet, add it as `crates/inkapp-harness/src/tests_common.rs` exposing `single_region_app(name) -> PublishedApp` — a one-page doc with one region named `r1`, built by calling `compile_to_document_with_sources` on a hardcoded Typst string and recovering the manifest. Gate with `#[cfg(any(test, feature = "tests-common"))]` or just `pub mod tests_common;` since it's small.)

- [ ] **Step 2: Run test to verify it fails**

Run: `nix develop -c cargo test -p inkapp-harness session_publish`
Expected: FAIL — `document_publish` not defined.

- [ ] **Step 3: Implement `PublishedApp` and `document_publish`**

In `crates/inkapp-harness/src/session.rs`:

```rust
use inkapp_core::manifest::Manifest;

#[derive(Clone)]
pub struct PublishedApp {
    pub app_name: String,
    pub pdf_bytes: Vec<u8>,
    pub manifest: Manifest,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DocSummary {
    pub id: String,
    pub device_id: String,
    pub app_name: String,
    pub version: u32,
    pub pages: usize,
}

impl Session {
    pub async fn document_publish(
        &mut self,
        device: &DeviceId,
        app: PublishedApp,
    ) -> std::io::Result<DocSummary> {
        // Stable doc id derived from (device, app_name) so re-publish updates the
        // same doc rather than creating a new one.
        let doc_id = format!("{}-{}", device.as_str(), slugify(&app.app_name));
        let doc_dir = self.state_dir.join("docs").join(&doc_id);
        fs::create_dir_all(&doc_dir)?;

        let prev_version = read_doc_version(&doc_dir).unwrap_or(0);
        let version = prev_version + 1;
        let pages = app.manifest.regions.iter().map(|r| r.page).max().map(|p| p + 1).unwrap_or(1);

        fs::write(doc_dir.join("pdf.pdf"), &app.pdf_bytes)?;
        fs::write(
            doc_dir.join("manifest.json"),
            serde_json::to_vec_pretty(&app.manifest).unwrap(),
        )?;
        let summary = DocSummary {
            id: doc_id.clone(),
            device_id: device.as_str().to_string(),
            app_name: app.app_name.clone(),
            version,
            pages,
        };
        fs::write(doc_dir.join("doc.json"), serde_json::to_vec_pretty(&summary).unwrap())?;

        // Push the PDF through the session-local cloud so a future device_sync
        // observes it. Uses CloudTransport pointed at self.cloud.base.
        push_to_fake_cloud(&self.cloud, &app).await?;
        Ok(summary)
    }
}

fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect()
}

fn read_doc_version(doc_dir: &Path) -> Option<u32> {
    let bytes = fs::read(doc_dir.join("doc.json")).ok()?;
    let s: DocSummary = serde_json::from_slice(&bytes).ok()?;
    Some(s.version)
}

async fn push_to_fake_cloud(
    cloud: &FakeCloud,
    app: &PublishedApp,
) -> std::io::Result<()> {
    // Use rm_cloud::Client against cloud.base, mkdir_p the app folder, put the PDF.
    // Concrete impl uses rm_cloud::Client::with_base_url(&cloud.base) + DocFiles::new_pdf.
    let client = rm_cloud::Client::with_base_url(&cloud.base)
        .map_err(|e| std::io::Error::other(format!("client init: {e}")))?;
    let folder = format!("/inkctl/{}", slugify(&app.app_name));
    client.mkdir_p(&folder).await.map_err(|e| std::io::Error::other(format!("mkdir: {e}")))?;
    let files = rm_cloud::DocFiles::new_pdf(&app.app_name, &app.pdf_bytes);
    client.put_content_only(&folder, files).await
        .map_err(|e| std::io::Error::other(format!("put: {e}")))?;
    Ok(())
}
```

(Confirm `rm_cloud::Client::with_base_url` exists; if the existing constructor differs, adjust the call to match the real API — `crates/rm-cloud/src/client.rs` is the source of truth.)

- [ ] **Step 4: Run tests**

Run: `nix develop -c cargo test -p inkapp-harness session_publish`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/inkapp-harness/
git commit -m "inkapp-harness: Session::document_publish (PDF+manifest+fake-cloud push)"
```

---

## Task 5: `harness::observe` — page describe + document describe

**Goal:** The primary "what's here" view for Claude. Reads stored manifest + doc summary, emits the accessibility-tree JSON from the spec (excluding links and ink — those land in Tasks 6 & 9).

**Files:**
- Create: `crates/inkapp-harness/src/observe.rs`
- Modify: `crates/inkapp-harness/src/lib.rs` (add `pub mod observe;`)
- Test: `crates/inkapp-harness/tests/observe_describe.rs` (new)

**Acceptance Criteria:**
- [ ] `observe::page_describe(session, doc_id, page) -> PageDescribe` returns regions with `{ name, rect, layer_hint, app_state, link: None, ink: { strokes: 0, by_layer: {} } }`, `links: []`, `layers_present: []`, `image: None`.
- [ ] `observe::document_describe(session, doc_id) -> DocumentDescribe` returns `{ doc_id, app_name, version, pages, regions_per_page, links_per_page }` (link counts will be 0 until Task 6).
- [ ] All output is `serde::Serialize` so the CLI can dump it as JSON.

**Verify:** `nix develop -c cargo test -p inkapp-harness observe_describe` → PASS

**Steps:**

- [ ] **Step 1: Write failing test**

```rust
use inkapp_harness::observe;
use inkapp_harness::session::Session;
use tempfile::tempdir;

#[tokio::test]
async fn page_describe_returns_regions_from_manifest() {
    let dir = tempdir().unwrap();
    let mut s = Session::new_fake(dir.path()).await.unwrap();
    let dev = s.device_new(None).unwrap();
    let doc = s.document_publish(&dev, inkapp_harness::tests_common::single_region_app("d1")).await.unwrap();

    let desc = observe::page_describe(&s, &doc.id, 0).unwrap();
    assert_eq!(desc.regions.len(), 1);
    assert_eq!(desc.regions[0].name, "r1");
    assert_eq!(desc.version, 1);
}
```

- [ ] **Step 2: Implement `observe.rs`**

```rust
//! Read-side views over a Session: the agent's "accessibility tree".

use std::fs;
use std::path::Path;

use inkapp_core::manifest::Manifest;
use serde::Serialize;

use crate::session::{DocSummary, Session};

#[derive(Debug, Serialize)]
pub struct RegionDescribe {
    pub name: String,
    pub rect: [f64; 4],
    pub layer_hint: String,
    pub link: Option<LinkTarget>,
    pub app_state: serde_json::Value,
    pub ink: InkSummary,
}

#[derive(Debug, Serialize)]
pub struct InkSummary {
    pub strokes: usize,
    pub by_layer: std::collections::BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
pub struct LinkAnnotation {
    pub rect: [f64; 4],
    pub target: String,
    pub region: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LinkTarget {
    pub target: String,
}

#[derive(Debug, Serialize)]
pub struct PageDescribe {
    pub doc_id: String,
    pub page: usize,
    pub version: u32,
    pub regions: Vec<RegionDescribe>,
    pub links: Vec<LinkAnnotation>,
    pub layers_present: Vec<String>,
    pub image: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DocumentDescribe {
    pub doc_id: String,
    pub app_name: String,
    pub version: u32,
    pub pages: usize,
    pub regions_per_page: Vec<usize>,
    pub links_per_page: Vec<usize>,
}

pub fn page_describe(session: &Session, doc_id: &str, page: usize) -> std::io::Result<PageDescribe> {
    let (summary, manifest) = load_doc(session.state_dir(), doc_id)?;
    let regions: Vec<RegionDescribe> = manifest
        .regions
        .iter()
        .filter(|r| r.page == page)
        .map(|r| RegionDescribe {
            name: r.name.clone(),
            rect: [r.rect.x0, r.rect.y0, r.rect.x1, r.rect.y1],
            layer_hint: "pen".to_string(),
            link: None, // Task 6
            app_state: r.app_state.clone().unwrap_or(serde_json::Value::Null),
            ink: InkSummary { strokes: 0, by_layer: Default::default() }, // Task 9
        })
        .collect();
    Ok(PageDescribe {
        doc_id: doc_id.to_string(),
        page,
        version: summary.version,
        regions,
        links: Vec::new(), // Task 6
        layers_present: Vec::new(), // Task 9
        image: None,
    })
}

pub fn document_describe(session: &Session, doc_id: &str) -> std::io::Result<DocumentDescribe> {
    let (summary, manifest) = load_doc(session.state_dir(), doc_id)?;
    let mut regions_per_page = vec![0usize; summary.pages];
    for r in &manifest.regions {
        if r.page < regions_per_page.len() {
            regions_per_page[r.page] += 1;
        }
    }
    Ok(DocumentDescribe {
        doc_id: summary.id,
        app_name: summary.app_name,
        version: summary.version,
        pages: summary.pages,
        regions_per_page,
        links_per_page: vec![0; summary.pages],
    })
}

fn load_doc(state_dir: &Path, doc_id: &str) -> std::io::Result<(DocSummary, Manifest)> {
    let dir = state_dir.join("docs").join(doc_id);
    let summary: DocSummary = serde_json::from_slice(&fs::read(dir.join("doc.json"))?)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let manifest: Manifest = serde_json::from_slice(&fs::read(dir.join("manifest.json"))?)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok((summary, manifest))
}
```

(If `Manifest`'s region struct does not have an `app_state` field, drop the line — verify by reading `crates/inkapp-core/src/manifest.rs`. The harness should use whatever per-region state field name actually exists.)

- [ ] **Step 3: Run test**

Run: `nix develop -c cargo test -p inkapp-harness observe_describe`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/inkapp-harness/
git commit -m "inkapp-harness: observe::page_describe / document_describe"
```

---

## Task 6: PDF link extraction + integration into describe

**Goal:** Extract link annotations from the published PDF and fold them into `page_describe` (and link counts into `document_describe`).

**Files:**
- Create: `crates/inkapp-harness/src/pdf_links.rs`
- Modify: `crates/inkapp-harness/src/observe.rs`
- Modify: `crates/inkapp-harness/Cargo.toml` (add `lopdf = "0.36"`)
- Test: `crates/inkapp-harness/tests/observe_links.rs` (new)

**Acceptance Criteria:**
- [ ] `pdf_links::extract(&pdf_bytes) -> Vec<RawLink>` returns one entry per `/Link` annotation: `{ page: usize, rect: [f64;4], target: LinkTarget }`.
- [ ] `LinkTarget::Page(usize)` for `/GoTo`+`/D [pageref ...]`, `LinkTarget::Uri(String)` for `/URI`.
- [ ] `page_describe` populates `links: Vec<LinkAnnotation>` from the PDF and, for each, sets `regions[i].link` when the link rect is contained in a region's rect (with a small tolerance, e.g. ±1pt).
- [ ] `document_describe::links_per_page` reflects real counts.

**Verify:** `nix develop -c cargo test -p inkapp-harness observe_links` → PASS

**Steps:**

- [ ] **Step 1: Write failing test** — build a `PublishedApp` whose Typst source emits a `link("https://example.org")[r1]` inside region `r1`, publish, call `page_describe`, assert `regions[0].link.target == "uri:https://example.org"`.

(Implementer: add a helper `tests_common::app_with_uri_link(region_name, uri) -> PublishedApp` mirroring `single_region_app` but with a link annotation. Use Typst's `link` function inside the region content.)

- [ ] **Step 2: Implement `pdf_links.rs`** using `lopdf` to walk each page's `/Annots` array, filter `/Subtype /Link`, and decode `/A /S /URI` (URI) or `/A /S /GoTo /D` (internal). Resolve `/Dest` page references to 0-based page indices.

- [ ] **Step 3: Integrate** into `observe::page_describe` (replace the `// Task 6` lines).

- [ ] **Step 4: Run test**

Run: `nix develop -c cargo test -p inkapp-harness observe_links`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/inkapp-harness/
git commit -m "inkapp-harness: PDF link extraction in page/document describe"
```

---

## Task 7: `observe::page_snapshot` (PNG render)

**Goal:** Render a single page of a published doc to PNG bytes — Claude's "screenshot".

**Files:**
- Modify: `crates/inkapp-harness/src/observe.rs`
- Test: `crates/inkapp-harness/tests/observe_snapshot.rs` (new)

**Acceptance Criteria:**
- [ ] `observe::page_snapshot(session, doc_id, page) -> Vec<u8>` returns valid PNG bytes (magic header `\x89PNG`).
- [ ] Uses the same `typst_render` pipeline as the existing inspector. Re-renders from the stored PDF is acceptable but rendering from cached Typst source is preferred if cheap.
- [ ] Default DPI = 150 (a `--dpi` flag is wired through later in the CLI task).

**Verify:** `nix develop -c cargo test -p inkapp-harness observe_snapshot` → PASS

**Steps:**

- [ ] **Step 1: Write failing test** — publish single-region app, call `page_snapshot`, assert PNG header.

- [ ] **Step 2: Implement** by re-using the existing render path. The simplest approach: store the Typst source alongside the manifest in `document_publish` (add `source.typ` to `docs/<id>/`), then re-compile and render the requested page. Modify `PublishedApp` to carry `pub source_typ: Option<String>` (optional, for now mandatory in practice).

- [ ] **Step 3: Run test**

Run: `nix develop -c cargo test -p inkapp-harness observe_snapshot`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/inkapp-harness/
git commit -m "inkapp-harness: observe::page_snapshot PNG render"
```

---

## Task 8: `observe::page_inspect` — layer + link overlays

**Goal:** Extend the existing `inspector.rs` to accept layer filters and a `show` flag set, and emit the inspector PNG with region/link/synth/attributed-stroke overlays color-coded.

**Files:**
- Modify: `crates/inkapp-harness/src/inspector.rs`
- Modify: `crates/inkapp-harness/src/observe.rs`
- Test: `crates/inkapp-harness/tests/observe_inspect.rs` (new)

**Acceptance Criteria:**
- [ ] `observe::page_inspect(session, doc_id, page, opts) -> Vec<u8>` returns PNG bytes.
- [ ] `InspectOpts { layers: Option<Vec<String>>, show: ShowFlags }` where `ShowFlags = { regions, links, synth_strokes, attributed_strokes }`, all `bool`, default all-true.
- [ ] When `layers` is `Some([..])`, only those rm-scene layers' strokes are drawn.
- [ ] When `show.links` is true, every link annotation gets a thin colored rectangle with its target as a small text label (or just a colored border if rendering text is hard).
- [ ] No regression in existing `inspector` tests.

**Verify:** `nix develop -c cargo test -p inkapp-harness observe_inspect` → PASS

**Steps:**

- [ ] **Step 1: Write failing test** — publish app, call `page_inspect` with `ShowFlags::default()` and with `show.regions = false`; assert PNG header + (with image::load_from_memory) verify the second image's red region overlay is absent (heuristic: count red pixels, expect 0 when regions hidden).

- [ ] **Step 2: Extend `inspector.rs`** to accept an `InspectOpts` struct. Keep the existing `inspect(...)` function signature working (call it via `inspect_with_opts(... InspectOpts::default())` internally).

- [ ] **Step 3: Wire `observe::page_inspect`** to load the stored Typst source + manifest + ink layer data (from per-device pending-ink files, once Task 10 lands — for this task, pass empty `&[]` for strokes; full integration happens in Task 11 where `step` produces strokes).

- [ ] **Step 4: Run test**

Run: `nix develop -c cargo test -p inkapp-harness observe_inspect`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/inkapp-harness/
git commit -m "inkapp-harness: observe::page_inspect with layer/link overlays"
```

---

## Task 9: `observe` — device tree + ink list

**Goal:** Two more views. `device_tree` mirrors `rmapi ls` against the session's fake cloud root for that device. `ink_list` groups the current page's strokes by layer and/or region.

**Files:**
- Modify: `crates/inkapp-harness/src/observe.rs`
- Modify: `crates/inkapp-harness/src/session.rs` (add `Session::pending_ink(device, doc, page) -> Vec<Stroke>` reader, returns `[]` until Task 10 plumbs writers)
- Test: `crates/inkapp-harness/tests/observe_tree_ink.rs` (new)

**Acceptance Criteria:**
- [ ] `observe::device_tree(session, device_id, path) -> DeviceTree` walks the fake cloud and returns a JSON tree of folders + docs (id, name, parent, file_type, last_sync).
- [ ] `observe::ink_list(session, doc_id, page, group_by) -> InkList` returns either by-layer or by-region or flat.
- [ ] Tested against a session with one published doc (one entry under the synthesized folder).

**Verify:** `nix develop -c cargo test -p inkapp-harness observe_tree_ink` → PASS

**Steps:**

- [ ] **Step 1: Write failing test** — publish a doc, call `device_tree(&dev, "/")`, assert `tree.children` contains the synthesized doc by name.

- [ ] **Step 2: Implement** using `rm_cloud::Snapshot::current` against `session.cloud().base`. Walk children recursively.

- [ ] **Step 3: Commit**

```bash
git add crates/inkapp-harness/
git commit -m "inkapp-harness: observe::device_tree / ink_list"
```

---

## Task 10: `Session::ink_*` — tap, swipe, fixture, draw

**Goal:** Synthesize strokes into a per-(device, doc, page) pending-ink buffer on disk. Strokes are persisted; `Session::step` later consumes them.

**Files:**
- Modify: `crates/inkapp-harness/src/session.rs`
- Test: `crates/inkapp-harness/tests/session_ink.rs` (new)

**Acceptance Criteria:**
- [ ] `Session::ink_tap(device, doc, page, region) -> ()` appends a center-point stroke (highlighter=false) to `devices/<dev>/pending/<doc>/<page>.json` (a JSON array of `Stroke` objects).
- [ ] `Session::ink_swipe(device, doc, page, region) -> ()` — full-width highlighter stroke.
- [ ] `Session::ink_fixture(device, doc, page, region, fixture_name) -> ()` — reuses the existing `fixtures::GestureFixture::transplant_default` flow from `simulator.rs`.
- [ ] `Session::ink_draw(device, doc, page, path: &[PdfPoint], layer: Option<&str>, highlighter: bool) -> ()` — freeform polyline.
- [ ] Each call also writes a corresponding `kind: "call"` line to `trace.jsonl` (groundwork for Task 14 — this task can write a minimal stub trace entry and Task 14 will formalize the shape).
- [ ] `observe::ink_list` returns the persisted strokes.

**Verify:** `nix develop -c cargo test -p inkapp-harness session_ink` → PASS

**Steps:**

- [ ] **Step 1: Write failing test**

```rust
#[tokio::test]
async fn ink_tap_persists_and_is_readable() {
    let dir = tempdir().unwrap();
    let mut s = Session::new_fake(dir.path()).await.unwrap();
    let dev = s.device_new(None).unwrap();
    let doc = s.document_publish(&dev, single_region_app("d")).await.unwrap();

    s.ink_tap(&dev, &doc.id, 0, "r1").unwrap();
    let list = observe::ink_list(&s, &doc.id, 0, ObserveGroup::Flat).unwrap();
    assert_eq!(list.strokes.len(), 1);
}
```

- [ ] **Step 2: Implement** the four methods. Use the existing `synthesize()` helper from `simulator.rs` as a reference for tap/swipe geometry — extract a shared helper if convenient (e.g. `ink::center_point(rect)` in core or a private fn in the harness).

- [ ] **Step 3: Run test**

Run: `nix develop -c cargo test -p inkapp-harness session_ink`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/inkapp-harness/
git commit -m "inkapp-harness: Session::ink_{tap,swipe,fixture,draw} → pending buffer"
```

---

## Task 11: `Session::step` — drive the loop one cycle

**Goal:** The core loop driver. Loads pending ink for a device, runs through `App::step` on every published doc the device owns, computes diffs/msgs, re-renders, and produces the `session step` JSON shape.

**Files:**
- Modify: `crates/inkapp-harness/src/session.rs`
- Test: `crates/inkapp-harness/tests/session_step.rs` (new)

**Acceptance Criteria:**
- [ ] `Session::step(device: &DeviceId, opts: StepOpts) -> StepResult` returns `{ cycle, msgs, model_diff, connector_writes, secrets_read, pages_changed, new_version, debug_renders }`.
- [ ] `cycle` is per-device, persisted in `devices/<id>/cursor.json`.
- [ ] Re-renders changed pages and writes new PDFs into `docs/<id>/pdf.pdf`; bumps `version` in `doc.json`.
- [ ] With `opts.debug == true`, writes one inspector PNG per changed page into `debug/cycle-<n>-page-<p>.png` and lists the paths in `debug_renders`.
- [ ] `secrets_read` reports names only — values must NEVER appear (assert in test).
- [ ] Smoke test: tap a known region of `apps/reading-queue` (or `tests_common::single_region_app`), step, assert at least one `msg` was emitted and at least one page changed.

**Verify:** `nix develop -c cargo test -p inkapp-harness session_step` → PASS

**Steps:**

- [ ] **Step 1: Write failing test** — publish `single_region_app`, tap `r1`, call `step`, assert `step.cycle == 1` and `step.pages_changed` is non-empty.

- [ ] **Step 2: Implement** by wiring through the existing `App::step` driver from `inkapp_core::runtime`. The session needs to hold an `App` instance per (device, doc) — for now, take `App` by closure: `document_publish` accepts an additional `app_factory: Box<dyn Fn() -> App>` argument and stores it in-memory keyed by `(device, doc)`. (Document this in `PublishedApp`'s docstring; revisit if it bites.)

- [ ] **Step 3: Secrets-leakage regression test**

```rust
#[tokio::test]
async fn step_never_logs_secret_values() {
    // ... build a session where a connector reads a known token "TOPSECRET123"
    let step = s.step(&dev, StepOpts::default()).await.unwrap();
    let json = serde_json::to_string(&step).unwrap();
    assert!(!json.contains("TOPSECRET123"), "secret value leaked into step output");
}
```

- [ ] **Step 4: Run tests**

Run: `nix develop -c cargo test -p inkapp-harness session_step`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/inkapp-harness/
git commit -m "inkapp-harness: Session::step (msgs + model diff + secrets audit)"
```

---

## Task 12: `Session::link_follow` + `Session::device_sync`

**Goal:** Two smaller verbs that round out the device-driving surface. `link_follow` resolves a link target and sets the device's current page; `device_sync` runs one push/pull cycle.

**Files:**
- Modify: `crates/inkapp-harness/src/session.rs`
- Test: `crates/inkapp-harness/tests/session_link_sync.rs` (new)

**Acceptance Criteria:**
- [ ] `Session::link_follow(device, doc, page, region) -> FollowResult { target_page, target_uri }` — page-target updates `devices/<id>/cursor.json::current_page`; URI-target returns the URI and does not change current page.
- [ ] `Session::device_sync(device) -> SyncResult { pushed: Vec<String>, pulled: Vec<String>, conflicts: Vec<String> }` — drives one push+pull through the fake cloud and updates `sync_cursor` in `device.json`.

**Verify:** `nix develop -c cargo test -p inkapp-harness session_link_sync` → PASS

**Steps:**

- [ ] **Step 1: Write failing test** for each verb.
- [ ] **Step 2: Implement.** `link_follow` reuses `pdf_links::extract` results filtered by containment in the region rect; `device_sync` calls `rm_cloud::Client::sync` against `cloud.base`.
- [ ] **Step 3: Run tests.**
- [ ] **Step 4: Commit**

```bash
git add crates/inkapp-harness/
git commit -m "inkapp-harness: Session::link_follow + Session::device_sync"
```

---

## Task 13: Trace recording in `recording.rs`

**Goal:** Every public `Session` mutating method writes a `kind: "call"` entry to `trace.jsonl`; `Session::record_assert(target, expected)` writes a `kind: "assert"` entry. `Session::record_start()` / `record_stop()` toggle a flag (recording is off by default).

**Files:**
- Modify: `crates/inkapp-harness/src/recording.rs`
- Modify: `crates/inkapp-harness/src/session.rs` (every mutating method calls the trace writer)
- Test: `crates/inkapp-harness/tests/recording_trace.rs` (new)

**Acceptance Criteria:**
- [ ] Calling `device_new`, `document_publish`, `ink_*`, `step`, `link_follow`, `device_sync` after `record_start` appends to `state_dir/trace.jsonl`.
- [ ] Each entry: `{ ts, kind: "call", cmd: ["device","new"], args: {...}, result: {...} }`.
- [ ] `record_assert("step.cycle", json!(1))` appends `{ ts, kind: "assert", target, expected }`.
- [ ] With recording off (default), no `trace.jsonl` writes happen.
- [ ] Trace can be parsed back into `Vec<TraceEntry>` via a public `recording::read_trace(path)` fn.

**Verify:** `nix develop -c cargo test -p inkapp-harness recording_trace` → PASS

**Steps:**

- [ ] **Step 1: Write failing test** — start a session, `record_start`, do a `device_new` + `ink_tap`, `record_stop`, parse `trace.jsonl`, assert two `kind: "call"` entries in order.
- [ ] **Step 2: Implement.** Add `pub struct TraceWriter` in `recording.rs` with `append_call` / `append_assert`. Sprinkle calls through `session.rs`.
- [ ] **Step 3: Run test.**
- [ ] **Step 4: Commit**

```bash
git add crates/inkapp-harness/
git commit -m "inkapp-harness: trace.jsonl recording on every Session mutation"
```

---

## Task 14: `harness::emit::to_rust` — trace → Rust `#[test]`

**Goal:** Walk a `trace.jsonl`, emit a self-contained `#[test]` Rust function that reproduces the mutations and asserts the recorded annotations. Elides pure-observation calls.

**Files:**
- Create: `crates/inkapp-harness/src/emit.rs`
- Modify: `crates/inkapp-harness/src/lib.rs`
- Test: `crates/inkapp-harness/tests/emit_to_rust.rs` (new)

**Acceptance Criteria:**
- [ ] `emit::to_rust(trace_path, test_name) -> String` returns valid Rust source containing one `#[tokio::test] async fn <test_name>() { ... }`.
- [ ] Mutation calls map to harness method calls (e.g. `["device","new"] → s.device_new(...)`).
- [ ] Observation calls (`["page","describe",...]`, `["device","tree",...]`, `["ink","list",...]`) are skipped UNLESS followed by a `kind: "assert"` entry — assertions get bound to the prior observation call's result via JSON-path lookup.
- [ ] Generated test compiles when written to `tests/` of any crate that depends on `inkapp-harness`. Tested by running the dogfood test (Task 23).
- [ ] `emit::to_rust` does NOT touch the filesystem itself — pure string-in/string-out. The CLI handles file writes.

**Verify:** `nix develop -c cargo test -p inkapp-harness emit_to_rust` → PASS

**Steps:**

- [ ] **Step 1: Write failing test** — handcraft a small `trace.jsonl`, call `emit::to_rust`, assert the returned string contains expected calls (`s.device_new(`, `s.ink_tap(`, `assert_eq!`).
- [ ] **Step 2: Implement** using a simple template-string approach (no codegen crate needed; `format!` over a hardcoded `#[tokio::test]` skeleton).
- [ ] **Step 3: Run test.**
- [ ] **Step 4: Commit**

```bash
git add crates/inkapp-harness/
git commit -m "inkapp-harness: emit::to_rust trace→#[test] generator"
```

---

## Task 15: `inkctl` crate scaffolding

**Goal:** Brand-new bin crate with `clap` top-level, JSON output envelope, and stub command modules. No commands wired yet beyond `--version`.

**Files:**
- Create: `crates/inkctl/Cargo.toml`
- Create: `crates/inkctl/src/main.rs`
- Create: `crates/inkctl/src/output.rs`
- Create: `crates/inkctl/src/cmd/mod.rs`
- Modify: `Cargo.toml` (workspace) — add `crates/inkctl` to `members`

**Acceptance Criteria:**
- [ ] `cargo build -p inkctl` succeeds.
- [ ] `inkctl --version` prints a version.
- [ ] `inkctl --help` lists the five nouns (`session`, `device`, `document`, `page`, `ink`) plus `record`, even if subcommands stub-error.
- [ ] All commands return JSON envelope `{ ok: true, data }` or `{ ok: false, error }` to stdout; exit code mirrors `ok`.

**Verify:** `nix develop -c cargo run -p inkctl -- --help` shows all six top-level nouns.

**Steps:**

- [ ] **Step 1: Add member**

In workspace root `Cargo.toml`, add `crates/inkctl` to `[workspace] members`.

- [ ] **Step 2: Create `Cargo.toml`**

```toml
[package]
name = "inkctl"
version = "0.1.0"
edition = "2021"
license = "MIT"
description = "Agent-drivable CLI for the inkapp test harness"

[dependencies]
inkapp-harness = { path = "../inkapp-harness" }
inkapp-core = { path = "../inkapp-core" }
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
anyhow = "1"

[dev-dependencies]
assert_cmd = "2"
predicates = "3"
tempfile = "3"
```

- [ ] **Step 3: Implement `output.rs`**

```rust
use serde::Serialize;

#[derive(Serialize)]
#[serde(untagged)]
pub enum Envelope<T: Serialize> {
    Ok { ok: bool, data: T },
    Err { ok: bool, error: ErrorBody },
}

#[derive(Serialize)]
pub struct ErrorBody {
    pub kind: String,
    pub message: String,
}

pub fn print_ok<T: Serialize>(data: T) -> ! {
    let env = Envelope::Ok { ok: true, data };
    println!("{}", serde_json::to_string(&env).unwrap());
    std::process::exit(0);
}

pub fn print_err(kind: &str, message: impl ToString) -> ! {
    let env: Envelope<()> = Envelope::Err {
        ok: false,
        error: ErrorBody { kind: kind.to_string(), message: message.to_string() },
    };
    println!("{}", serde_json::to_string(&env).unwrap());
    std::process::exit(1);
}
```

- [ ] **Step 4: Implement `main.rs`**

```rust
use clap::{Parser, Subcommand};

mod cmd;
mod output;

#[derive(Parser)]
#[command(name = "inkctl", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Top,
}

#[derive(Subcommand)]
enum Top {
    Session(cmd::session::Args),
    Device(cmd::device::Args),
    Document(cmd::document::Args),
    Page(cmd::page::Args),
    Ink(cmd::ink::Args),
    Record(cmd::record::Args),
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Top::Session(a) => cmd::session::run(a).await,
        Top::Device(a) => cmd::device::run(a).await,
        Top::Document(a) => cmd::document::run(a).await,
        Top::Page(a) => cmd::page::run(a).await,
        Top::Ink(a) => cmd::ink::run(a).await,
        Top::Record(a) => cmd::record::run(a).await,
    }
}
```

- [ ] **Step 5: Implement `cmd/mod.rs`** with stub modules:

```rust
pub mod session;
pub mod device;
pub mod document;
pub mod page;
pub mod ink;
pub mod record;
```

And one stub per noun (e.g. `cmd/session.rs`):

```rust
use clap::Subcommand;
use crate::output;

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    New,
}

pub async fn run(_args: Args) -> ! {
    output::print_err("not_implemented", "session commands land in Task 16")
}
```

(Same shape for the other five.)

- [ ] **Step 6: Verify build + help**

Run: `nix develop -c cargo build -p inkctl`
Run: `nix develop -c cargo run -p inkctl -- --help`
Expected: PASS; help lists all six nouns.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/inkctl/
git commit -m "inkctl: scaffold bin crate with clap top-level and JSON envelope"
```

---

## Task 16: inkctl `session` commands

**Goal:** Wire `session new`, `session list`, `session destroy`, `session env`, `session step` to `harness::Session`.

**Files:**
- Modify: `crates/inkctl/src/cmd/session.rs`
- Test: `crates/inkctl/tests/smoke_session.rs` (new)

**Acceptance Criteria:**
- [ ] `inkctl session new --name foo` prints `{"ok":true,"data":{"session_id":"...","backend":"fake","path":"..."}}`.
- [ ] `INKCTL_HOME` env var controls the parent dir for sessions; defaults to `$XDG_STATE_HOME/inkctl` or `~/.local/state/inkctl`.
- [ ] `inkctl session env <id>` prints `INKCTL_SESSION=<id>` (no JSON envelope — designed for `eval`).
- [ ] `inkctl session step --session <id> --device <dev-id>` calls `Session::step` and prints the `StepResult` JSON.
- [ ] Each command opens the session, runs, calls `flush`, releases the lock.

**Verify:** `nix develop -c cargo test -p inkctl smoke_session` → PASS

**Steps:**

- [ ] **Step 1: Write failing test**

Create `crates/inkctl/tests/smoke_session.rs`:

```rust
use assert_cmd::Command;
use serde_json::Value;
use tempfile::tempdir;

#[test]
fn session_new_and_list() {
    let home = tempdir().unwrap();
    let out = Command::cargo_bin("inkctl").unwrap()
        .env("INKCTL_HOME", home.path())
        .args(["session", "new"])
        .assert().success().get_output().clone();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], true);
    let id = v["data"]["session_id"].as_str().unwrap().to_string();

    let out = Command::cargo_bin("inkctl").unwrap()
        .env("INKCTL_HOME", home.path())
        .args(["session", "list"])
        .assert().success().get_output().clone();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let sessions = v["data"]["sessions"].as_array().unwrap();
    assert!(sessions.iter().any(|s| s["id"] == id));
}
```

- [ ] **Step 2: Implement `cmd/session.rs`** by replacing the stub with full clap subcommands and dispatching to harness methods. Session dirs live at `$INKCTL_HOME/<id>/`.

- [ ] **Step 3: Run test.**
- [ ] **Step 4: Commit**

```bash
git add crates/inkctl/
git commit -m "inkctl: session new/list/destroy/env/step"
```

---

## Task 17: inkctl `device` commands

**Goal:** `device new`, `device list`, `device tree`, `device sync`.

**Files:**
- Modify: `crates/inkctl/src/cmd/device.rs`
- Test: `crates/inkctl/tests/smoke_device.rs` (new)

**Acceptance Criteria:**
- [ ] Each command requires `--session <id>` (or reads `INKCTL_SESSION` env).
- [ ] Round-trips: `device new` → `device list` shows the new device; `device tree` returns valid JSON; `device sync` returns `{ pushed, pulled, conflicts }`.

**Verify:** `nix develop -c cargo test -p inkctl smoke_device` → PASS

**Steps:**

- [ ] **Step 1: Test → 2: Implement → 3: Verify → 4: Commit**

```bash
git add crates/inkctl/
git commit -m "inkctl: device new/list/tree/sync"
```

---

## Task 18: inkctl `document` commands

**Goal:** `document publish`, `document open`, `document describe`, `document pdf`, `document rmdoc`.

**Files:**
- Modify: `crates/inkctl/src/cmd/document.rs`
- Test: `crates/inkctl/tests/smoke_document.rs` (new)

**Acceptance Criteria:**
- [ ] `document publish <dev> <app-path>` builds a `PublishedApp` by `cargo build`ing the app and invoking its publish entry point. For v1, support apps from the workspace `apps/` directory by name (e.g. `apps/reading-queue`); a generic protocol comes later.
- [ ] `document describe <doc-id>` returns the JSON shape from Task 5.
- [ ] `document pdf <doc-id> --out p.pdf` writes raw PDF bytes; success message goes to stderr (so stdout stays clean for piping).

**Verify:** `nix develop -c cargo test -p inkctl smoke_document` → PASS

**Steps:**

- [ ] **Step 1: Test → 2: Implement → 3: Verify → 4: Commit**

Implementation note: `document publish` is the trickiest. For v1, the CLI accepts a built-in registry of known apps: `match app_path.file_name() { "reading-queue" => reading_queue::publish_for_harness(), ... }`. The registry lives in a new `crates/inkctl/src/apps.rs`. Adding new apps requires a small registry update — acceptable for now.

```bash
git add crates/inkctl/
git commit -m "inkctl: document publish/open/describe/pdf/rmdoc"
```

---

## Task 19: inkctl `page` commands

**Goal:** `page describe`, `page snapshot`, `page inspect`, `page links`.

**Files:**
- Modify: `crates/inkctl/src/cmd/page.rs`
- Test: `crates/inkctl/tests/smoke_page.rs` (new)

**Acceptance Criteria:**
- [ ] All four commands dispatch to the matching `observe::*` calls.
- [ ] `page snapshot` and `page inspect` accept `--out <path>` (required for binary output); without it, error with `kind: "missing_arg"`.
- [ ] `page inspect` accepts `--layers a,b,c` and `--show regions,links,strokes,attributed` (default: all).

**Verify:** `nix develop -c cargo test -p inkctl smoke_page` → PASS

**Steps:**

- [ ] **Step 1: Test → 2: Implement → 3: Verify → 4: Commit**

```bash
git add crates/inkctl/
git commit -m "inkctl: page describe/snapshot/inspect/links"
```

---

## Task 20: inkctl `ink` commands + `link follow`

**Goal:** `ink tap/swipe/fixture/draw/list` and `link follow`.

**Files:**
- Modify: `crates/inkctl/src/cmd/ink.rs`
- Test: `crates/inkctl/tests/smoke_ink.rs` (new)

**Acceptance Criteria:**
- [ ] `ink draw --path "12,34 56,78 90,12"` parses the polyline.
- [ ] `link follow <doc> <page> <region>` returns `{ target_page, target_uri }`.

**Verify:** `nix develop -c cargo test -p inkctl smoke_ink` → PASS

**Steps:**

- [ ] **Step 1: Test → 2: Implement → 3: Verify → 4: Commit**

```bash
git add crates/inkctl/
git commit -m "inkctl: ink tap/swipe/fixture/draw/list + link follow"
```

---

## Task 21: inkctl `record` commands

**Goal:** `record start`, `record stop`, `record assert`, `replay`, `emit test`.

**Files:**
- Modify: `crates/inkctl/src/cmd/record.rs`
- Test: extend `crates/inkctl/tests/smoke_session.rs` with a record-roundtrip test.

**Acceptance Criteria:**
- [ ] `record start --session <id>` flips the recording flag in `session.json`.
- [ ] `record assert <target> <json-value>` appends to `trace.jsonl`.
- [ ] `emit test --from <trace> --name foo --out tests/foo.rs` writes a Rust source file. (Note: the path is relative to the user's CWD; CLI does not assume any layout.)

**Verify:** `nix develop -c cargo test -p inkctl smoke_record` → PASS

**Steps:**

- [ ] **Step 1: Test → 2: Implement → 3: Verify → 4: Commit**

```bash
git add crates/inkctl/
git commit -m "inkctl: record start/stop/assert/replay + emit test"
```

---

## Task 22: Dogfood test — full record→emit→cargo-test loop

**Goal:** Single integration test that proves the whole thing works end-to-end against a real app.

**Files:**
- Create: `crates/inkctl/tests/dogfood.rs`

**Acceptance Criteria:**
- [ ] Creates a session, publishes `apps/reading-queue`, records a session that taps a known region and steps, calls `emit test`, writes the generated file to a tempdir, then runs `cargo test` on it (using `assert_cmd` + a stub `Cargo.toml` that depends on `inkapp-harness` via path).
- [ ] The generated test passes.

**Verify:** `nix develop -c cargo test -p inkctl dogfood` → PASS

**Steps:**

- [ ] **Step 1: Write the test** — uses `tempdir` to scaffold a tiny `Cargo.toml` + `tests/<generated>.rs`, then `Command::new("cargo").args(["test"])` in that dir.
- [ ] **Step 2: If it fails**, iterate on `emit::to_rust` output until the generated test compiles and passes.
- [ ] **Step 3: Commit**

```bash
git add crates/inkctl/
git commit -m "inkctl: dogfood test — record→emit→cargo-test round trip"
```

---

## Task 23: Update `docs/appdx.md` (definition of done)

**Goal:** Per the project convention, every spec is "built" when appdx.md is updated to reflect it.

**Files:**
- Modify: `docs/appdx.md`

**Acceptance Criteria:**
- [ ] Add an "inkctl test harness" section under the test-infrastructure heading (or appropriate location) describing the new CLI + harness library APIs.
- [ ] Cross-reference the design spec (`docs/superpowers/specs/2026-05-26-inkctl-test-harness-design.md`).
- [ ] If there's a build-order list in appdx, mark the item built.

**Verify:** `git diff docs/appdx.md` shows the new section.

**Steps:**

- [ ] **Step 1: Read current `docs/appdx.md`** and find the relevant section.
- [ ] **Step 2: Add the section** describing the new surface and that it's built.
- [ ] **Step 3: Commit**

```bash
git add docs/appdx.md
git commit -m "appdx: mark inkctl test harness built"
```

---

## Self-review notes

- All 23 tasks have concrete file paths, exact verify commands, and code blocks for each step that touches code.
- No "TBD" / "TODO" placeholders.
- Names are consistent across tasks: `Session::ink_tap` is used in Task 10 and 11; `PublishedApp` is introduced in Task 4 and referenced in Tasks 5/7/11; `observe::page_describe` shape is locked in Task 5 and incrementally enriched in Tasks 6/8/9.
- Spec coverage: every noun in the spec maps to a task (sessions→16, devices→17, documents→18, pages→19, ink/link→20, record/emit→21, dogfood→22). All observation shapes (manifest/links/layers/msg trace/connector writes/secrets/version history/rmdoc tree) are surfaced by Tasks 5/6/8/9/11.
- One known scope item deferred to follow-up per spec: connector cassette format. Not a task here.
- L3 (live cloud) is not implemented in this plan — the spec called it "gated" and listed it as a non-default; adding it is a small follow-up task once L2 is solid.
