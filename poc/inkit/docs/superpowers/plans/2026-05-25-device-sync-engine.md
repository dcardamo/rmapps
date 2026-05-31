# Device-agnostic, config-driven on-device deployment — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make on-device deployment a framework capability — apps call two device-agnostic functions; the device backend and folder come from a TOML config — eliminating the duplicated per-app `serve.rs`.

**Architecture:** A generic engine in `inkapp-core::sync` drives any `App` over a `DeviceTransport` trait (keys, PDF bytes, PDF-space strokes only — no device specifics). The reMarkable backend (`RmTransport`) lives in the renamed `rm-device` crate behind an `RmCommand` rmapi seam (real `Rmapi` / fake `FakeRm`), making path/key mapping and `.rmdoc` discovery unit-testable with no device. The `inkapp` facade resolves a `deploy.toml` (env-pointed) to a boxed backend — the only place a concrete device is named.

**Tech Stack:** Rust (workspace, edition 2021); `inkapp-core` (runtime/render/device seam); `rm-device` (transform + transport, was `inkapp-remarkable`); `zip`/`tempfile` (`.rmdoc` I/O); `toml`/`serde` (config); `tokio` (test loop); `rmapi` CLI (behind the seam, exercised only by `#[ignore]` bars).

**Conventions (apply to every task):**
- All work in the worktree `/home/dan/.paseo/worktrees/2ymhz306/jealous-husky`, branch `inkapp-device-sync`.
- Run tests with `nix develop -c cargo …`.
- Before every commit: `nix develop -c cargo fmt` (the pre-commit hook runs `cargo fmt --check`).
- **Never stage `Cargo.lock`** (`git add` only the listed files).
- Mark the native task `completed` before committing it (the commit path blocks on open/in-progress tasks).

---

### Task 0: Rename `inkapp-remarkable` → `rm-device`

**Goal:** Apply the `rm-` crate-naming convention to the reMarkable crate, with no behavior change.

**Files:**
- Move: `crates/inkapp-remarkable/` → `crates/rm-device/`
- Modify: `Cargo.toml` (workspace members), `crates/rm-device/Cargo.toml`, `crates/inkapp/Cargo.toml`, `crates/inkapp/src/lib.rs`, `crates/inkapp-harness/Cargo.toml`
- Modify (imports): `crates/inkapp-harness/tests/*.rs`, `crates/rm-device/tests/device.rs`

**Acceptance Criteria:**
- [ ] No `inkapp-remarkable` / `inkapp_remarkable` references remain (outside `docs/` and `Cargo.lock`).
- [ ] `nix develop -c cargo test --workspace` passes unchanged.

**Verify:** `nix develop -c cargo test --workspace` → all green; `grep -rn 'inkapp.remarkable' crates apps Cargo.toml` → no matches.

**Steps:**

- [ ] **Step 1: Move the crate directory**

```bash
cd /home/dan/.paseo/worktrees/2ymhz306/jealous-husky
git mv crates/inkapp-remarkable crates/rm-device
```

- [ ] **Step 2: Rename the crate package**

In `crates/rm-device/Cargo.toml` change the name:

```toml
name = "rm-device"
```

- [ ] **Step 3: Update workspace + dependent manifests**

In root `Cargo.toml`, in `members`:

```toml
    "crates/rm-device",
```

In `crates/inkapp/Cargo.toml`:

```toml
rm-device = { path = "../rm-device" }
```

In `crates/inkapp-harness/Cargo.toml` (it is a dependency line there):

```toml
rm-device = { path = "../rm-device" }
```

- [ ] **Step 4: Update the facade re-export**

In `crates/inkapp/src/lib.rs`, change the last line:

```rust
pub use rm_device::Remarkable;
```

- [ ] **Step 5: Update all import sites (mechanical)**

```bash
grep -rl 'inkapp_remarkable\|inkapp-remarkable' crates/inkapp-harness/tests crates/rm-device/tests \
  | xargs sed -i 's/inkapp_remarkable/rm_device/g; s/inkapp-remarkable/rm-device/g'
```

- [ ] **Step 6: Build + test (regenerates Cargo.lock; do not stage it)**

Run: `nix develop -c cargo test --workspace`
Expected: PASS across all crates (no behavior change).

- [ ] **Step 7: Commit**

```bash
cd /home/dan/.paseo/worktrees/2ymhz306/jealous-husky
nix develop -c cargo fmt
git add crates/rm-device crates/inkapp-harness crates/inkapp/Cargo.toml crates/inkapp/src/lib.rs Cargo.toml
git -c core.hooksPath=.githooks commit -m "rm-device: rename inkapp-remarkable to the rm- convention"
```

---

### Task 1: `DeviceTransport` seam + generic sync engine

**Goal:** Add the device-agnostic engine (`publish`/`sync_once`) and the `DeviceTransport` trait to `inkapp-core`, proven by a fake-transport test with no device.

**Files:**
- Modify: `crates/inkapp-core/src/error.rs` (add `Config`, `Transport` variants)
- Create: `crates/inkapp-core/src/sync.rs`
- Modify: `crates/inkapp-core/src/lib.rs` (register + re-export)

**Acceptance Criteria:**
- [ ] `inkapp_core::sync::{DeviceTransport, publish, sync_once}` exist and are re-exported at crate root via `pub use sync::*`-style entries.
- [ ] A `FakeTransport` test proves `publish` pushes every rendered doc and `sync_once` consults the transport, with no device.

**Verify:** `nix develop -c cargo test -p inkapp-core sync` → tests pass.

**Steps:**

- [ ] **Step 1: Add error variants**

In `crates/inkapp-core/src/error.rs`, add inside the `enum Error { … }` (after `Cache`):

