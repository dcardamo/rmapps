# Local Preview & Doctor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add two reusable framework facades (`preview`, `doctor`) to `crates/inkapp` and mount them as subcommands in `reading-queue` and `agenda`, so apps can be eyeballed and preflight-checked without device round-trips.

**Architecture:** Two new modules in the `inkapp` facade — `preview` (render `App::render` output to PDFs on disk; optionally serve via an axum router on `0.0.0.0:4747`) and `doctor` (a `Checklist` builder running per-check inspections of secrets, config, connector liveness, and a render probe; prints a plaintext report and returns an exit code). App binaries gain `Preview` and `Doctor` variants on their `Cli` enum.

**Tech Stack:** Rust, axum 0.8 (workspace dep via rm-cloud feature; promoted to direct dep in `inkapp`), tower (dev-dep for handler unit tests), clap, tokio, tempfile, gethostname.

---

## File Structure

**Create:**
- `crates/inkapp/src/preview.rs` — `PreviewArgs`, `RenderedEntry`, `render_to_dir`, `make_router`, `run`.
- `crates/inkapp/src/doctor.rs` — `Status`, `Outcome`, `Check` (internal trait), `Checklist`, `run`.

**Modify:**
- `crates/inkapp/Cargo.toml` — add `axum = "0.8"`, `tokio = { workspace+features }`, `gethostname = "0.5"` as direct deps; add `tower = "0.5"` as dev-dep.
- `crates/inkapp/src/lib.rs` — add `pub mod preview; pub mod doctor; pub use cli::PreviewArgs;` (additive).
- `crates/inkapp-config/src/cli.rs` (or `inkapp` itself) — host `cli::PreviewArgs` (a `clap::Args` struct). Spec says under `inkapp::cli`; we'll add a thin re-export module `crates/inkapp/src/cli.rs` to avoid touching `inkapp-config` for an unrelated concern.
- `apps/reading-queue/src/main.rs` — `Cli` gains `Preview(inkapp::cli::PreviewArgs)` and `Doctor` variants; main dispatches.
- `apps/agenda/src/main.rs` — same shape.
- `docs/appdx.md` — append "Local preview & doctor" subsection marking both built.

**Test:** Inline `#[cfg(test)] mod tests` in `preview.rs` and `doctor.rs`. No integration test file needed.

---

## Task 0: Add deps and module scaffolding

**Goal:** Wire the new deps and create empty module files so the rest of the plan compiles task-by-task.

**Files:**
- Modify: `crates/inkapp/Cargo.toml`
- Create: `crates/inkapp/src/preview.rs`
- Create: `crates/inkapp/src/doctor.rs`
- Create: `crates/inkapp/src/cli.rs`
- Modify: `crates/inkapp/src/lib.rs`

**Acceptance Criteria:**
- [ ] `nix develop -c cargo build -p inkapp` succeeds.
- [ ] `nix develop -c cargo test -p inkapp` runs (existing tests still pass; new modules empty).

**Verify:** `nix develop -c cargo build -p inkapp` → exits 0

**Steps:**

- [ ] **Step 1: Add deps to `crates/inkapp/Cargo.toml`**

In `[dependencies]`, append:
```toml
axum = "0.8"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net"] }
gethostname = "0.5"
clap = { version = "4", features = ["derive"] }
```

In `[dev-dependencies]`, append:
```toml
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
inkapp-readwise-reader = { path = "../inkapp-readwise-reader" }
inkapp-content = { path = "../inkapp-content" }
```

(Cargo.lock will be updated by `cargo build` — do NOT stage it; the convention is a separate dependency-bump commit.)

- [ ] **Step 2: Create the three module files**

`crates/inkapp/src/preview.rs`:
```rust
//! Local preview: render the app's document set to PDFs and optionally serve
//! them over HTTP for browser viewing (Tailscale-friendly: binds 0.0.0.0).
```

`crates/inkapp/src/doctor.rs`:
```rust
//! Preflight checklist for an inkapp app: secrets, config, connector liveness,
//! and a render probe. Reports plaintext rows and returns an exit code.
```

`crates/inkapp/src/cli.rs`:
```rust
//! Clap argument groups exposed by the facade so app binaries can mount
//! framework subcommands directly.

use std::path::PathBuf;

#[derive(clap::Args, Debug, Clone)]
pub struct PreviewArgs {
    /// Directory to write `<key>.pdf` files into.
    #[arg(long, default_value = "./preview")]
    pub out: PathBuf,
    /// Also bind an HTTP server on 0.0.0.0 and serve the rendered PDFs.
    #[arg(long, default_value_t = false)]
    pub serve: bool,
    /// Port for --serve (default 4747).
    #[arg(long, default_value_t = 4747)]
    pub port: u16,
}
```

- [ ] **Step 3: Wire modules into `crates/inkapp/src/lib.rs`**

Append (after the existing `mod deploy;` line and its `pub use`):
```rust
pub mod cli;
pub mod doctor;
pub mod preview;
```

- [ ] **Step 4: Verify build**

Run: `nix develop -c cargo build -p inkapp`
Expected: exits 0; new modules empty so no warnings about unused items.

- [ ] **Step 5: Commit**

```bash
git add crates/inkapp/Cargo.toml crates/inkapp/src/preview.rs crates/inkapp/src/doctor.rs crates/inkapp/src/cli.rs crates/inkapp/src/lib.rs
git commit -m "inkapp: scaffold preview/doctor modules and cli::PreviewArgs"
```

(Do NOT stage `Cargo.lock`.)

---

## Task 1: `preview::render_to_dir` + `RenderedEntry`

**Goal:** Render the app's docset and write each PDF to `<out>/<key>.pdf`, returning a per-doc summary.

**Files:**
- Modify: `crates/inkapp/src/preview.rs`

**Acceptance Criteria:**
- [ ] `render_to_dir` creates the `out` dir if missing.
- [ ] Each `<key>.pdf` is written with the bytes from `App::render`.
- [ ] Returned `Vec<RenderedEntry>` is in `view()` order with `key`, `path`, `size_bytes`, `page_count` populated.
- [ ] Each PDF starts with the bytes `%PDF`.

**Verify:** `nix develop -c cargo test -p inkapp preview::tests::render_to_dir -- --nocapture` → all pass

**Steps:**

- [ ] **Step 1: Write the failing test**

