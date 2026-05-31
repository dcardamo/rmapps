# Serve Loop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a reusable, transport-agnostic `serve` loop to inkapp that publishes once and then runs `sync_once` on an interval until shutdown, plus `run` and `sync` CLI subcommands in the reading-queue worked example.

**Architecture:** New `serve` function added to `inkapp_core::sync` next to `publish`/`sync_once`. Shutdown is a parameter (a `Future<Output=()>`) so the loop is testable without signal plumbing; the binary passes `tokio::signal::ctrl_c()`. The CLI grows a `Cmd` enum; no subcommand → today's publish-once behaviour (preserved). Interval source: `--interval <secs>` overrides `DeviceConfig.sync_interval_secs` (default 30).

**Tech Stack:** Rust, tokio (`select!`, `sleep`, `signal::ctrl_c`), clap subcommands, the existing `App`/`DocSet`/`DeviceTransport` runtime.

**Spec:** `docs/superpowers/specs/2026-05-25-serve-loop-design.md`

**File map:**
- Modify `crates/inkapp-core/src/geometry.rs` — add `sync_interval_secs` to `DeviceConfig`.
- Modify `crates/inkapp-core/src/sync.rs` — add `serve`, extend `FakeTransport`, add 3 tests + a tiny "archive on ink" test app.
- Modify `crates/inkapp/src/deploy.rs` — add `serve` facade.
- Modify `crates/inkapp/src/lib.rs` — single-line `pub use deploy::serve;`.
- Modify `apps/reading-queue/src/main.rs` — `Cmd` enum with `Run`/`Sync`/`Config`, default branch unchanged.
- Modify `docs/appdx.md` — mark publish/sync_once/serve as built; describe the loop.

**Conventions (repeat in every commit):**
- Run `nix develop -c cargo test --workspace` and `nix develop -c cargo clippy --all-targets -- -D warnings` before committing.
- Do NOT stage `Cargo.lock`.
- Clear `.tasks.json` open items before committing (pre-commit hook blocks).
- Format with `nix develop -c cargo fmt` before committing.

---

### Task 1: Add `sync_interval_secs` to `DeviceConfig`

**Goal:** Make polling cadence a configurable field on the existing `[device]` section, with a 30s default.

**Files:**
- Modify: `crates/inkapp-core/src/geometry.rs:114-121`

**Acceptance Criteria:**
- [ ] `DeviceConfig` carries `pub sync_interval_secs: u64` with default `30`.
- [ ] `nix develop -c cargo test -p inkapp-core` passes.
- [ ] `nix develop -c cargo test --workspace` still passes (no caller breaks — the field has a serde + config default).

**Verify:** `nix develop -c cargo test --workspace` → all green.

**Steps:**

- [ ] **Step 1: Edit `DeviceConfig` to add the new field**

In `crates/inkapp-core/src/geometry.rs`, replace the existing struct:

```rust
/// The `[device]` config section — which device backend to deploy to, and the
/// polling cadence the `serve` loop uses between sync cycles. The per-app target
/// folder lives in each app's own config section, not here.
#[derive(Debug, Clone, serde::Deserialize, inkapp_config::Config)]
#[serde(default)]
#[config(kind = "device", namespace = "framework")]
pub struct DeviceConfig {
    /// Device backend identifier (e.g. "remarkable").
    #[config(default = String::from("remarkable"))]
    pub backend: String,
    /// Seconds between sync cycles when running `serve`. The `--interval` CLI
    /// flag, when provided, overrides this.
    #[config(default = 30u64)]
    pub sync_interval_secs: u64,
}
```

- [ ] **Step 2: Run workspace tests**

```bash
nix develop -c cargo test --workspace
```

Expected: all tests pass. `DeviceConfig` is consumed by `crates/inkapp/src/deploy.rs` (only `backend` is read) and by `reading-queue/src/main.rs` (same). Adding the field is additive.

- [ ] **Step 3: Run clippy + fmt**

```bash
nix develop -c cargo fmt
nix develop -c cargo clippy --all-targets -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add crates/inkapp-core/src/geometry.rs
git commit -m "inkapp-core: add sync_interval_secs to DeviceConfig (default 30s)"
```

---

### Task 2: Extend `FakeTransport` to script pull responses and observe deletes