```rust
    #[error("deploy config: {0}")]
    Config(String),
    #[error("device transport failed: {0}")]
    Transport(String),
```

- [ ] **Step 2: Write the engine module with its failing tests**

Create `crates/inkapp-core/src/sync.rs`:

```rust
//! Device-agnostic on-device deployment: the `DeviceTransport` seam plus the
//! generic publish/sync engine that drives an `App` over any transport. Only
//! keys, PDF bytes, and PDF-space strokes cross this boundary — no reMarkable (or
//! any device) specifics live here.

use std::collections::HashMap;

use crate::connector::ConnectorSet;
use crate::error::Result;
use crate::ink::Stroke;
use crate::reconcile::DocOp;
use crate::runtime::{App, Cycle, DocSet};

/// A device's sync transport: how rendered documents reach the hardware and how
/// the user's ink comes back. Implemented once per device family (reMarkable
/// today, in `rm-device`). Object-safe so the facade can dispatch on config.
pub trait DeviceTransport {
    /// Push a rendered document (its key + PDF bytes) to the device.
    fn push(&self, key: &str, pdf: &[u8]) -> Result<()>;
    /// Delete a document by key. Best-effort: a missing document is not an error.
    fn delete(&self, key: &str);
    /// Pull all device ink, keyed by document key, as PDF-space strokes.
    /// `page_h_by_key` lets the backend decode each document at its page height.
    fn pull(&self, page_h_by_key: &HashMap<String, f64>) -> HashMap<String, Vec<Vec<Stroke>>>;
}

/// Render the app's full document set and push every document to the device.
pub async fn publish<M, Msg, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    set: &mut DocSet,
    transport: &dyn DeviceTransport,
) -> Result<()> {
    let rendered = app.render(set).await?;
    for rd in &rendered {
        transport.push(&rd.key.0, &rd.pdf)?;
    }
    println!("published {} document(s)", rendered.len());
    Ok(())
}

/// Render to rebuild the set, pull device ink, fold one cycle, then apply the
/// resulting ops to the device (delete removed, push created/updated).
pub async fn sync_once<M, Msg: Clone, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    set: &mut DocSet,
    transport: &dyn DeviceTransport,
) -> Result<Cycle<Msg>> {
    // Rebuild the in-memory set from current state so pulled ink attributes
    // against the same manifests/page heights that were last published.
    app.render(set).await?;
    let page_h: HashMap<String, f64> = set
        .keys()
        .into_iter()
        .filter_map(|k| set.page_h(&k).map(|h| (k.0, h)))
        .collect();
    let ink = transport.pull(&page_h);
    let cycle = app.step(set, &ink).await?;
    for op in &cycle.ops {
        if let DocOp::Delete(k) = op {
            transport.delete(&k.0);
        }
    }
    for rd in &cycle.rendered {
        transport.push(&rd.key.0, &rd.pdf)?;
    }
    println!(
        "synced: {} message(s), {} op(s)",
        cycle.decoded.len(),
        cycle.ops.len()
    );
    Ok(cycle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::notice::Notice;
    use crate::connector::Connector;
    use crate::crypto::Key;
    use crate::document::{Document, Documents};
    use crate::runtime::{app, App};
    use std::sync::{Arc, Mutex};

    struct NoCx;
    impl ConnectorSet for NoCx {
        fn connectors(&self) -> Vec<Arc<dyn Connector>> {
            vec![]
        }
    }

    #[derive(Clone)]
    enum TestMsg {
        #[allow(dead_code)]
        Noop,
    }

    fn view(_m: &(), _cx: &NoCx) -> Documents<TestMsg> {
        Documents(vec![Document::keyed(
            "doc-a",
            crate::flow![Notice::line("hello")],
        )])
    }
    fn update(_msg: TestMsg, _m: &mut (), _cx: &NoCx) {}

    fn build_test_app() -> App<(), TestMsg, NoCx> {
        app(())
            .connector(NoCx)
            .update(update as fn(TestMsg, &mut (), &NoCx))
            .view(view as fn(&(), &NoCx) -> Documents<TestMsg>)
            .key(Key::from_bytes([7u8; 32]))
            .build()
    }

    #[derive(Default)]
    struct FakeTransport {
        pushed: Mutex<Vec<(String, usize)>>,
        pulled: Mutex<usize>,
    }
    impl DeviceTransport for FakeTransport {
        fn push(&self, key: &str, pdf: &[u8]) -> Result<()> {
            self.pushed.lock().unwrap().push((key.to_string(), pdf.len()));
            Ok(())
        }
        fn delete(&self, _key: &str) {}
        fn pull(&self, _p: &HashMap<String, f64>) -> HashMap<String, Vec<Vec<Stroke>>> {
            *self.pulled.lock().unwrap() += 1;
            HashMap::new()
        }
    }

    #[tokio::test]
    async fn publish_pushes_every_rendered_doc() {
        let mut application = build_test_app();
        let mut set = DocSet::default();
        let t = FakeTransport::default();
        publish(&mut application, &mut set, &t).await.unwrap();
        let pushed = t.pushed.lock().unwrap();
        assert_eq!(pushed.len(), 1);
        assert_eq!(pushed[0].0, "doc-a");
        assert!(pushed[0].1 > 0, "pushed a non-empty pdf");
    }

    #[tokio::test]
    async fn sync_once_consults_transport_and_no_ops_without_ink() {
        let mut application = build_test_app();
        let mut set = DocSet::default();
        let t = FakeTransport::default();
        let cycle = sync_once(&mut application, &mut set, &t).await.unwrap();
        assert_eq!(*t.pulled.lock().unwrap(), 1);
        assert!(cycle.ops.is_empty(), "no device ops without ink");
        assert!(cycle.decoded.is_empty());
    }
}
```