Append to `crates/inkapp/src/preview.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use inkapp_core::crypto::Key;
    use inkapp_core::runtime::app;
    use reading_queue_test_fixture::{cassette_app};
    // We'll define cassette_app inline in this crate — see Step 2.

    #[tokio::test]
    async fn render_to_dir_writes_nonempty_pdfs_starting_with_magic() {
        let tmp = tempfile::tempdir().unwrap();
        let mut application = crate::preview::tests::fixture::cassette_app();
        let entries = render_to_dir(&mut application, tmp.path()).await.unwrap();
        assert!(!entries.is_empty(), "cassette must yield at least one doc");
        for e in &entries {
            assert!(e.path.exists(), "{} must exist on disk", e.path.display());
            let bytes = std::fs::read(&e.path).unwrap();
            assert!(!bytes.is_empty(), "{} must be non-empty", e.key);
            assert!(bytes.starts_with(b"%PDF"), "{} must start with %PDF", e.key);
            assert_eq!(bytes.len(), e.size_bytes, "size_bytes matches file");
            assert!(e.page_count >= 1, "{} must have at least one page", e.key);
        }
    }
}
```

- [ ] **Step 2: Add the test fixture sub-module**

Inside the `tests` module (above the test), add:
```rust
    mod fixture {
        use inkapp_core::crypto::Key;
        use inkapp_core::runtime::{app, App};
        use inkapp_readwise_reader::Readwise;
        use reading_queue::{update, view, App as RqApp, Connectors};
        use std::sync::Arc;

        pub fn cassette_app() -> App<RqApp, reading_queue::Msg, Connectors> {
            let connectors = Connectors::from_arc(Arc::new(Readwise::from_cassette()));
            app(RqApp)
                .connector(connectors)
                .update(update)
                .view(view)
                .key(Key::from_bytes([7u8; 32]))
                .build()
        }
    }
```

Add to `crates/inkapp/Cargo.toml` `[dev-dependencies]`:
```toml
reading-queue = { path = "../../apps/reading-queue" }
```