**Goal:** Make the existing test fake able to (a) return scripted ink per `pull` call, and (b) record `delete` calls — without breaking the two existing tests.

**Files:**
- Modify: `crates/inkapp-core/src/sync.rs:117-159`

**Acceptance Criteria:**
- [ ] `FakeTransport` keeps a `VecDeque<HashMap<String, Vec<Vec<Stroke>>>>` of canned pulls; `pull` pops front; empty queue → `HashMap::new()`.
- [ ] `FakeTransport` records every `delete(key)` call.
- [ ] Existing tests `publish_pushes_every_rendered_doc` and `sync_once_consults_transport_and_no_ops_without_ink` still pass unchanged.

**Verify:** `nix develop -c cargo test -p inkapp-core sync::tests` → all green.

**Steps:**

- [ ] **Step 1: Replace the `FakeTransport` definition in the test module**

In `crates/inkapp-core/src/sync.rs`, replace lines 117–136 (the `FakeTransport` struct + impl) with:

```rust
use std::collections::VecDeque;

#[derive(Default)]
struct FakeTransport {
    pushed: Mutex<Vec<(String, usize)>>,
    canned_pulls: Mutex<VecDeque<HashMap<String, Vec<Vec<Stroke>>>>>,
    pulls_done: Mutex<usize>,
    deleted: Mutex<Vec<String>>,
}

impl FakeTransport {
    /// Seed the queue of pull responses (front = first call).
    fn with_pulls(pulls: Vec<HashMap<String, Vec<Vec<Stroke>>>>) -> Self {
        Self {
            canned_pulls: Mutex::new(pulls.into_iter().collect()),
            ..Self::default()
        }
    }
}

#[async_trait::async_trait]
impl DeviceTransport for FakeTransport {
    async fn push(&self, key: &str, pdf: &[u8]) -> Result<()> {
        self.pushed
            .lock()
            .unwrap()
            .push((key.to_string(), pdf.len()));
        Ok(())
    }
    async fn delete(&self, key: &str) {
        self.deleted.lock().unwrap().push(key.to_string());
    }
    async fn pull(&self, _p: &HashMap<String, f64>) -> HashMap<String, Vec<Vec<Stroke>>> {
        *self.pulls_done.lock().unwrap() += 1;
        self.canned_pulls
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_default()
    }
}
```

The existing test `sync_once_consults_transport_and_no_ops_without_ink` asserts `*t.pulled.lock().unwrap() == 1`; update that assertion to use the new field name. Find that line (currently `assert_eq!(*t.pulled.lock().unwrap(), 1);`) and change it to:

```rust
assert_eq!(*t.pulls_done.lock().unwrap(), 1);
```

- [ ] **Step 2: Run the sync tests**

```bash
nix develop -c cargo test -p inkapp-core sync::tests
```

Expected: both existing tests pass.

- [ ] **Step 3: Run workspace tests + clippy**

```bash
nix develop -c cargo test --workspace
nix develop -c cargo fmt
nix develop -c cargo clippy --all-targets -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add crates/inkapp-core/src/sync.rs
git commit -m "inkapp-core: extend FakeTransport with scripted pulls and delete log"
```

---

### Task 3: Add the `serve` loop with TDD coverage

**Goal:** Implement `serve(app, set, transport, interval, shutdown)` plus three tests proving (a) publish before first pull, (b) archive-on-ink delete path, (c) two-cycle behaviour (decode then quiet).

**Files:**
- Modify: `crates/inkapp-core/src/sync.rs` — add `serve` (top-level) and a "remove on ink" test app in the `tests` module.

**Acceptance Criteria:**
- [ ] `pub async fn serve<M, Msg: Clone, Cx: ConnectorSet>(app, set, transport, interval, shutdown) -> Result<()>` is defined.
- [ ] `serve` calls `publish` once, then `tokio::select!`s between `sleep(interval)` and the shutdown future per cycle.
- [ ] Each cycle prints one summary line; non-empty cycles also print `msg:`/`op:` lines.
- [ ] Three new tests pass:
  - `serve_publishes_before_first_pull`
  - `sync_once_archives_doc_on_ink`
  - `serve_two_cycles_decode_then_quiet`

**Verify:** `nix develop -c cargo test -p inkapp-core sync::tests` → 5 tests pass (2 existing + 3 new).

**Steps:**