- [ ] **Step 3: Register the module**

In `crates/inkapp-core/src/lib.rs`, add to the `pub mod` list (alphabetical, after `secrets` is fine — place after `runtime`):

```rust
pub mod sync;
```

And add a re-export after the `runtime` re-export block (the trait at the crate root; the free functions are reached by full path `inkapp_core::sync::{publish, sync_once}`):

```rust
pub use sync::DeviceTransport;
```

- [ ] **Step 4: Run tests**

Run: `nix develop -c cargo test -p inkapp-core sync`
Expected: `publish_pushes_every_rendered_doc` and `sync_once_consults_transport_and_no_ops_without_ink` PASS.

- [ ] **Step 5: Commit**

```bash
nix develop -c cargo fmt
git add crates/inkapp-core/src/sync.rs crates/inkapp-core/src/error.rs crates/inkapp-core/src/lib.rs
git -c core.hooksPath=.githooks commit -m "inkapp-core: device-agnostic sync engine + DeviceTransport seam"
```

---

### Task 2: reMarkable pure helpers + `RmCommand` seam

**Goal:** Add the `RmCommand` trait and the pure, device-free `find_rmdocs`/`discover` helpers to `rm-device`, unit-tested directly against a temp dir.

**Files:**
- Create: `crates/rm-device/src/transport.rs` (helpers + trait + their tests; `RmTransport`/`Rmapi` arrive in Task 3)
- Modify: `crates/rm-device/src/lib.rs` (register module)
- Modify: `crates/rm-device/Cargo.toml` (add `zip`, `tempfile`)

**Acceptance Criteria:**
- [ ] `find_rmdocs` recurses into nested subdirs and ignores non-`.rmdoc` files.
- [ ] `discover` maps each `.rmdoc` stem to its key and the per-key page height (0.0 if unknown).

**Verify:** `nix develop -c cargo test -p rm-device transport` → tests pass.

**Steps:**

- [ ] **Step 1: Add dependencies**

In `crates/rm-device/Cargo.toml` under `[dependencies]`:

```toml
zip = "2"
tempfile = "3"
```

- [ ] **Step 2: Write the helpers + `RmCommand` with failing tests**

Create `crates/rm-device/src/transport.rs`:

```rust
//! reMarkable on-device transport: the `DeviceTransport` impl plus the `rmapi`
//! command seam it shells out through. The load-bearing logic — folder/key
//! mapping, recursive `.rmdoc` discovery, per-key page-height decode — is pure and
//! unit-tested without `rmapi` or a device, via a fake command seam.
//!
//! The real `rmapi` invocations preserve the proven invariants verbatim
//! (remarkable-pdf-mechanics.md §3, §10): always `-ni` with stdin nulled
//! (token-clobber guard); `put --content-only` (PDF-blob-only push, preserving the
//! device ink layer); folder pulls via `mget`; non-recursive `mkdir`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The `rmapi` surface the transport needs — the seam that makes the transport
/// testable without `rmapi` or a device.
pub trait RmCommand {
    /// Create `folder` (best-effort; non-recursive — create ancestors separately).
    fn mkdir(&self, folder: &str);
    /// Push `local_pdf` into `folder`, swapping only the PDF blob (content-only).
    fn put_content_only(&self, local_pdf: &Path, folder: &str) -> inkapp_core::error::Result<()>;
    /// Delete `remote_path` (best-effort; a missing document is fine).
    fn rm(&self, remote_path: &str);
    /// Pull `folder` recursively into `into_dir`. Returns false on failure.
    fn mget(&self, folder: &str, into_dir: &Path) -> bool;
}

/// A discovered on-device document pulled to disk: its key, the `.rmdoc` path, and
/// the page height to decode its ink at.
pub(crate) struct Discovered {
    pub key: String,
    pub path: PathBuf,
    pub page_h: f64,
}

/// Recursively collect `*.rmdoc` files under `dir` (mget nests downloads under a
/// subdir named after the remote folder, so we walk rather than assume flat).
pub(crate) fn find_rmdocs(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(dir, &mut out);
    out
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "rmdoc") {
            out.push(p);
        }
    }
}

/// Map each discovered `.rmdoc` to (key, path, page_h): the filename stem is the
/// key (we push `<key>.pdf`), decoded with that key's page height (0.0 if unknown).
pub(crate) fn discover(dir: &Path, page_h_by_key: &HashMap<String, f64>) -> Vec<Discovered> {
    let mut out = Vec::new();
    for path in find_rmdocs(dir) {
        let Some(key) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
            continue;
        };
        let page_h = page_h_by_key.get(&key).copied().unwrap_or(0.0);
        out.push(Discovered { key, path, page_h });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_rmdocs_recurses_and_filters() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("ReadingQueue");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("a.rmdoc"), b"x").unwrap();
        std::fs::write(nested.join("b.rmdoc"), b"y").unwrap();
        std::fs::write(nested.join("notes.txt"), b"z").unwrap();
        let mut found: Vec<String> = find_rmdocs(dir.path())
            .iter()
            .filter_map(|p| p.file_name().and_then(|s| s.to_str()).map(str::to_string))
            .collect();
        found.sort();
        assert_eq!(found, vec!["a.rmdoc".to_string(), "b.rmdoc".to_string()]);
    }

    #[test]
    fn discover_maps_stem_to_key_and_page_height() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("Agenda");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("article-7.rmdoc"), b"x").unwrap();
        std::fs::write(nested.join("orphan.rmdoc"), b"y").unwrap();
        let mut page_h = HashMap::new();
        page_h.insert("article-7".to_string(), 560.0);
        let got = discover(dir.path(), &page_h);
        let by_key: HashMap<&str, f64> = got.iter().map(|d| (d.key.as_str(), d.page_h)).collect();
        assert_eq!(by_key.get("article-7"), Some(&560.0));
        assert_eq!(by_key.get("orphan"), Some(&0.0));
    }
}
```

