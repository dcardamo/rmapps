# Local preview & doctor

**Status:** design
**Date:** 2026-05-25

## Motivation

Device round-trips (render → push to reMarkable cloud → sync to tablet → eyeball) are
slow and dependent on network and hardware. Most of what an app author wants to
validate — the content pipeline, theme aesthetics, pagination, manifest sealing,
connector wiring, secret/config presence — can be checked entirely off-device.

Two reusable facades, mounted as subcommands by each app binary:

- `preview` — render the app's current document set to PDFs on disk, optionally
  serve them over HTTP for browser viewing over Tailscale.
- `doctor` — a preflight checklist that reports pass/fail for the prerequisites
  every app needs before it can deploy: user key, connector credentials, device
  auth, config sections, connector liveness, and a render probe.

These are framework facilities, not app code: the implementations live in
`crates/inkapp` and are reused by every app.

## Non-goals

- No live device interaction in either subcommand.
- No HTML re-rendering of PDFs — just list-and-serve raw bytes.
- No watch mode / hot-reload. `preview --serve` is one-shot render then serve.
- No `--fake` flag — `preview` runs against the real config; `doctor` reports
  what's missing.

## API surface

Two new public modules in the `inkapp` facade crate:

### `inkapp::preview`

```rust
pub struct PreviewArgs {
    /// Directory to write `<key>.pdf` files into.
    pub out: PathBuf,
    /// Also start an HTTP server bound to 0.0.0.0.
    pub serve: bool,
    /// Port for --serve (default 4747).
    pub port: u16,
}

/// A single rendered preview document on disk.
pub struct RenderedEntry {
    pub key: String,
    pub path: PathBuf,
    pub size_bytes: usize,
    pub page_count: usize,
}

/// Render the app's document set and write each to `<out>/<key>.pdf`.
/// Returns one entry per document, in view() order.
pub async fn render_to_dir<M, Msg, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    out: &Path,
) -> Result<Vec<RenderedEntry>>;

/// Build the axum router for the in-memory PDF set. Pure; testable
/// without binding a socket. GET / → HTML index; GET /<key>.pdf → bytes.
pub fn make_router(pdfs: HashMap<String, Vec<u8>>) -> axum::Router;

/// One-shot top-level: render to `args.out`, optionally serve.
/// Returns an exit code (0 on success). Prints the URL on `--serve`
/// as `http://<hostname>:<port>` using the local hostname.
pub async fn run<M, Msg, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    args: PreviewArgs,
) -> Result<i32>;
```

`PreviewArgs` is also re-exported under `inkapp::cli::PreviewArgs` with a
`clap::Args` derive so app binaries can mount it directly.

### `inkapp::doctor`

```rust
pub enum Status { Pass, Fail, Skip }

pub struct Outcome {
    pub name: String,
    pub status: Status,
    pub detail: String,
}

pub struct Checklist { /* … */ }

impl Checklist {
    /// Construct a Checklist bound to a secrets-file path. Secret checks
    /// inspect this file directly (a missing file is treated as empty).
    pub fn new(secrets_path: impl Into<PathBuf>) -> Self;

    /// Check that a secret is present in the bound store.
    pub fn secret(self, scope: Scope, name: impl Into<String>) -> Self;

    /// Check that the per-user encryption key is present and 32 bytes.
    pub fn user_key(self) -> Self;

    /// Check that a config section resolves cleanly.
    pub fn config_resolves<T: inkapp_config::Config>(
        self, store: &ConfigStore, instance: &str, label: &str,
    ) -> Self;

    /// Check that a bound connector refreshes without error.
    pub fn connector_refresh(self, label: &str, c: Arc<dyn Connector>) -> Self;

    /// Run the app's full render and pick a probe document: the first
    /// non-`_banner` doc in view(). Reports its key, page count, and size.
    /// Skips with detail "user_key missing" if user_key is absent.
    pub fn render_probe<M, Msg, Cx: ConnectorSet>(
        self, app_builder: impl FnOnce() -> Result<App<M, Msg, Cx>>,
    ) -> Self;