(reading-queue is a workspace member with a `lib.rs`, so it's depable as a dev fixture.)

- [ ] **Step 3: Verify the test fails (no `render_to_dir` yet)**

Run: `nix develop -c cargo test -p inkapp preview::tests::render_to_dir`
Expected: COMPILE ERROR — `render_to_dir` not defined.

- [ ] **Step 4: Implement `RenderedEntry` and `render_to_dir`**

At the top of `crates/inkapp/src/preview.rs`:
```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use inkapp_core::connector::ConnectorSet;
use inkapp_core::error::{Error, Result};
use inkapp_core::runtime::{App, DocSet, RenderedDoc};

pub use crate::cli::PreviewArgs;

#[derive(Debug, Clone)]
pub struct RenderedEntry {
    pub key: String,
    pub path: PathBuf,
    pub size_bytes: usize,
    pub page_count: usize,
}

/// Render the app's document set and write each to `<out>/<key>.pdf`.
pub async fn render_to_dir<M, Msg, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    out: &Path,
) -> Result<Vec<RenderedEntry>> {
    std::fs::create_dir_all(out).map_err(|e| Error::Other(format!("preview mkdir: {e}")))?;
    let mut set = DocSet::default();
    let rendered = app.render(&mut set).await?;
    let mut entries = Vec::with_capacity(rendered.len());
    for rd in &rendered {
        let path = out.join(format!("{}.pdf", sanitize_key(&rd.key.0)));
        std::fs::write(&path, &rd.pdf).map_err(|e| Error::Other(format!("preview write: {e}")))?;
        entries.push(RenderedEntry {
            key: rd.key.0.clone(),
            path,
            size_bytes: rd.pdf.len(),
            page_count: rd.page_count,
        });
    }
    Ok(entries)
}

/// Replace path separators in a doc key so it can be a filename.
fn sanitize_key(k: &str) -> String {
    k.replace(['/', '\\'], "_")
}

/// Also returns the in-memory PDF set keyed by doc key (used by `run`).
pub(crate) async fn render_to_dir_and_map<M, Msg, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    out: &Path,
) -> Result<(Vec<RenderedEntry>, HashMap<String, Vec<u8>>)> {
    std::fs::create_dir_all(out).map_err(|e| Error::Other(format!("preview mkdir: {e}")))?;
    let mut set = DocSet::default();
    let rendered = app.render(&mut set).await?;
    let mut entries = Vec::with_capacity(rendered.len());
    let mut pdfs: HashMap<String, Vec<u8>> = HashMap::new();
    for rd in rendered {
        let path = out.join(format!("{}.pdf", sanitize_key(&rd.key.0)));
        std::fs::write(&path, &rd.pdf).map_err(|e| Error::Other(format!("preview write: {e}")))?;
        entries.push(RenderedEntry {
            key: rd.key.0.clone(),
            path,
            size_bytes: rd.pdf.len(),
            page_count: rd.page_count,
        });
        pdfs.insert(rd.key.0, rd.pdf);
    }
    Ok((entries, pdfs))
}
```

If `inkapp_core::error::Error` lacks an `Other` variant, replace `Error::Other(...)` with whatever generic variant exists (likely `Error::Io` or `Error::Config`); check with `grep -n "pub enum Error" crates/inkapp-core/src/error.rs` and adjust the two call sites.

- [ ] **Step 5: Run the test — must pass**

Run: `nix develop -c cargo test -p inkapp preview::tests::render_to_dir`
Expected: PASS.

- [ ] **Step 6: Clear the native task list (commit hook will block otherwise) and commit**

```bash
git add crates/inkapp/Cargo.toml crates/inkapp/src/preview.rs
git commit -m "inkapp: preview::render_to_dir writes RenderedEntry PDFs"
```

---

## Task 2: `preview::make_router`

**Goal:** Build an `axum::Router` that lists and serves the in-memory PDFs; testable without binding a socket.

**Files:**
- Modify: `crates/inkapp/src/preview.rs`

**Acceptance Criteria:**
- [ ] `GET /` returns HTML containing each key as `<a href="/{key}.pdf">`.
- [ ] `GET /<key>.pdf` returns 200 with `Content-Type: application/pdf` and exact bytes.
- [ ] `GET /missing.pdf` returns 404.

**Verify:** `nix develop -c cargo test -p inkapp preview::tests::router` → all pass

**Steps:**

- [ ] **Step 1: Write the failing tests**

Append inside the existing `mod tests`:
```rust
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn fixture_pdfs() -> std::collections::HashMap<String, Vec<u8>> {
        let mut m = std::collections::HashMap::new();
        m.insert("alpha".to_string(), b"%PDF-1.4\n...alpha...".to_vec());
        m.insert("beta".to_string(), b"%PDF-1.4\n...beta...".to_vec());
        m
    }

    #[tokio::test]
    async fn router_lists_keys_at_root() {
        let router = make_router(fixture_pdfs());
        let resp = router
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains(r#"href="/alpha.pdf""#), "index lists alpha: {body}");
        assert!(body.contains(r#"href="/beta.pdf""#), "index lists beta: {body}");
    }

    #[tokio::test]
    async fn router_serves_known_pdf_with_correct_content_type_and_bytes() {
        let pdfs = fixture_pdfs();
        let expected = pdfs.get("alpha").cloned().unwrap();
        let router = make_router(pdfs);
        let resp = router
            .oneshot(Request::builder().uri("/alpha.pdf").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/pdf",
        );
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body.as_ref(), expected.as_slice());
    }

    #[tokio::test]
    async fn router_returns_404_for_unknown_key() {
        let router = make_router(fixture_pdfs());
        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/missing.pdf")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
```

- [ ] **Step 2: Verify the tests fail (no `make_router` yet)**

Run: `nix develop -c cargo test -p inkapp preview::tests::router`
Expected: COMPILE ERROR — `make_router` not defined.

- [ ] **Step 3: Implement `make_router`**

Append to `crates/inkapp/src/preview.rs`:
```rust
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};

#[derive(Clone)]
struct PdfState {
    pdfs: Arc<HashMap<String, Vec<u8>>>,
}

/// Build a router that lists and serves an in-memory PDF set.
/// Pure: takes the map by value, returns a configured Router.
pub fn make_router(pdfs: HashMap<String, Vec<u8>>) -> Router {
    let state = PdfState { pdfs: Arc::new(pdfs) };
    Router::new()
        .route("/", get(index))
        .route("/{key}.pdf", get(serve_pdf))
        .with_state(state)
}

async fn index(State(s): State<PdfState>) -> Html<String> {
    let mut keys: Vec<&String> = s.pdfs.keys().collect();
    keys.sort();
    let mut html = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>inkapp preview</title>\
         <style>body{font-family:sans-serif;max-width:60em;margin:2em auto;padding:0 1em}\
         li{margin:.4em 0}.meta{color:#888;font-size:.9em}</style></head><body>\
         <h1>inkapp preview</h1><ul>",
    );
    for k in keys {
        let bytes = s.pdfs[k].len();
        html.push_str(&format!(
            "<li><a href=\"/{k}.pdf\">{k}</a> <span class=\"meta\">({bytes} bytes)</span></li>"
        ));
    }
    html.push_str("</ul></body></html>");
    Html(html)
}

async fn serve_pdf(
    State(s): State<PdfState>,
    AxumPath(key): AxumPath<String>,
) -> Response {
    match s.pdfs.get(&key) {
        Some(bytes) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/pdf")],
            bytes.clone(),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}
```

- [ ] **Step 4: Run the tests — must pass**

Run: `nix develop -c cargo test -p inkapp preview::tests::router`
Expected: 3 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/inkapp/src/preview.rs
git commit -m "inkapp: preview::make_router lists and serves in-memory PDFs"
```

---

## Task 3: `preview::run` (serve + hostname URL)

**Goal:** Top-level `run(app, args)` that renders to disk, optionally binds the router on `0.0.0.0:<port>`, and prints `http://<hostname>:<port>`.

**Files:**
- Modify: `crates/inkapp/src/preview.rs`

**Acceptance Criteria:**
- [ ] `run(&mut app, args { serve: false, ... })` writes PDFs and returns `Ok(0)`.
- [ ] `serve: true` branch binds `0.0.0.0:<port>` via `tokio::net::TcpListener::bind`.
- [ ] URL printed uses `gethostname::gethostname()` (lossy-to-string), never `localhost`/`127.0.0.1`.

**Verify:** `nix develop -c cargo test -p inkapp preview::tests::run_no_serve` → passes; manual smoke: `nix develop -c cargo run -p reading-queue -- preview --out /tmp/rq --serve --port 4748` shows `http://neptune:4748` (deferred to Task 8 verification).

**Steps:**

- [ ] **Step 1: Write the failing test (no-serve branch only)**

Append inside `mod tests`:
```rust
    #[tokio::test]
    async fn run_without_serve_writes_pdfs_and_returns_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let mut application = fixture::cassette_app();
        let args = PreviewArgs {
            out: tmp.path().to_path_buf(),
            serve: false,
            port: 4747,
        };
        let code = run(&mut application, args).await.unwrap();
        assert_eq!(code, 0);
        // At least one PDF exists.
        let count = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter(|e| e.as_ref().unwrap().path().extension().map_or(false, |x| x == "pdf"))
            .count();
        assert!(count >= 1, "expected at least one .pdf in {}", tmp.path().display());
    }
```

- [ ] **Step 2: Verify the test fails**

Run: `nix develop -c cargo test -p inkapp preview::tests::run_without_serve`
Expected: COMPILE ERROR — `run` not defined.

- [ ] **Step 3: Implement `run`**

Append to `crates/inkapp/src/preview.rs`:
```rust
/// Render to `args.out`; if `args.serve`, bind 0.0.0.0:port and serve the same
/// PDFs over HTTP, printing a Tailscale-reachable URL using the local hostname.
pub async fn run<M, Msg, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    args: PreviewArgs,
) -> Result<i32> {
    let (entries, pdfs) = render_to_dir_and_map(app, &args.out).await?;
    println!("preview: wrote {} PDF(s) to {}", entries.len(), args.out.display());
    for e in &entries {
        println!("  {}  ({} pages, {} bytes)  -> {}", e.key, e.page_count, e.size_bytes, e.path.display());
    }
    if !args.serve {
        return Ok(0);
    }
    let host = gethostname::gethostname().to_string_lossy().into_owned();
    let addr: std::net::SocketAddr = ([0, 0, 0, 0], args.port).into();
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| Error::Other(format!("preview bind {addr}: {e}")))?;
    println!("preview: serving at http://{host}:{port}", host = host, port = args.port);
    let router = make_router(pdfs);
    axum::serve(listener, router)
        .await
        .map_err(|e| Error::Other(format!("preview serve: {e}")))?;
    Ok(0)
}
```

- [ ] **Step 4: Run the test — must pass**

Run: `nix develop -c cargo test -p inkapp preview::tests::run_without_serve`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/inkapp/src/preview.rs
git commit -m "inkapp: preview::run renders to disk and optionally serves over 0.0.0.0"
```

---

## Task 4: `doctor::Checklist` + secret checks

**Goal:** Foundation of the doctor: `Status`, `Outcome`, `Check` trait, `Checklist::new(secrets_path)`, and the two secret-presence checks (`user_key`, `secret`).

**Files:**
- Modify: `crates/inkapp/src/doctor.rs`

**Acceptance Criteria:**
- [ ] `Checklist::new(path).user_key().secret(Scope::ConnectorCred, "readwise").secret(Scope::DeviceAuth, "remarkable").run().await` returns `1` on an empty store with rows reporting each missing secret by name.
- [ ] Same chain on a seeded store returns `0` with all `Pass`.

**Verify:** `nix develop -c cargo test -p inkapp doctor::tests::secrets` → all pass

**Steps:**

- [ ] **Step 1: Write the failing tests**

Append to `crates/inkapp/src/doctor.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use inkapp_core::secrets::{Scope, SecretStore};

    #[tokio::test]
    async fn secrets_empty_store_fails_each_check() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("secrets.json"); // file does not exist
        let outcomes = Checklist::new(&path)
            .user_key()
            .secret(Scope::ConnectorCred, "readwise")
            .secret(Scope::DeviceAuth, "remarkable")
            .collect()
            .await;
        assert_eq!(outcomes.len(), 3);
        for o in &outcomes {
            assert!(matches!(o.status, Status::Fail), "{} should fail: {:?}", o.name, o.status);
        }
        // Names mention each scope/name so the user can recognize what's missing.
        assert!(outcomes.iter().any(|o| o.name.contains("user key")));
        assert!(outcomes.iter().any(|o| o.name.contains("readwise")));
        assert!(outcomes.iter().any(|o| o.name.contains("remarkable")));
    }

    #[tokio::test]
    async fn secrets_populated_store_passes_each_check() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("secrets.json");
        {
            let mut s = SecretStore::open(&path).unwrap();
            s.set(Scope::UserKey, "default", &[0u8; 32]).unwrap();
            s.set(Scope::ConnectorCred, "readwise", b"tok").unwrap();
            s.set(Scope::DeviceAuth, "remarkable", b"auth").unwrap();
        }
        let outcomes = Checklist::new(&path)
            .user_key()
            .secret(Scope::ConnectorCred, "readwise")
            .secret(Scope::DeviceAuth, "remarkable")
            .collect()
            .await;
        for o in &outcomes {
            assert!(matches!(o.status, Status::Pass), "{} should pass: {:?}", o.name, o);
        }
    }

    #[tokio::test]
    async fn run_returns_exit_codes() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("secrets.json");
        let code_empty = Checklist::new(&path).user_key().run().await;
        assert_eq!(code_empty, 1, "missing user_key => exit 1");

        {
            let mut s = SecretStore::open(&path).unwrap();
            s.set(Scope::UserKey, "default", &[0u8; 32]).unwrap();
        }
        let code_ok = Checklist::new(&path).user_key().run().await;
        assert_eq!(code_ok, 0, "present user_key => exit 0");
    }
}
```

- [ ] **Step 2: Verify the tests fail**

Run: `nix develop -c cargo test -p inkapp doctor::tests`
Expected: COMPILE ERROR — `Checklist`, `Status`, etc. not defined.

- [ ] **Step 3: Implement the foundation + secret checks**

Replace the contents of `crates/inkapp/src/doctor.rs` with:
```rust
//! Preflight checklist for an inkapp app. See
//! docs/superpowers/specs/2026-05-25-local-preview-and-doctor-design.md.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use inkapp_core::secrets::{Scope, SecretStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status { Pass, Fail, Skip }