- [ ] **Step 3: Register the module**

In `crates/rm-device/src/lib.rs`, after the existing `use` block (before `const CANVAS_W`), add:

```rust
mod transport;
pub use transport::{RmCommand};
```

(The `RmTransport`/`Rmapi` re-exports are added in Task 3.)

- [ ] **Step 4: Run tests**

Run: `nix develop -c cargo test -p rm-device transport`
Expected: `find_rmdocs_recurses_and_filters` and `discover_maps_stem_to_key_and_page_height` PASS.

- [ ] **Step 5: Commit**

```bash
nix develop -c cargo fmt
git add crates/rm-device/src/transport.rs crates/rm-device/src/lib.rs crates/rm-device/Cargo.toml
git -c core.hooksPath=.githooks commit -m "rm-device: RmCommand seam + pure .rmdoc discovery helpers"
```

---

### Task 3: `RmTransport` + real `Rmapi` seam + `DeviceTransport` impl

**Goal:** Implement the reMarkable transport over the seam (real `Rmapi`), and prove push/delete/pull wiring with a `FakeRm` — no rmapi, no device.

**Files:**
- Modify: `crates/rm-device/src/transport.rs` (add `Rmapi`, `RmTransport`, `strokes_from_rmdoc`, `DeviceTransport` impl, FakeRm tests)
- Modify: `crates/rm-device/src/lib.rs` (export `RmTransport`, `Rmapi`)

**Acceptance Criteria:**
- [ ] `RmTransport::push` writes `<key>.pdf` and puts it under the folder; `delete` targets `{folder}/{key}`.
- [ ] `RmTransport::pull` decodes `.rm` ink out of a pulled `.rmdoc` and maps it back to its key at the right page height.
- [ ] The real `Rmapi` impl reproduces the documented invocations verbatim.

**Verify:** `nix develop -c cargo test -p rm-device` → all pass.

**Steps:**

- [ ] **Step 1: Add the real seam, transport, and decode to `transport.rs`**

Replace the `use` header of `crates/rm-device/src/transport.rs` with:

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use inkapp_core::device::Device;
use inkapp_core::error::{Error, Result};
use inkapp_core::ink::Stroke;
use inkapp_core::sync::DeviceTransport;

use crate::Remarkable;
```

Then append (after the `discover` fn, before the `#[cfg(test)]` module):

```rust
/// The real seam: shells out to the `rmapi` CLI.
pub struct Rmapi;

impl RmCommand for Rmapi {
    fn mkdir(&self, folder: &str) {
        let _ = Command::new("rmapi")
            .args(["-ni", "mkdir", folder])
            .stdin(Stdio::null())
            .status();
    }

    fn put_content_only(&self, local_pdf: &Path, folder: &str) -> Result<()> {
        let path = local_pdf
            .to_str()
            .ok_or_else(|| Error::Transport("non-utf8 pdf path".into()))?;
        let ok = Command::new("rmapi")
            .args(["-ni", "put", "--content-only", path, folder])
            .stdin(Stdio::null())
            .status()
            .map_err(|e| Error::Transport(format!("rmapi put: {e}")))?
            .success();
        if ok {
            Ok(())
        } else {
            Err(Error::Transport(format!("rmapi put failed for {path}")))
        }
    }

    fn rm(&self, remote_path: &str) {
        let _ = Command::new("rmapi")
            .args(["-ni", "rm", remote_path])
            .stdin(Stdio::null())
            .status();
    }

    fn mget(&self, folder: &str, into_dir: &Path) -> bool {
        Command::new("rmapi")
            .args(["-ni", "mget", folder])
            .current_dir(into_dir)
            .stdin(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// Read the first `.rm` entry's PDF-space strokes out of an `.rmdoc` zip, via the
/// device transform. Empty if the document has no ink yet.
fn strokes_from_rmdoc(device: &Remarkable, path: &Path, page_h: f64) -> Vec<Stroke> {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        return Vec::new();
    };
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .collect();
    let Some(rm) = names.into_iter().find(|n| n.ends_with(".rm")) else {
        return Vec::new();
    };
    let mut bytes = Vec::new();
    if zip.by_name(&rm).unwrap().read_to_end(&mut bytes).is_err() {
        return Vec::new();
    }
    device.read_ink(&bytes, page_h).unwrap_or_default()
}

/// reMarkable transport: maps the framework's key/PDF/ink model onto `rmapi` and
/// the `.rmdoc` zip format. Generic over the command seam so tests inject a fake.
pub struct RmTransport<C: RmCommand = Rmapi> {
    folder: String,
    device: Remarkable,
    cmd: C,
}

impl RmTransport<Rmapi> {
    /// A transport that shells out to the real `rmapi`, deploying under `folder`.
    pub fn new(folder: impl Into<String>) -> Self {
        Self::with_command(Rmapi, folder)
    }
}

impl<C: RmCommand> RmTransport<C> {
    /// A transport over an explicit command seam (tests pass a fake).
    pub fn with_command(cmd: C, folder: impl Into<String>) -> Self {
        Self {
            folder: folder.into(),
            device: Remarkable::new(),
            cmd,
        }
    }
}

impl<C: RmCommand> DeviceTransport for RmTransport<C> {
    fn push(&self, key: &str, pdf: &[u8]) -> Result<()> {
        self.cmd.mkdir(&self.folder);
        // The on-device visibleName is the file stem, so name the temp file <key>.pdf.
        let tmp = std::env::temp_dir().join(format!("{key}.pdf"));
        std::fs::write(&tmp, pdf).map_err(|e| Error::Transport(format!("write {key}.pdf: {e}")))?;
        self.cmd.put_content_only(&tmp, &self.folder)
    }

    fn delete(&self, key: &str) {
        self.cmd.rm(&format!("{}/{}", self.folder, key));
    }

    fn pull(&self, page_h_by_key: &HashMap<String, f64>) -> HashMap<String, Vec<Vec<Stroke>>> {
        let mut out = HashMap::new();
        let Ok(dir) = tempfile::tempdir() else {
            return out;
        };
        if !self.cmd.mget(&self.folder, dir.path()) {
            return out;
        }
        for d in discover(dir.path(), page_h_by_key) {
            let strokes = strokes_from_rmdoc(&self.device, &d.path, d.page_h);
            if !strokes.is_empty() {
                // Single-page for now; multi-page rmdoc support is a future step.
                out.insert(d.key, vec![strokes]);
            }
        }
        out
    }
}
```