- [ ] **Step 1: Add `serve` to the top of `sync.rs` (after `sync_once`)**

Append after `sync_once`, before `#[cfg(test)]`:

```rust
use std::time::Duration;
use std::future::Future;

use crate::reconcile::DocOp as _DocOp; // already imported above; remove this line if duplicate

/// Drive the device round-trip: publish the current document set, then loop —
/// every `interval`, run one `sync_once` cycle and log the decoded messages and
/// reconcile ops. Returns when `shutdown` resolves.
///
/// Transport-agnostic by construction. The shutdown future is a parameter so
/// callers can plumb `tokio::signal::ctrl_c()`, a oneshot, a `Notify`, or an
/// immediately-ready future (for tests).
pub async fn serve<M, Msg, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    set: &mut DocSet,
    transport: &dyn DeviceTransport,
    interval: Duration,
    shutdown: impl Future<Output = ()>,
) -> Result<()>
where
    Msg: Clone + std::fmt::Debug,
{
    publish(app, set, transport).await?;
    let mut shutdown = Box::pin(shutdown);
    let mut n: u64 = 0;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            _ = &mut shutdown => {
                println!("serve: shutdown");
                return Ok(());
            }
        }
        n += 1;
        let cycle = sync_once(app, set, transport).await?;
        let pushes = cycle
            .ops
            .iter()
            .filter(|o| matches!(o, DocOp::Create(_) | DocOp::Update(_)))
            .count();
        let deletes = cycle
            .ops
            .iter()
            .filter(|o| matches!(o, DocOp::Delete(_)))
            .count();
        println!(
            "cycle {n}: decoded={} ops=push:{} delete:{}",
            cycle.decoded.len(),
            pushes,
            deletes
        );
        for m in &cycle.decoded {
            println!("  msg: {m:?}");
        }
        for op in &cycle.ops {
            println!("  op:  {op:?}");
        }
    }
}
```

Remove the duplicate `_DocOp` import — `DocOp` is already imported at the top of the file (`use crate::reconcile::DocOp;`). The `Duration` and `Future` imports should be added to the top-of-file `use` block (move them out of this snippet into the existing imports section so they live next to `std::collections::HashMap`).

Also, `Cycle` is already imported via `use crate::runtime::{App, Cycle, DocSet};`; `Cycle<Msg>` carries `Msg`, which now needs `Debug`. Reading `runtime.rs`: `Cycle` is `pub struct Cycle<Msg> { pub decoded: Vec<Msg>, pub ops: Vec<DocOp>, pub rendered: Vec<RenderedDoc> }` — no bound on `Msg`. We add the bound on `serve`'s own `Msg` parameter only, not on `Cycle`, so this is fine.

- [ ] **Step 2: Add a tiny "archive on ink" test app to the `tests` module**

In `crates/inkapp-core/src/sync.rs`, inside `mod tests { ... }`, alongside the existing `NoCx`/`TestMsg` definitions, add a second test app. Replace the existing `view`, `update`, `build_test_app`, `TestMsg`, and `NoCx` block. Add this new content after the existing `build_test_app` (keep the existing app for the two existing tests):