#[derive(Debug, Clone)]
pub struct Outcome {
    pub name: String,
    pub status: Status,
    pub detail: String,
}

#[async_trait]
trait Check: Send {
    async fn run(&self) -> Outcome;
}

/// Doctor builder. Bind to a secrets-file path once; secret checks inspect it.
pub struct Checklist {
    secrets_path: PathBuf,
    checks: Vec<Box<dyn Check>>,
}

impl Checklist {
    pub fn new(secrets_path: impl Into<PathBuf>) -> Self {
        Self { secrets_path: secrets_path.into(), checks: Vec::new() }
    }

    pub fn user_key(mut self) -> Self {
        self.checks.push(Box::new(SecretCheck {
            path: self.secrets_path.clone(),
            scope: Scope::UserKey,
            name: "default".to_string(),
            label: "user key present".to_string(),
            expect_len: Some(32),
        }));
        self
    }

    pub fn secret(mut self, scope: Scope, name: impl Into<String>) -> Self {
        let name = name.into();
        let label = match scope {
            Scope::ConnectorCred => format!("{} connector token present", name),
            Scope::DeviceAuth => format!("device auth '{}' present", name),
            Scope::UserKey => format!("user key '{}' present", name),
        };
        self.checks.push(Box::new(SecretCheck {
            path: self.secrets_path.clone(),
            scope, name, label, expect_len: None,
        }));
        self
    }

    /// Run every check, returning the outcomes. Used by tests.
    pub async fn collect(self) -> Vec<Outcome> {
        let mut out = Vec::with_capacity(self.checks.len());
        for c in &self.checks {
            out.push(c.run().await);
        }
        out
    }

    /// Run every check, print rows, return exit code (0 if all Pass/Skip; 1 otherwise).
    pub async fn run(self) -> i32 {
        let outcomes = self.collect().await;
        let mut fail = false;
        for o in &outcomes {
            let tag = match o.status {
                Status::Pass => "[PASS]",
                Status::Fail => { fail = true; "[FAIL]" }
                Status::Skip => "[SKIP]",
            };
            if o.detail.is_empty() {
                println!("{tag} {}", o.name);
            } else {
                println!("{tag} {:<42} — {}", o.name, o.detail);
            }
        }
        if fail { 1 } else { 0 }
    }
}

struct SecretCheck {
    path: PathBuf,
    scope: Scope,
    name: String,
    label: String,
    /// If set, the stored value must be exactly this many bytes.
    expect_len: Option<usize>,
}