- [ ] **Step 2: Extend the test module with `FakeRm` + transport tests**

Append inside the existing `#[cfg(test)] mod tests { … }` in `transport.rs` (after the two helper tests):

```rust
    use super::{RmTransport, Rmapi};
    use inkapp_core::geometry::PdfPoint;
    use inkapp_core::ink::Stroke;
    use std::path::Path;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeRm {
        puts: Mutex<Vec<(String, String)>>, // (file_name, folder)
        rms: Mutex<Vec<String>>,
        mget_doc: Option<(String, Vec<u8>)>, // (key, .rm bytes) materialized on mget
    }

    impl RmCommand for FakeRm {
        fn mkdir(&self, _folder: &str) {}
        fn put_content_only(
            &self,
            local_pdf: &Path,
            folder: &str,
        ) -> inkapp_core::error::Result<()> {
            let name = local_pdf.file_name().unwrap().to_str().unwrap().to_string();
            self.puts.lock().unwrap().push((name, folder.to_string()));
            Ok(())
        }
        fn rm(&self, remote_path: &str) {
            self.rms.lock().unwrap().push(remote_path.to_string());
        }
        fn mget(&self, _folder: &str, into_dir: &Path) -> bool {
            if let Some((key, rm_bytes)) = &self.mget_doc {
                // Mimic mget's nested layout: <into_dir>/<remote-folder>/<key>.rmdoc
                let nested = into_dir.join("RemoteFolder");
                std::fs::create_dir_all(&nested).unwrap();
                let f = std::fs::File::create(nested.join(format!("{key}.rmdoc"))).unwrap();
                let mut zw = zip::ZipWriter::new(f);
                zw.start_file("page0.rm", zip::write::SimpleFileOptions::default())
                    .unwrap();
                use std::io::Write;
                zw.write_all(rm_bytes).unwrap();
                zw.finish().unwrap();
            }
            true
        }
    }

    #[test]
    fn push_maps_key_to_pdf_named_under_folder() {
        let t = RmTransport::with_command(FakeRm::default(), "/ReadingQueue");
        t.push("article-7", b"%PDF fake bytes").unwrap();
        let puts = t.cmd.puts.lock().unwrap();
        assert_eq!(puts.len(), 1);
        assert_eq!(puts[0].0, "article-7.pdf");
        assert_eq!(puts[0].1, "/ReadingQueue");
    }

    #[test]
    fn delete_targets_folder_slash_key() {
        let t = RmTransport::with_command(FakeRm::default(), "/Agenda");
        t.delete("event-3");
        assert_eq!(*t.cmd.rms.lock().unwrap(), vec!["/Agenda/event-3".to_string()]);
    }

    #[test]
    fn pull_decodes_ink_under_the_right_key() {
        // Build real .rm bytes for one stroke via the transform's inverse. We do
        // not assert coordinates (the harness already proves the transform); we
        // assert the ink maps back to its key at the requested page height.
        let device = Remarkable::new();
        let page_h = 560.0;
        let strokes = vec![Stroke {
            points: vec![
                PdfPoint { x: 100.0, y: 200.0 },
                PdfPoint { x: 150.0, y: 220.0 },
            ],
            highlighter: false,
        }];
        let rm_bytes = device.write_ink(&strokes, page_h).unwrap();

        let fake = FakeRm {
            mget_doc: Some(("article-7".to_string(), rm_bytes)),
            ..FakeRm::default()
        };
        let t = RmTransport::with_command(fake, "/ReadingQueue");

        let mut page_h_by_key = HashMap::new();
        page_h_by_key.insert("article-7".to_string(), page_h);
        let ink = t.pull(&page_h_by_key);

        assert!(ink.contains_key("article-7"), "ink mapped back to its key");
        assert_eq!(ink["article-7"].len(), 1, "single page");
        assert_eq!(ink["article-7"][0].len(), 1, "one stroke round-tripped");
    }
```

- [ ] **Step 3: Export the transport**

In `crates/rm-device/src/lib.rs`, change the transport re-export line to:

```rust
pub use transport::{Rmapi, RmCommand, RmTransport};
```

- [ ] **Step 4: Run tests**

Run: `nix develop -c cargo test -p rm-device`
Expected: helper tests + `push_maps_key_to_pdf_named_under_folder`, `delete_targets_folder_slash_key`, `pull_decodes_ink_under_the_right_key`, and the existing `device.rs` tests all PASS.

- [ ] **Step 5: Commit**