```rust
// --- A second tiny test app whose component decodes any ink to `Archive(key)`
// and whose update removes that key from a Vec<String> model. This lets us
// exercise the full pull → decode → fold → ops → delete path without depending
// on real app components.

use crate::component::{Component, RenderCx};
use crate::ink::RegionInk;
use crate::manifest::Manifest;

#[derive(Clone, Debug, PartialEq)]
enum ArchiveMsg {
    Archive(String),
}

/// A component owning one named region for a given doc key. Any ink that
/// attributes to "archive-<key>" produces `Archive(<key>)`.
struct ArchiveTile {
    key: String,
}

impl Component for ArchiveTile {
    type Msg = ArchiveMsg;

    fn render(&self, _cx: &mut RenderCx) -> String {
        // A full-width fixed-height region labelled with this doc's archive name.
        let region = format!("archive-{}", self.key);
        format!(
            "#block(width: 100%, height: 60pt, [#region(\"{region}\")[archive]])\n\n"
        )
    }

    fn decode(&self, ink: &[RegionInk], _manifest: &Manifest) -> Vec<ArchiveMsg> {
        let want = format!("archive-{}", self.key);
        if ink
            .iter()
            .any(|r| r.region == want && !r.strokes.is_empty())
        {
            vec![ArchiveMsg::Archive(self.key.clone())]
        } else {
            vec![]
        }
    }
}

#[derive(Default, Clone)]
struct ArchiveModel(Vec<String>);

fn archive_view(m: &ArchiveModel, _cx: &NoCx) -> Documents<ArchiveMsg> {
    Documents(
        m.0.iter()
            .map(|k| {
                let key = k.clone();
                Document::keyed(
                    &key,
                    crate::flow![ArchiveTile { key: key.clone() }],
                )
            })
            .collect(),
    )
}

fn archive_update(msg: ArchiveMsg, m: &mut ArchiveModel, _cx: &NoCx) {
    match msg {
        ArchiveMsg::Archive(k) => m.0.retain(|x| x != &k),
    }
}

fn build_archive_app() -> App<ArchiveModel, ArchiveMsg, NoCx> {
    app(ArchiveModel(vec!["doc-a".into()]))
        .connector(NoCx)
        .update(archive_update as fn(ArchiveMsg, &mut ArchiveModel, &NoCx))
        .view(archive_view as fn(&ArchiveModel, &NoCx) -> Documents<ArchiveMsg>)
        .key(Key::from_bytes([9u8; 32]))
        .build()
}
```

Note on `flow!`: `crate::flow![ArchiveTile { ... }]` is the existing macro used by the first test app — same form.

- [ ] **Step 3: Add helper to mint scripted ink across a region**

Still in `mod tests`, add a helper to build a `HashMap<String, Vec<Vec<Stroke>>>` containing one stroke in the centre of page 1 of the named doc. We don't know the exact rect (Typst lays it out), so we just use a stroke at the page's centre using a generous coordinate that should land inside the 60pt block. The page geometry default is 420×560pt with 16pt margin; the `block(width: 100%, height: 60pt)` will sit at the top of the content area. We aim a stroke at roughly `(210, 530)` (PDF space, y-up) which is inside the first block.

```rust
use crate::geometry::PdfPoint;
use crate::ink::Stroke;

fn stroke_at(x: f64, y: f64) -> Stroke {
    Stroke {
        points: vec![PdfPoint { x, y }],
        highlighter: false,
    }
}

fn ink_for(key: &str, stroke: Stroke) -> HashMap<String, Vec<Vec<Stroke>>> {
    let mut m = HashMap::new();
    m.insert(key.to_string(), vec![vec![stroke]]);
    m
}
```

- [ ] **Step 4: Write the three new tests**

After the existing two tests in `mod tests`, add:

```rust
#[tokio::test]
async fn serve_publishes_before_first_pull() {
    let mut application = build_archive_app();
    let mut set = DocSet::default();
    let t = FakeTransport::default();
    // Shutdown is immediately ready — the loop body must not run, but the
    // initial publish must complete.
    serve(
        &mut application,
        &mut set,
        &t,
        Duration::from_millis(1),
        std::future::ready(()),
    )
    .await
    .unwrap();
    assert!(
        !t.pushed.lock().unwrap().is_empty(),
        "initial publish must push before any pull"
    );
    assert_eq!(
        *t.pulls_done.lock().unwrap(),
        0,
        "no pull before the first sleep elapses"
    );
}

#[tokio::test]
async fn sync_once_archives_doc_on_ink() {
    let mut application = build_archive_app();
    let mut set = DocSet::default();
    // Need to publish once so the manifest is in `set` for ink attribution.
    let t = FakeTransport::default();
    publish(&mut application, &mut set, &t).await.unwrap();
    // Queue ink landing inside the archive region of doc-a.
    let scripted = ink_for("doc-a", stroke_at(210.0, 530.0));
    let t = FakeTransport::with_pulls(vec![scripted]);
    let cycle = sync_once(&mut application, &mut set, &t).await.unwrap();
    assert_eq!(
        cycle.decoded,
        vec![ArchiveMsg::Archive("doc-a".into())],
        "ink in archive region decodes to Archive(doc-a)"
    );
    assert!(
        cycle
            .ops
            .iter()
            .any(|o| matches!(o, DocOp::Delete(k) if k.0 == "doc-a")),
        "removing doc-a from model yields a Delete op"
    );
    assert_eq!(
        t.deleted.lock().unwrap().as_slice(),
        &["doc-a".to_string()],
        "transport saw the delete"
    );
}

#[tokio::test]
async fn serve_two_cycles_decode_then_quiet() {
    use tokio::sync::Notify;
    let mut application = build_archive_app();
    let mut set = DocSet::default();
    // Two scripted pulls: cycle 1 = ink, cycle 2 = empty.
    let scripted = ink_for("doc-a", stroke_at(210.0, 530.0));
    let t = FakeTransport::with_pulls(vec![scripted, HashMap::new()]);
    let notify = std::sync::Arc::new(Notify::new());
    let shutdown_notify = notify.clone();
    let shutdown = async move { shutdown_notify.notified().await };
    // After both cycles have run, fire the notify from a spawned task.
    let t_ref = &t;
    let runner = async {
        // Poll until two cycles done, then notify.
        let poller = {
            let notify = notify.clone();
            async move {
                loop {
                    if *t_ref.pulls_done.lock().unwrap() >= 2 {
                        notify.notify_one();
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }
        };
        let _ = tokio::join!(
            serve(
                &mut application,
                &mut set,
                t_ref,
                Duration::from_millis(1),
                shutdown,
            ),
            poller,
        );
    };
    runner.await;
    assert_eq!(
        t.deleted.lock().unwrap().as_slice(),
        &["doc-a".to_string()],
        "exactly one delete across the run"
    );
}
```