#[async_trait]
impl Check for SecretCheck {
    async fn run(&self) -> Outcome {
        let store = match SecretStore::open(&self.path) {
            Ok(s) => s,
            Err(e) => return Outcome {
                name: self.label.clone(),
                status: Status::Fail,
                detail: format!("open secrets failed: {e}"),
            },
        };
        match store.get(self.scope, &self.name) {
            Ok(Some(bytes)) => {
                if let Some(n) = self.expect_len {
                    if bytes.len() != n {
                        return Outcome {
                            name: self.label.clone(),
                            status: Status::Fail,
                            detail: format!("stored value is {} bytes, expected {}", bytes.len(), n),
                        };
                    }
                }
                Outcome { name: self.label.clone(), status: Status::Pass, detail: String::new() }
            }
            Ok(None) => Outcome {
                name: self.label.clone(),
                status: Status::Fail,
                detail: format!("{:?} name='{}' not in store", self.scope, self.name),
            },
            Err(e) => Outcome {
                name: self.label.clone(),
                status: Status::Fail,
                detail: format!("get failed: {e}"),
            },
        }
    }
}
```

- [ ] **Step 4: Add `async-trait` dep**

In `crates/inkapp/Cargo.toml` `[dependencies]`:
```toml
async-trait = "0.1"
```

- [ ] **Step 5: Run tests — must pass**

Run: `nix develop -c cargo test -p inkapp doctor::tests`
Expected: 3 tests PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/inkapp/Cargo.toml crates/inkapp/src/doctor.rs
git commit -m "inkapp: doctor Checklist with user_key and secret checks"
```

---

## Task 5: `config_resolves` + `connector_refresh` checks

**Goal:** Two more checks: a `config_resolves::<T>` that resolves a typed config section, and a `connector_refresh` that calls `Connector::refresh` and reports.

**Files:**
- Modify: `crates/inkapp/src/doctor.rs`

**Acceptance Criteria:**
- [ ] `config_resolves::<AppConfig>(&store, "default", "app.reading-queue")` Passes when the section exists and is valid; Fails on invalid; Passes (defaults) when absent (matching `ConfigStore::resolve` semantics).
- [ ] `connector_refresh` calls `c.refresh()` and reports its error; Passes if `refresh` returns `Ok`.

**Verify:** `nix develop -c cargo test -p inkapp doctor::tests` → all pass

**Steps:**

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests`:
```rust
    use inkapp_config::store::ConfigStore;
    use inkapp_readwise_reader::Readwise;
    use std::sync::Arc;

    #[tokio::test]
    async fn config_resolves_passes_on_valid_section() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, "[app.reading-queue.default]\ndevice_folder = \"/RQ\"\n").unwrap();
        let store = ConfigStore::open(&cfg).unwrap();
        let outcomes = Checklist::new(dir.path().join("s.json"))
            .config_resolves::<reading_queue::AppConfig>(&store, "default", "app.reading-queue")
            .collect()
            .await;
        assert_eq!(outcomes.len(), 1);
        assert!(matches!(outcomes[0].status, Status::Pass), "{:?}", outcomes[0]);
    }

    #[tokio::test]
    async fn connector_refresh_passes_for_cassette() {
        let dir = tempfile::tempdir().unwrap();
        let rw: Arc<dyn inkapp_core::connector::Connector> = Arc::new(Readwise::from_cassette());
        let outcomes = Checklist::new(dir.path().join("s.json"))
            .connector_refresh("readwise", rw)
            .collect()
            .await;
        assert_eq!(outcomes.len(), 1);
        assert!(matches!(outcomes[0].status, Status::Pass), "{:?}", outcomes[0]);
    }
```

- [ ] **Step 2: Verify the tests fail**

Run: `nix develop -c cargo test -p inkapp doctor::tests::config_resolves_passes`
Expected: COMPILE ERROR — methods not defined.

- [ ] **Step 3: Implement the two checks**

In `crates/inkapp/src/doctor.rs`, add imports and methods.

At the top, add:
```rust
use std::sync::Arc;

use inkapp_config::store::ConfigStore;
use inkapp_config::Config as ConfigTrait;
use inkapp_core::connector::Connector;
```

Inside `impl Checklist`, add:
```rust
    pub fn config_resolves<T: ConfigTrait + 'static>(
        mut self,
        store: &ConfigStore,
        instance: &str,
        label: &str,
    ) -> Self {
        let res = store.resolve::<T>(instance);
        self.checks.push(Box::new(StaticCheck {
            label: format!("[{}] config resolves", label),
            outcome_status: match res {
                Ok(_) => Status::Pass,
                Err(_) => Status::Fail,
            },
            detail: res.err().map(|e| e.to_string()).unwrap_or_default(),
        }));
        self
    }

    pub fn connector_refresh(mut self, label: &str, c: Arc<dyn Connector>) -> Self {
        self.checks.push(Box::new(ConnectorCheck {
            label: format!("{} connector refresh", label),
            c,
        }));
        self
    }
```

And add the new check types at the bottom of the file:
```rust
struct StaticCheck {
    label: String,
    outcome_status: Status,
    detail: String,
}

#[async_trait]
impl Check for StaticCheck {
    async fn run(&self) -> Outcome {
        Outcome { name: self.label.clone(), status: self.outcome_status.clone(), detail: self.detail.clone() }
    }
}

struct ConnectorCheck {
    label: String,
    c: Arc<dyn Connector>,
}