```bash
nix develop -c cargo fmt
git add crates/rm-device/src/transport.rs crates/rm-device/src/lib.rs
git -c core.hooksPath=.githooks commit -m "rm-device: RmTransport over rmapi seam, tested with a fake (no device)"
```

---

### Task 4: Facade config resolution + app-facing `publish`/`sync_once`

**Goal:** Add `DeployConfig` (TOML, env-pointed) and the config-driven `inkapp::publish`/`inkapp::sync_once`, resolving the only named backend.

**Files:**
- Create: `crates/inkapp/src/deploy.rs`
- Modify: `crates/inkapp/src/lib.rs`, `crates/inkapp/Cargo.toml`
- Create: `crates/inkapp/tests/deploy.rs`

**Acceptance Criteria:**
- [ ] `DeployConfig::from_toml` parses backend+folder and defaults backend to `"remarkable"`; missing folder errors.
- [ ] `resolve` returns a transport for `"remarkable"` and errors for any other backend.
- [ ] `inkapp::publish`/`inkapp::sync_once` exist and read config from `INKAPP_DEPLOY_CONFIG`.

**Verify:** `nix develop -c cargo test -p inkapp` → all pass.

**Steps:**

- [ ] **Step 1: Add dependencies**

In `crates/inkapp/Cargo.toml`, under `[dependencies]` add:

```toml
serde = { version = "1", features = ["derive"] }
toml = "0.8"
```

And add a dev-dependencies section:

```toml
[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
tempfile = "3"
```

- [ ] **Step 2: Write the deploy module (with in-crate resolve test)**

Create `crates/inkapp/src/deploy.rs`:

```rust
//! Config-driven, device-agnostic on-device deployment. Apps call
//! `inkapp::publish` / `inkapp::sync_once`; the backend and target folder come
//! from a `deploy.toml` located via the `INKAPP_DEPLOY_CONFIG` env var. This is
//! the only place a concrete device backend is named.

use std::path::Path;

use inkapp_core::connector::ConnectorSet;
use inkapp_core::error::{Error, Result};
use inkapp_core::runtime::{App, Cycle, DocSet};
use inkapp_core::sync::{self, DeviceTransport};

use rm_device::RmTransport;

/// Env var naming the path to the deploy TOML.
const CONFIG_ENV: &str = "INKAPP_DEPLOY_CONFIG";

fn default_backend() -> String {
    "remarkable".to_string()
}

/// Deployment configuration: which device backend, and the device folder this
/// app's documents live under.
#[derive(Debug, serde::Deserialize)]
pub struct DeployConfig {
    #[serde(default = "default_backend")]
    pub backend: String,
    pub folder: String,
}

impl DeployConfig {
    /// Parse a `DeployConfig` from TOML text.
    pub fn from_toml(text: &str) -> Result<Self> {
        toml::from_str(text).map_err(|e| Error::Config(format!("parse deploy config: {e}")))
    }

    /// Load from the file named by `INKAPP_DEPLOY_CONFIG`.
    pub fn from_env() -> Result<Self> {
        let path =
            std::env::var(CONFIG_ENV).map_err(|_| Error::Config(format!("{CONFIG_ENV} is not set")))?;
        Self::from_path(path)
    }

    fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let text = std::fs::read_to_string(path.as_ref())
            .map_err(|e| Error::Config(format!("read deploy config {:?}: {e}", path.as_ref())))?;
        Self::from_toml(&text)
    }
}

/// Resolve a config into a concrete transport. The single place backends are
/// named; a new device family adds one arm and one `*-device` crate.
fn resolve(cfg: &DeployConfig) -> Result<Box<dyn DeviceTransport>> {
    match cfg.backend.as_str() {
        "remarkable" => Ok(Box::new(RmTransport::new(cfg.folder.clone()))),
        other => Err(Error::Config(format!("unknown deploy backend {other:?}"))),
    }
}

/// Render the app's document set and push every document to the configured device.
pub async fn publish<M, Msg, Cx: ConnectorSet>(app: &mut App<M, Msg, Cx>) -> Result<()> {
    let transport = resolve(&DeployConfig::from_env()?)?;
    let mut set = DocSet::default();
    sync::publish(app, &mut set, transport.as_ref()).await
}

/// Pull device ink, fold one cycle, and apply the resulting ops to the device.
pub async fn sync_once<M, Msg: Clone, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
) -> Result<Cycle<Msg>> {
    let transport = resolve(&DeployConfig::from_env()?)?;
    let mut set = DocSet::default();
    sync::sync_once(app, &mut set, transport.as_ref()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_known_and_unknown_backends() {
        let ok = DeployConfig {
            backend: "remarkable".into(),
            folder: "/X".into(),
        };
        assert!(resolve(&ok).is_ok());
        let bad = DeployConfig {
            backend: "supernote".into(),
            folder: "/X".into(),
        };
        assert!(resolve(&bad).is_err());
    }
}
```

- [ ] **Step 3: Register + re-export in the facade**

In `crates/inkapp/src/lib.rs`, add after the `pub use inkapp_core::*` block and before `pub use rm_device::Remarkable;`:

```rust
mod deploy;
pub use deploy::{publish, sync_once, DeployConfig};
pub use inkapp_core::sync::DeviceTransport;
```

- [ ] **Step 4: Write the public config tests**

Create `crates/inkapp/tests/deploy.rs`:

```rust
use inkapp::DeployConfig;

#[test]
fn parses_explicit_backend_and_folder() {
    let cfg =
        DeployConfig::from_toml("backend = \"remarkable\"\nfolder = \"/ReadingQueue\"").unwrap();
    assert_eq!(cfg.backend, "remarkable");
    assert_eq!(cfg.folder, "/ReadingQueue");
}

#[test]
fn backend_defaults_to_remarkable() {
    let cfg = DeployConfig::from_toml("folder = \"/Agenda\"").unwrap();
    assert_eq!(cfg.backend, "remarkable");
}

#[test]
fn missing_folder_is_an_error() {
    assert!(DeployConfig::from_toml("backend = \"remarkable\"").is_err());
}
```

- [ ] **Step 5: Run tests**

Run: `nix develop -c cargo test -p inkapp`
Expected: `resolve_known_and_unknown_backends`, the three `deploy.rs` tests, and the existing `facade.rs` test PASS.

- [ ] **Step 6: Commit**

```bash
nix develop -c cargo fmt
git add crates/inkapp/src/deploy.rs crates/inkapp/src/lib.rs crates/inkapp/Cargo.toml crates/inkapp/tests/deploy.rs
git -c core.hooksPath=.githooks commit -m "inkapp: config-driven publish/sync_once resolving deploy.toml backend"
```

---

### Task 5: Migrate `reading-queue` and `agenda` to the framework surface

**Goal:** Delete both `serve.rs` copies and rewrite the manual device bars to call `inkapp::publish`/`inkapp::sync_once`; workspace stays green.

**Files:**
- Delete: `apps/reading-queue/src/serve.rs`, `apps/agenda/src/serve.rs`
- Modify: `apps/reading-queue/src/lib.rs`, `apps/agenda/src/lib.rs` (drop `pub mod serve;`)
- Modify: `apps/reading-queue/tests/device.rs`, `apps/agenda/tests/device.rs`
- Modify: `apps/reading-queue/Cargo.toml`, `apps/agenda/Cargo.toml` (drop `zip`)
- Modify: `.gitignore` (ignore per-app `deploy.toml`)

**Acceptance Criteria:**
- [ ] No `serve.rs` remains; no `pub mod serve;` remains.
- [ ] Both `#[ignore]` device bars compile and call the facade.
- [ ] `nix develop -c cargo test --workspace` is green.

**Verify:** `nix develop -c cargo test --workspace` → green; `find apps -name serve.rs` → empty.

**Steps:**

- [ ] **Step 1: Delete the duplicated transports**

```bash
cd /home/dan/.paseo/worktrees/2ymhz306/jealous-husky
git rm apps/reading-queue/src/serve.rs apps/agenda/src/serve.rs
```

- [ ] **Step 2: Drop the module declarations**

In `apps/reading-queue/src/lib.rs` remove the line `pub mod serve;` (line 4).
In `apps/agenda/src/lib.rs` remove the line `pub mod serve;` (line 7).

- [ ] **Step 3: Drop the now-unused `zip` dependency**

In `apps/reading-queue/Cargo.toml` remove the line `zip = "2"`.
In `apps/agenda/Cargo.toml` remove the line `zip = "2"`.

- [ ] **Step 4: Rewrite `apps/reading-queue/tests/device.rs`**

```rust
//! Manual on-device bars. Requires a paired reMarkable, an authenticated `rmapi`,
//! and a deploy config: set `INKAPP_DEPLOY_CONFIG` to a `deploy.toml` with
//! `backend = "remarkable"` and `folder = "/ReadingQueue"`. Two steps, run as
//! separate processes so inking happens out-of-band:
//!
//!   1. publish the queue to the device:
//!      nix develop -c cargo test -p reading-queue --test device -- --ignored --nocapture publish_to_device
//!   2. (on the tablet: open the docs, highlight a word, tick an Archive box, then SYNC)
//!   3. pull + fold + re-push:
//!      nix develop -c cargo test -p reading-queue --test device -- --ignored --nocapture sync_from_device
//!
//! State persists between the two runs via the gitignored overlay file
//! (`.overlay.json`). Honors rmapi v4/token/mkdir notes (remarkable-pdf-mechanics.md §10).

use inkapp::{app, App as Framework, SecretStore};
use reading_queue::{update, view, App, Connectors, Msg};

/// Gitignored overlay path so manual archives/highlights survive between the two runs.
const OVERLAY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/.overlay.json");

/// Build the assembled app with the persisted (device-use) connector.
fn build_app() -> Framework<App, Msg, Connectors> {
    let key = SecretStore::open_default()
        .and_then(|mut s| s.user_key())
        .expect("open secrets store / load user key");
    app(App)
        .connector(Connectors::persisted(OVERLAY))
        .update(update)
        .view(view)
        .key(key)
        .build()
}

#[tokio::test]
#[ignore = "manual: requires a paired reMarkable + rmapi + INKAPP_DEPLOY_CONFIG"]
async fn publish_to_device() {
    let mut application = build_app();
    inkapp::publish(&mut application).await.expect("publish");
    eprintln!(
        "Published. On the tablet: open the docs under /ReadingQueue, highlight a word in one \
         article and tick the Archive box in another, then SYNC the device. Then run \
         `sync_from_device`."
    );
}

#[tokio::test]
#[ignore = "manual: requires a paired reMarkable + rmapi + INKAPP_DEPLOY_CONFIG; run after inking + syncing"]
async fn sync_from_device() {
    let mut application = build_app();
    inkapp::sync_once(&mut application).await.expect("sync");
    eprintln!(
        "Synced. Archived articles are deleted; highlights are baked into the bodies on re-push."
    );
}
```

- [ ] **Step 5: Rewrite `apps/agenda/tests/device.rs`**