- [ ] **Step 5: Run the sync tests, expect failures, then green**

```bash
nix develop -c cargo test -p inkapp-core sync::tests
```

Expected: 5 tests, all pass. If `sync_once_archives_doc_on_ink` fails because the stroke lands outside the region, increase block height in `ArchiveTile::render` (e.g. `height: 200pt`) and re-aim the stroke at the top of the page (`stroke_at(210.0, 540.0)`). The `region` macro from `typst/region.typ` is what attributes ink — verify the rendered region by running with `--nocapture` if needed.

- [ ] **Step 6: Workspace tests + clippy + fmt**

```bash
nix develop -c cargo fmt
nix develop -c cargo test --workspace
nix develop -c cargo clippy --all-targets -- -D warnings
```

- [ ] **Step 7: Commit**

```bash
git add crates/inkapp-core/src/sync.rs
git commit -m "inkapp-core: add serve loop with publish-then-cycle TDD coverage"
```

---

### Task 4: Re-export `serve` from the `inkapp` facade

**Goal:** Add a thin `inkapp::serve` wrapper next to `inkapp::publish` / `inkapp::sync_once` and re-export it from the crate root.

**Files:**
- Modify: `crates/inkapp/src/deploy.rs:36-42` (add a new function after `sync_once`).
- Modify: `crates/inkapp/src/lib.rs:23-24` (extend the existing `pub use deploy::{...}` line).

**Acceptance Criteria:**
- [ ] `inkapp::serve(app, transport, interval, shutdown)` exists.
- [ ] `nix develop -c cargo test -p inkapp` passes.

**Verify:** `nix develop -c cargo test --workspace`.

**Steps:**

- [ ] **Step 1: Add the facade function in `deploy.rs`**

Append after `sync_once`:

```rust
use std::future::Future;
use std::time::Duration;

/// Publish, then loop: every `interval` run one `sync_once`. Returns when
/// `shutdown` resolves.
pub async fn serve<M, Msg, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    transport: &dyn DeviceTransport,
    interval: Duration,
    shutdown: impl Future<Output = ()>,
) -> Result<()>
where
    Msg: Clone + std::fmt::Debug,
{
    let mut set = DocSet::default();
    sync::serve(app, &mut set, transport, interval, shutdown).await
}
```

(Move `use std::future::Future;` and `use std::time::Duration;` into the existing top-of-file `use` block.)

- [ ] **Step 2: Re-export from `lib.rs`**

In `crates/inkapp/src/lib.rs`, change the existing line:

```rust
pub use deploy::{publish, resolve_transport, sync_once};
```

to:

```rust
pub use deploy::{publish, resolve_transport, serve, sync_once};
```

(Single-line edit — sibling worktrees may touch this same line; keep additions to one line to make merges trivial.)

- [ ] **Step 3: Test, fmt, clippy**

```bash
nix develop -c cargo fmt
nix develop -c cargo test --workspace
nix develop -c cargo clippy --all-targets -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add crates/inkapp/src/deploy.rs crates/inkapp/src/lib.rs
git commit -m "inkapp: re-export serve loop from the facade"
```