#[async_trait]
impl Check for ConnectorCheck {
    async fn run(&self) -> Outcome {
        match self.c.refresh().await {
            Ok(()) => Outcome { name: self.label.clone(), status: Status::Pass, detail: String::new() },
            Err(e) => Outcome { name: self.label.clone(), status: Status::Fail, detail: e.to_string() },
        }
    }
}
```

If `Connector::refresh` does not return `Result<(), ConnectorError>` — confirm by reading `crates/inkapp-core/src/connector.rs`. If it returns `Result<(), ConnectorError>` directly the above works; if it returns a different error type, adjust the `e.to_string()` call accordingly.

- [ ] **Step 4: Run tests — must pass**

Run: `nix develop -c cargo test -p inkapp doctor::tests`
Expected: 5 tests PASS (2 new + 3 existing).

- [ ] **Step 5: Commit**

```bash
git add crates/inkapp/src/doctor.rs
git commit -m "inkapp: doctor config_resolves and connector_refresh checks"
```

---

## Task 6: `render_probe` + end-to-end doctor test

**Goal:** A `render_probe` check that runs the app's `view()`, picks the first non-`_banner` doc, renders it, and reports its size/pages. Then an end-to-end populated-store test.

**Files:**
- Modify: `crates/inkapp/src/doctor.rs`

**Acceptance Criteria:**
- [ ] `render_probe(closure_yielding_app)` Passes with a cassette-backed reading-queue app and reports `first content doc 'X' → N pages, M bytes`.
- [ ] If `view()` produces no non-banner docs, the check Fails with "no content docs to render".
- [ ] If user_key cannot be derived (the closure errors), the check Fails with the error text.
- [ ] Full populated-store doctor (`user_key + secrets + config_resolves + connector_refresh + render_probe`) returns 0.

**Verify:** `nix develop -c cargo test -p inkapp doctor::tests` → all pass

**Steps:**

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests`:
```rust
    use inkapp_core::runtime::{app, App as Runtime};
    use inkapp_core::crypto::Key;
    use reading_queue::{update, view, App as RqApp, Connectors};

    fn build_cassette_app() -> inkapp_core::error::Result<Runtime<RqApp, reading_queue::Msg, Connectors>> {
        let connectors = Connectors::from_arc(Arc::new(Readwise::from_cassette()));
        Ok(app(RqApp)
            .connector(connectors)
            .update(update)
            .view(view)
            .key(Key::from_bytes([7u8; 32]))
            .build())
    }

    #[tokio::test]
    async fn render_probe_passes_on_cassette_app() {
        let dir = tempfile::tempdir().unwrap();
        let outcomes = Checklist::new(dir.path().join("s.json"))
            .render_probe(|| async { build_cassette_app() })
            .collect()
            .await;
        assert_eq!(outcomes.len(), 1);
        assert!(matches!(outcomes[0].status, Status::Pass), "{:?}", outcomes[0]);
        assert!(outcomes[0].detail.contains("pages"), "detail mentions pages: {:?}", outcomes[0]);
        assert!(outcomes[0].detail.contains("bytes"), "detail mentions bytes: {:?}", outcomes[0]);
    }

    #[tokio::test]
    async fn doctor_populated_returns_zero_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let secrets_path = dir.path().join("secrets.json");
        {
            let mut s = SecretStore::open(&secrets_path).unwrap();
            s.set(Scope::UserKey, "default", &[0u8; 32]).unwrap();
            s.set(Scope::ConnectorCred, "readwise", b"tok").unwrap();
            s.set(Scope::DeviceAuth, "remarkable", b"auth").unwrap();
        }
        let cfg = dir.path().join("config.toml");
        std::fs::write(
            &cfg,
            "[app.reading-queue.default]\ndevice_folder = \"/RQ\"\n\
             [page]\n\
             [device.default]\nbackend = \"remarkable\"\n",
        ).unwrap();
        let store = ConfigStore::open(&cfg).unwrap();
        let rw: Arc<dyn inkapp_core::connector::Connector> = Arc::new(Readwise::from_cassette());
        let code = Checklist::new(&secrets_path)
            .user_key()
            .secret(Scope::ConnectorCred, "readwise")
            .secret(Scope::DeviceAuth, "remarkable")
            .config_resolves::<reading_queue::AppConfig>(&store, "default", "app.reading-queue")
            .config_resolves::<inkapp_core::geometry::PageConfig>(&store, "default", "page")
            .config_resolves::<inkapp::DeviceConfig>(&store, "default", "device")
            .connector_refresh("readwise", rw)
            .render_probe(|| async { build_cassette_app() })
            .run()
            .await;
        assert_eq!(code, 0);
    }
```

- [ ] **Step 2: Verify the tests fail**

Run: `nix develop -c cargo test -p inkapp doctor::tests::render_probe`
Expected: COMPILE ERROR — `render_probe` not defined.

- [ ] **Step 3: Implement `render_probe`**

In `crates/inkapp/src/doctor.rs`, add to `impl Checklist`:
```rust
    /// Render the app's full document set and report the size+pages of the
    /// first non-`_banner` doc. The closure builds the App asynchronously; any
    /// error becomes a Fail outcome (so missing user_key surfaces here cleanly).
    pub fn render_probe<F, Fut, M, Msg, Cx>(mut self, build: F) -> Self
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<
                Output = inkapp_core::error::Result<inkapp_core::runtime::App<M, Msg, Cx>>,
            > + Send
            + 'static,
        M: Send + 'static,
        Msg: Send + 'static,
        Cx: inkapp_core::connector::ConnectorSet + Send + 'static,
    {
        self.checks.push(Box::new(RenderProbe {
            build: std::sync::Mutex::new(Some(Box::new(move || {
                Box::pin(async move {
                    let mut application = build().await?;
                    let mut set = inkapp_core::runtime::DocSet::default();
                    let rendered = application.render(&mut set).await?;
                    Ok(rendered)
                })
            }))),
        }));
        self
    }
```

Add the supporting types at the bottom:
```rust
use std::future::Future;
use std::pin::Pin;

type RenderFut = Pin<Box<dyn Future<Output = inkapp_core::error::Result<Vec<inkapp_core::runtime::RenderedDoc>>> + Send>>;
type RenderBuilder = Box<dyn FnOnce() -> RenderFut + Send>;

struct RenderProbe {
    build: std::sync::Mutex<Option<RenderBuilder>>,
}

#[async_trait]
impl Check for RenderProbe {
    async fn run(&self) -> Outcome {
        let label = "render probe".to_string();
        let builder = self.build.lock().unwrap().take();
        let Some(b) = builder else {
            return Outcome { name: label, status: Status::Fail, detail: "probe already consumed".into() };
        };
        match b().await {
            Ok(docs) => {
                let probe = docs.iter().find(|d| d.key.0 != "_banner");
                match probe {
                    Some(d) => Outcome {
                        name: label,
                        status: Status::Pass,
                        detail: format!(
                            "first content doc '{}' → {} pages, {} bytes",
                            d.key.0, d.page_count, d.pdf.len()
                        ),
                    },
                    None => Outcome {
                        name: label,
                        status: Status::Fail,
                        detail: "no content docs to render".into(),
                    },
                }
            }
            Err(e) => Outcome { name: label, status: Status::Fail, detail: e.to_string() },
        }
    }
}
```