    /// Run every check in order. Prints one row per check.
    /// Returns 0 iff every outcome is Pass or Skip; 1 if any Fail.
    pub async fn run(self) -> i32;
}
```

Doctor inspects `SecretStore::open_default()` directly rather than going through
`secrets.user_key()` (which generates a missing key) — its job is to *report*,
not to silently materialize state.

## App wiring (shared-file additive change)

In `apps/reading-queue/src/main.rs` and `apps/agenda/src/main.rs`, the `Cli`
enum gains two variants:

```rust
#[derive(Parser)]
struct Cli {
    #[arg(long, global = true)]
    instance: Option<String>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    Config(cli::ConfigCmd),
    Preview(inkapp::cli::PreviewArgs),
    Doctor,
}
```

- `Cmd::Config` and `Cmd::Preview` build the app from real config and run their
  respective facades.
- `Cmd::Doctor` does NOT panic on missing secrets — it constructs the Checklist
  from `app_cfg.readwise` (the connector ref) and `device.backend` (the device
  auth name), then runs.

`crates/inkapp/src/lib.rs` adds:

```rust
pub mod preview;
pub mod doctor;
pub use cli::PreviewArgs;
```

(Two sibling worktrees may both touch this file with single-line additive
changes; merge is trivial.)

## Preview server

- Bind `0.0.0.0:<port>`. Default port `4747`, overridable with `--port`.
- Hostname resolution: use the `hostname` crate (already small, no subprocess).
  If it fails, fall back to printing `0.0.0.0` with a note — the user knows the
  host they ran it on.
- Routes:
  - `GET /` → minimal HTML index listing each `<key>` as an `<a href="/{key}.pdf">`
    with page count and byte size shown alongside.
  - `GET /{key}.pdf` → `Content-Type: application/pdf`, body = raw PDF bytes.
  - Unknown keys → 404.
- Storage: an in-memory `HashMap<String, Vec<u8>>` built from the same
  `Vec<RenderedDoc>` returned by `App::render`. The same bytes are written to
  disk under `--out`.

## Doctor output

Single-pass plaintext, no ANSI colors. One row per check:

```
[PASS] user key present
[PASS] readwise token present
[FAIL] device auth 'remarkable' present       — Scope::DeviceAuth name='remarkable' not in store
[PASS] [app.reading-queue] config resolves
[PASS] [page] config resolves
[PASS] [device.default] config resolves
[PASS] readwise connector refresh
[PASS] render probe — first content doc 'abc123' → 2 pages, 4128 bytes
```

`render_probe` picks the first doc in `view()` whose key is not `_banner` (the
framework's failure notice doc). If `view()` returns only the banner, the probe
Fails with detail "no content docs to render".

Exit code: `0` iff every outcome is `Pass` or `Skip`; `1` if any `Fail`.

## Testing (TDD, no device, no network)

### `crates/inkapp/src/preview.rs`

- `render_to_dir_writes_n_pdfs`: build a cassette-backed reading-queue `App`
  with a fixed `Key::from_bytes([7; 32])`, call `render_to_dir` into a tempdir,
  assert N files (where N matches the cassette's article count, plus banner if
  present), each non-empty, each starts with `%PDF`.
- `router_serves_listed_pdfs`: construct `make_router` from a fixture
  `HashMap` of two known PDFs, exercise via `tower::ServiceExt::oneshot`:
  - `GET /` → 200, body contains both keys as `<a>` tags.
  - `GET /<key>.pdf` → 200, `Content-Type: application/pdf`, body is the exact
    bytes.
  - `GET /missing.pdf` → 404.
- No live socket bind is tested.

### `crates/inkapp/src/doctor.rs`

- `doctor_empty_store_fails`: tempdir SecretStore (file does not exist),
  Checklist with `user_key`, `secret(ConnectorCred, "readwise")`,
  `secret(DeviceAuth, "remarkable")`, and a `config_resolves` against an empty
  `ConfigStore`. `run().await` returns 1; outcomes include the missing-secret
  Fails by name; render_probe (if added) Skips.
- `doctor_populated_store_passes`: tempdir SecretStore seeded with user_key
  bytes, `readwise` token, `remarkable` device auth; ConfigStore with valid
  `[app.reading-queue.default]`, `[page]`, `[device.default]`; cassette
  `Readwise` connector; `render_probe` closure returns a cassette-backed App.
  `run().await` returns 0; every outcome is `Pass`.
- `outcome_summary_counts`: structural — verifies the runner counts
  pass/fail/skip correctly and exits with the right code.

### Workspace-wide

`nix develop -c cargo test --workspace` stays green.

## Conventions to honor

- Native task list cleared before each commit (commit hook blocks otherwise).
- Do NOT stage `Cargo.lock`.
- Final step: update `docs/appdx.md` with a "Local preview & doctor" subsection
  marking these as built, showing the two subcommands with example output.

## Open considerations

- The `hostname` crate vs `gethostname` vs subprocess: choose whichever is
  already in the dependency graph; otherwise `hostname` (smaller, no deps).
- The doctor output could grow ANSI color later, but plaintext is fine for now
  and survives ssh/tmux/non-tty without paragraphs of `tput` plumbing.