---

### Task 5: Wire `run` and `sync` subcommands into reading-queue

**Goal:** Add `Run { interval: Option<u64> }` and `Sync` subcommands. Default (no subcommand) preserves today's publish-once behaviour. `Run` plumbs `tokio::signal::ctrl_c()` into `serve`.

**Files:**
- Modify: `apps/reading-queue/src/main.rs` (whole file rewrite — small).

**Acceptance Criteria:**
- [ ] `reading-queue` (no subcommand) still publishes once and exits.
- [ ] `reading-queue sync` runs one `sync_once` and prints the cycle summary.
- [ ] `reading-queue run` publishes then loops; Ctrl-C exits cleanly.
- [ ] `reading-queue run --interval 5` overrides the configured interval.
- [ ] `reading-queue config ...` (existing) still works.
- [ ] `nix develop -c cargo build -p reading-queue` succeeds.

**Verify:** `nix develop -c cargo test --workspace && nix develop -c cargo build -p reading-queue`.

**Steps:**

- [ ] **Step 1: Rewrite `apps/reading-queue/src/main.rs`**

Replace the file with:

```rust
//! Assemble and run the reading-queue app from configuration. Subcommands:
//! `config` (config CLI), `sync` (one-shot sync_once), `run` (publish + serve
//! loop). With no subcommand, performs a one-shot publish (today's behaviour).

use std::time::Duration;

use clap::{Parser, Subcommand};
use inkapp::{app, cli, ConfigStore, DeviceConfig, SecretStore};
use inkapp_config::store::select_instance;
use reading_queue::{update, view, App, AppConfig, Connectors};

#[derive(Parser)]
#[command(name = "reading-queue")]
struct Cli {
    /// Config instance to run (default: $INKAPP_INSTANCE or "default").
    #[arg(long, global = true)]
    instance: Option<String>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Configuration management (instances, secrets, connectors).
    #[command(subcommand)]
    Config(cli::ConfigCmd),
    /// Publish the document set, then loop sync_once forever (Ctrl-C exits).
    Run {
        /// Override the configured `sync_interval_secs` for this run.
        #[arg(long)]
        interval: Option<u64>,
    },
    /// One-shot pull + fold + push.
    Sync,
}

#[tokio::main]
async fn main() {
    let args = Cli::parse();
    let cfg_path = ConfigStore::default_path().expect("config path");

    // `config` subcommand: run the config CLI and exit before any wiring.
    if let Some(Cmd::Config(cmd)) = args.cmd {
        let code = cli::run(cmd, cfg_path).expect("config command");
        std::process::exit(code);
    }

    let instance = select_instance(args.instance.as_deref());
    let store = ConfigStore::open(&cfg_path).expect("open config");
    let app_cfg: AppConfig = store.resolve(&instance).expect("resolve app config");
    let page: inkapp_core::geometry::PageConfig =
        store.resolve(&instance).expect("resolve page config");
    let device: DeviceConfig = store.resolve(&instance).expect("resolve device config");

    let mut secrets = SecretStore::open_default().expect("open secrets");
    let key = secrets.user_key().expect("user key");

    let cache_dir = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("inkapp")
        .join(format!("reading-queue-{instance}"));

    let connectors = Connectors::from_config(&store, &app_cfg, &secrets, cache_dir)
        .await
        .expect("wire connectors from config");

    let mut application = app(App)
        .connector(connectors)
        .update(update)
        .view(view)
        .key(key)
        .page(page.into())
        .build();

    let transport = inkapp::resolve_transport(&device.backend, app_cfg.device_folder.clone())
        .expect("resolve device transport");

    match args.cmd {
        Some(Cmd::Config(_)) => unreachable!("handled above"),
        Some(Cmd::Sync) => {
            let cycle = inkapp::sync_once(&mut application, transport.as_ref())
                .await
                .expect("sync_once");
            println!(
                "reading-queue[{instance}]: synced {} msg(s), {} op(s)",
                cycle.decoded.len(),
                cycle.ops.len()
            );
        }
        Some(Cmd::Run { interval }) => {
            let secs = interval.unwrap_or(device.sync_interval_secs);
            println!(
                "reading-queue[{instance}]: serving every {secs}s on {} ({})",
                app_cfg.device_folder, device.backend
            );
            let shutdown = async {
                let _ = tokio::signal::ctrl_c().await;
            };
            inkapp::serve(
                &mut application,
                transport.as_ref(),
                Duration::from_secs(secs),
                shutdown,
            )
            .await
            .expect("serve loop");
        }
        None => {
            inkapp::publish(&mut application, transport.as_ref())
                .await
                .expect("publish to device");
            println!(
                "reading-queue[{instance}]: published to {} ({})",
                app_cfg.device_folder, device.backend
            );
        }
    }
}
```