The `Mutex<Option<...>>` exists because `Check::run(&self)` takes `&self` (so we can store `dyn Check` in a `Vec`), but the builder is `FnOnce`. `.take()` consumes it the first time `run` is called.

- [ ] **Step 4: Run tests — must pass**

Run: `nix develop -c cargo test -p inkapp doctor::tests`
Expected: 7 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/inkapp/src/doctor.rs
git commit -m "inkapp: doctor render_probe + end-to-end populated test"
```

---

## Task 7: Wire `Preview` and `Doctor` into `apps/reading-queue`

**Goal:** Add the two new subcommands to the reading-queue binary, dispatching to `inkapp::preview::run` and a hand-assembled `inkapp::doctor::Checklist`.

**Files:**
- Modify: `apps/reading-queue/src/main.rs`

**Acceptance Criteria:**
- [ ] `reading-queue preview --out /tmp/rq` writes PDFs and exits 0 (manual smoke).
- [ ] `reading-queue doctor` prints the checklist and returns an exit code reflecting secret/config state (manual smoke; no asserts beyond compile).
- [ ] `nix develop -c cargo build -p reading-queue` succeeds.

**Verify:** `nix develop -c cargo build -p reading-queue && nix develop -c cargo test --workspace` → exits 0 throughout.

**Steps:**

- [ ] **Step 1: Replace `apps/reading-queue/src/main.rs`**

```rust
//! Assemble and run the reading-queue app from configuration. Supports
//! `config`, `preview`, and `doctor` subcommands as framework facades.

use clap::{Parser, Subcommand};
use inkapp::{app, cli, ConfigStore, DeviceConfig, SecretStore};
use inkapp_config::store::select_instance;
use reading_queue::{update, view, App, AppConfig, Connectors};
use std::sync::Arc;

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
    /// Manage configuration.
    #[command(subcommand)]
    Config(cli::ConfigCmd),
    /// Render the document set locally for browser preview.
    Preview(inkapp::cli::PreviewArgs),
    /// Run preflight checks (secrets, config, connectors, render).
    Doctor,
}