```rust
//! Manual on-device bars for the agenda app. Requires a paired reMarkable, an
//! authenticated `rmapi`, and a deploy config: set `INKAPP_DEPLOY_CONFIG` to a
//! `deploy.toml` with `backend = "remarkable"` and `folder = "/Agenda"`.
//!
//!   1. publish the agenda to the device:
//!      nix develop -c cargo test -p agenda --test device -- --ignored --nocapture publish_to_device
//!   2. (on the tablet: mark the cancel box on an event in the editable calendar, then SYNC)
//!   3. pull + fold + re-push:
//!      nix develop -c cargo test -p agenda --test device -- --ignored --nocapture sync_from_device
//!
//! State persists between the two runs via the gitignored local-calendar store
//! (`.localcal.json`).

use agenda::{update, view, App, Connectors, Msg};
use inkapp::{app, App as Framework, SecretStore};

/// Gitignored local-calendar store so manual cancels survive between the two runs.
const STORE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/.localcal.json");

/// Build the assembled app with the persisted (device-use) connectors.
fn build_app() -> Framework<App, Msg, Connectors> {
    let key = SecretStore::open_default()
        .and_then(|mut s| s.user_key())
        .expect("open secrets store / load user key");
    app(App)
        .connector(Connectors::persisted(STORE))
        .update(update)
        .view(view)
        .key(key)
        .build()
}

#[tokio::test]
#[ignore = "manual: requires a paired reMarkable + rmapi + INKAPP_DEPLOY_CONFIG"]
async fn publish_to_device() {
    let mut application = build_app();
    inkapp::publish(&mut application).await.expect("publish");
    eprintln!(
        "Published. On the tablet: open the agenda doc under /Agenda, mark the cancel box on an \
         event in the editable (lower) calendar, then SYNC the device. Then run `sync_from_device`."
    );
}

#[tokio::test]
#[ignore = "manual: requires a paired reMarkable + rmapi + INKAPP_DEPLOY_CONFIG; run after inking + syncing"]
async fn sync_from_device() {
    let mut application = build_app();
    inkapp::sync_once(&mut application).await.expect("sync");
    eprintln!("Synced. A cancelled event is reflected on the editable calendar on re-push.");
}
```

- [ ] **Step 6: Ignore per-app deploy config**

Append to `.gitignore`:

```
/apps/reading-queue/deploy.toml
/apps/agenda/deploy.toml
```

- [ ] **Step 7: Build + test the workspace**

Run: `nix develop -c cargo test --workspace`
Expected: green. (The `#[ignore]` device bars compile but do not run.)

- [ ] **Step 8: Commit**

```bash
nix develop -c cargo fmt
git add apps/reading-queue apps/agenda .gitignore
git -c core.hooksPath=.githooks commit -m "apps: deploy via framework surface; delete duplicated serve.rs"
```

---

### Task 6: Record the pushed-down capability in `docs/appdx.md`

**Goal:** Note in the appendix that on-device deployment is now framework-provided, config-driven, and device-agnostic.

**Files:**
- Modify: `docs/appdx.md`

**Acceptance Criteria:**
- [ ] `docs/appdx.md` documents the new deployment capability and the removal of per-app `serve.rs`.

**Verify:** `grep -n "framework-provided" docs/appdx.md` → matches the new section.

**Steps:**

- [ ] **Step 1: Insert a capability section**

In `docs/appdx.md`, immediately before the line `## Open questions parking lot`, insert:

```markdown
## On-device deployment is framework-provided

On-device deployment is no longer per-app code. An app deploys with two
device-agnostic calls — `inkapp::publish(&mut app)` and
`inkapp::sync_once(&mut app)` — and the device backend plus target folder come
from a `deploy.toml` (located via `INKAPP_DEPLOY_CONFIG`), resolved by the
`inkapp` facade. The generic engine (`inkapp-core::sync`) drives any
`DeviceTransport`; the reMarkable backend (`rm-device::RmTransport`, over an
`rmapi` command seam) is today's only implementation. Adding a device family is a
new `*-device` crate plus one `match` arm in the facade — apps and the engine are
untouched. The old per-app `serve.rs` (duplicated across reading-queue and
agenda) is gone.

---

```

- [ ] **Step 2: Commit**

```bash
git add docs/appdx.md
git -c core.hooksPath=.githooks commit -m "appdx: on-device deployment is now framework-provided"
```

---

## Self-Review

**Spec coverage:**
- Rename `inkapp-remarkable` → `rm-device` → Task 0. ✓
- `DeviceTransport` trait + generic engine (`inkapp-core::sync`) → Task 1. ✓
- `RmCommand` seam + pure `find_rmdocs`/`discover` → Task 2. ✓
- `RmTransport` + real `Rmapi` + `FakeRm` push/delete/pull tests → Task 3. ✓
- Facade `DeployConfig` (TOML/env), `resolve`, app-facing `publish`/`sync_once` → Task 4. ✓
- Delete both `serve.rs`, rewrite device bars, drop `zip`, gitignore `deploy.toml` → Task 5. ✓
- appdx note → Task 6. ✓
- "No device, no rmapi" unit tests (mapping, discovery, page-height wiring) → Tasks 2–3. ✓
- Preserve rmapi invariants verbatim → Task 3 `Rmapi` impl + module doc-comment. ✓
- `cargo test --workspace` green → Tasks 0 and 5 verify. ✓

**Type consistency:** `DeviceTransport::{push,delete,pull}` signatures match across Tasks 1/3/4; engine takes `&dyn DeviceTransport` everywhere; `RmTransport::{new,with_command}`, `RmCommand::{mkdir,put_content_only,rm,mget}`, `discover`/`find_rmdocs`/`Discovered`, and `DeployConfig::{from_toml,from_env,from_path}` + `resolve` are used consistently.

**Placeholder scan:** none — every step contains full code or exact commands.