- [ ] **Step 2: Build and test**

```bash
nix develop -c cargo fmt
nix develop -c cargo build -p reading-queue
nix develop -c cargo test --workspace
nix develop -c cargo clippy --all-targets -- -D warnings
```

- [ ] **Step 3: Smoke-test the CLI parser without hardware**

```bash
nix develop -c cargo run -p reading-queue -- --help
nix develop -c cargo run -p reading-queue -- run --help
nix develop -c cargo run -p reading-queue -- sync --help
```

Expected: `--help` lists the three subcommands (`config`, `run`, `sync`); `run --help` lists `--interval`. (Do not run `run` itself — it would attempt to talk to the device.)

- [ ] **Step 4: Commit**

```bash
git add apps/reading-queue/src/main.rs
git commit -m "reading-queue: add run/sync subcommands; default stays publish-once"
```

---

### Task 6: Reconcile `docs/appdx.md`

**Goal:** Mark the publish / sync_once / serve trio as built and describe the loop as the final step. The spec's definition of done requires this.

**Files:**
- Modify: `docs/appdx.md`

**Acceptance Criteria:**
- [ ] `appdx.md` has a section (or updated existing section) describing `serve` as the loop the device round-trip rides on.
- [ ] Any "TODO/unbuilt" marker on publish/sync_once is updated.
- [ ] The change passes `make fmt-check` (docs are not formatted by cargo, so this is a no-op — verify nothing else broke).

**Verify:** `nix develop -c cargo test --workspace` (sanity) and a manual read of the section.

**Steps:**

- [ ] **Step 1: Locate the relevant section**

```bash
nix develop -c grep -n "sync_once\|publish\|serve\|deploy" docs/appdx.md | head -50
```

Find the runtime / deployment section where `publish` and `sync_once` are described.

- [ ] **Step 2: Edit `appdx.md`**

In the section that lists the deployment primitives, add a paragraph (or sub-bullet, matching the file's existing style) along these lines — adapt wording to fit the doc's voice:

> **`serve(app, transport, interval)`** — built. The loop the device round-trip rides on. Calls `publish` once so the device has documents to read, then every `interval` runs one `sync_once`: pull ink → decode pre-fold → fold messages → re-render → reconcile → push/delete. Honours `tokio::signal::ctrl_c()`. Apps invoke it via the `run` subcommand on their binary; one-shot diagnostics use `sync`.

If `publish` / `sync_once` are marked unbuilt, change them to built.

- [ ] **Step 3: Verify**

```bash
nix develop -c cargo test --workspace
```

(Docs change is non-code; this just confirms nothing else regressed.)

- [ ] **Step 4: Commit**

```bash
git add docs/appdx.md
git commit -m "appdx: record the serve loop as the final deploy step"
```

---

## Self-Review

**Spec coverage:**
- `serve` signature with shutdown parameter → Task 3.
- Publish-before-first-pull → Task 3 step 4 (`serve_publishes_before_first_pull`).
- Logging format → Task 3 step 1.
- `DeviceConfig.sync_interval_secs` → Task 1.
- `Cmd` enum with `Run`/`Sync`/`Config` and default-preserved publish → Task 5.
- `inkapp::serve` re-export → Task 4.
- `FakeTransport` extension + 3 tests → Tasks 2 + 3.
- `appdx.md` reconciliation → Task 6.

**Placeholder scan:** no `TBD`/`TODO`/"similar to" — all code shown.

**Type consistency:** `FakeTransport` field renames (`pulled` → `pulls_done`) are applied consistently across Task 2 (rename + existing-test update) and Task 3 (test usage). `Msg: Clone + Debug` bound on `serve` is set in both `inkapp_core::sync::serve` and the `inkapp::serve` facade.