#[tokio::main]
async fn main() {
    let args = Cli::parse();
    let cfg_path = ConfigStore::default_path().expect("config path");
    let secrets_path = SecretStore::default_path().expect("secrets path");
    let instance = select_instance(args.instance.as_deref());

    match args.cmd {
        Some(Cmd::Config(c)) => {
            let code = cli::run(c, cfg_path).expect("config command");
            std::process::exit(code);
        }
        Some(Cmd::Doctor) => {
            let code = run_doctor(&cfg_path, &secrets_path, &instance).await;
            std::process::exit(code);
        }
        Some(Cmd::Preview(args)) => {
            let mut application = build_app(&cfg_path, &instance).await;
            let code = inkapp::preview::run(&mut application, args).await.expect("preview run");
            std::process::exit(code);
        }
        None => {
            // Default behavior preserved: publish to device.
            let store = ConfigStore::open(&cfg_path).expect("open config");
            let device: DeviceConfig = store.resolve(&instance).expect("resolve device config");
            let app_cfg: AppConfig = store.resolve(&instance).expect("resolve app config");
            let mut application = build_app(&cfg_path, &instance).await;
            let transport = inkapp::resolve_transport(&device.backend, app_cfg.device_folder.clone())
                .expect("resolve device transport");
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

async fn build_app(
    cfg_path: &std::path::Path,
    instance: &str,
) -> inkapp_core::runtime::App<App, reading_queue::Msg, Connectors> {
    let store = ConfigStore::open(cfg_path).expect("open config");
    let app_cfg: AppConfig = store.resolve(instance).expect("resolve app config");
    let page: inkapp_core::geometry::PageConfig = store.resolve(instance).expect("resolve page config");
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

    app(App)
        .connector(connectors)
        .update(update)
        .view(view)
        .key(key)
        .page(page.into())
        .build()
}

async fn run_doctor(
    cfg_path: &std::path::Path,
    secrets_path: &std::path::Path,
    instance: &str,
) -> i32 {
    use inkapp::Scope;
    let store = match ConfigStore::open(cfg_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("doctor: cannot open config at {}: {e}", cfg_path.display());
            return 1;
        }
    };
    let app_cfg: AppConfig = store.resolve(instance).unwrap_or_default();
    let device: DeviceConfig = store.resolve(instance).unwrap_or_default();
    let connector_token_name = app_cfg.readwise.kind.clone(); // "readwise"
    let device_auth_name = device.backend.clone();             // "remarkable"

    // Build the cassette-free Readwise from config IF possible; otherwise
    // skip the connector_refresh check by not adding it.
    let cache_dir = std::env::temp_dir().join("inkapp-doctor").join(instance);
    let mut secrets = SecretStore::open(secrets_path).expect("open secrets");
    let readwise_arc: Option<Arc<dyn inkapp::Connector>> =
        match Connectors::from_config(&store, &app_cfg, &secrets, cache_dir.clone()).await {
            Ok(cx) => Some(cx.readwise.clone()),
            Err(_) => None,
        };

    let instance_owned = instance.to_string();
    let mut checklist = inkapp::doctor::Checklist::new(secrets_path)
        .user_key()
        .secret(Scope::ConnectorCred, connector_token_name.clone())
        .secret(Scope::DeviceAuth, device_auth_name.clone())
        .config_resolves::<AppConfig>(&store, instance, "app.reading-queue")
        .config_resolves::<inkapp_core::geometry::PageConfig>(&store, instance, "page")
        .config_resolves::<DeviceConfig>(&store, instance, "device");

    if let Some(rw) = readwise_arc {
        checklist = checklist.connector_refresh(&connector_token_name, rw);
    }

    // Render probe builds the app the normal way (will panic if user_key
    // missing — guard against that by only adding the probe when user_key set).
    if secrets.get(Scope::UserKey, "default").ok().flatten().is_some() {
        let cfg_path = cfg_path.to_path_buf();
        let instance = instance_owned.clone();
        checklist = checklist.render_probe(move || async move {
            let store = ConfigStore::open(&cfg_path)?;
            let app_cfg: AppConfig = store.resolve(&instance)?;
            let page: inkapp_core::geometry::PageConfig = store.resolve(&instance)?;
            let mut secrets = SecretStore::open_default()?;
            let key = secrets.user_key()?;
            let cache_dir = std::env::temp_dir().join("inkapp-doctor-probe").join(&instance);
            let connectors = Connectors::from_config(&store, &app_cfg, &secrets, cache_dir)
                .await
                .map_err(|e| inkapp_core::error::Error::Config(e.to_string()))?;
            Ok(app(App)
                .connector(connectors)
                .update(update)
                .view(view)
                .key(key)
                .page(page.into())
                .build())
        });
    }

    checklist.run().await
}
```

Note: the `build` closure for `render_probe` returns `Result<App, _>`, so any wiring error becomes a Fail outcome.

- [ ] **Step 2: Build the workspace**

Run: `nix develop -c cargo build -p reading-queue`
Expected: 0.

- [ ] **Step 3: Run the workspace test suite**

Run: `nix develop -c cargo test --workspace`
Expected: 0.

- [ ] **Step 4: Smoke-test preview without serve**

Run: `nix develop -c cargo run -p reading-queue -- preview --out /tmp/rq-preview`
Expected: writes PDFs to `/tmp/rq-preview/`; exit 0. (Requires real config + secrets present — if missing, will fail with a clear error; that's expected.)

- [ ] **Step 5: Smoke-test doctor**

Run: `nix develop -c cargo run -p reading-queue -- doctor`
Expected: prints the rows; exit code reflects state.

- [ ] **Step 6: Commit**

```bash
git add apps/reading-queue/src/main.rs
git commit -m "reading-queue: add preview and doctor subcommands"
```

---

## Task 8: Wire into `apps/agenda`

**Goal:** Mirror Task 7 on the agenda app — same `Cmd::Preview` and `Cmd::Doctor` shape; doctor's connector list reflects the agenda's connectors (ICS + localcal).

**Files:**
- Modify: `apps/agenda/src/main.rs`

**Acceptance Criteria:**
- [ ] `cargo build -p agenda` succeeds.
- [ ] `agenda preview --out /tmp/ag` writes PDFs.
- [ ] `agenda doctor` prints checks for agenda's actual connectors (no readwise check).

**Verify:** `nix develop -c cargo build -p agenda && nix develop -c cargo test --workspace` → exits 0

**Steps:**

- [ ] **Step 1: Read `apps/agenda/src/main.rs` and `apps/agenda/src/lib.rs`**

Run: `cat apps/agenda/src/main.rs apps/agenda/src/lib.rs`

Identify: the agenda's `Connectors::from_config` signature, its `AppConfig` field names for connector refs (likely `ics` and `localcal`), and the existing `Cli` shape.

- [ ] **Step 2: Adapt the reading-queue pattern from Task 7**

Apply the same structural changes:
- `Cli` gains `Preview(inkapp::cli::PreviewArgs)` and `Doctor` variants.
- `build_app(cfg_path, instance)` extracted.
- `run_doctor(...)` adds:
  - `secret(Scope::ConnectorCred, "ics")` (or whatever the agenda's connector kind name is)
  - `secret(Scope::ConnectorCred, "localcal")` if it has a credential, OR skip if it has no token
  - `config_resolves` for the agenda's AppConfig + page + device sections
  - `connector_refresh` for each of the agenda's connectors

Mirror the `render_probe` guarding logic from Task 7.

- [ ] **Step 3: Build and test the workspace**

Run: `nix develop -c cargo build -p agenda && nix develop -c cargo test --workspace`
Expected: 0.

- [ ] **Step 4: Commit**

```bash
git add apps/agenda/src/main.rs
git commit -m "agenda: add preview and doctor subcommands"
```

---

## Task 9: Document in `appdx.md` and final workspace check

**Goal:** Append the "Local preview & doctor" subsection to `docs/appdx.md` so the doc is true (the spec convention: definition-of-done = appdx updated). Confirm the full workspace test suite is green.

**Files:**
- Modify: `docs/appdx.md`

**Acceptance Criteria:**
- [ ] `docs/appdx.md` contains a new subsection naming both subcommands with example usage.
- [ ] `nix develop -c cargo test --workspace` is green.
- [ ] `nix develop -c cargo clippy --all-targets -- -D warnings` is green.
- [ ] `nix develop -c cargo fmt --check` is green.

**Verify:** `make test && make clippy && make fmt-check` → exits 0

**Steps:**

- [ ] **Step 1: Locate the right insertion point in `docs/appdx.md`**

Run: `grep -n "^##\|^###" docs/appdx.md`

Find a "Developer experience" / "Authoring" / "Tooling" section. If none fits exactly, append under a new `## Local preview & doctor` heading near the end of the doc, before any FUTURE/changelog material.

- [ ] **Step 2: Append the subsection**

```markdown
## Local preview & doctor

Two framework subcommands let an author validate an app without device round-trips.
They live in the `inkapp` facade and are mounted by every app binary.

### `<app> preview [--out DIR] [--serve] [--port N]`

Renders the app's current document set to `<DIR>/<key>.pdf` (default `./preview`).
With `--serve`, also binds an HTTP server on `0.0.0.0:<port>` (default `4747`) and
prints a Tailscale-reachable URL using the local hostname:

```
$ reading-queue preview --serve
preview: wrote 7 PDF(s) to ./preview
  abc123  (3 pages, 4128 bytes)  -> ./preview/abc123.pdf
  ...
preview: serving at http://neptune:4747
```

The server lists every doc at `/` and serves the raw PDF at `/<key>.pdf`.

### `<app> doctor`

Preflight checklist for an app's prerequisites. Reports one row per check:

```
[PASS] user key present
[PASS] readwise connector token present
[FAIL] device auth 'remarkable' present       — Scope::DeviceAuth name='remarkable' not in store
[PASS] [app.reading-queue] config resolves
[PASS] [page] config resolves
[PASS] [device] config resolves
[PASS] readwise connector refresh
[PASS] render probe — first content doc 'abc123' → 3 pages, 4128 bytes
```

Exit code is `0` if every check passed, `1` if any failed. Built on
`inkapp::doctor::Checklist`, which each app composes with its own connector
list — see `apps/reading-queue/src/main.rs::run_doctor` for the worked example.
```

- [ ] **Step 3: Run the full check suite**

Run: `nix develop -c cargo fmt && nix develop -c cargo test --workspace && nix develop -c cargo clippy --all-targets -- -D warnings`
Expected: 0 from each.

- [ ] **Step 4: Commit**

```bash
git add docs/appdx.md
git commit -m "appdx: document local preview and doctor subcommands"
```
