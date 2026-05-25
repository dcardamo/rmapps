# Framework-provided, device-agnostic on-device deployment

**Date:** 2026-05-25
**Status:** design, pending implementation

## Goal

Make on-device deployment a framework capability instead of per-app code. Today
`apps/reading-queue/src/serve.rs` and `apps/agenda/src/serve.rs` are **byte-identical
183-line copies** (only the `FOLDER` const, the temp pull-dir name, and the
`crate::{Connectors, Msg}` import differ). Every new app — `rmreader` and beyond —
would copy that file again. That file also conflates two concerns: *transport*
(rmapi push/pull/delete) and *orchestration* (driving `App::render`/`step`).

The end state:

- An app's deploy code is **two device-agnostic calls** — `inkapp::publish(&mut app)`
  and `inkapp::sync_once(&mut app)`. No `serve.rs`, no device name, no path in source.
- **Which device backend and which folder are configuration**, read from a TOML file
  located by an env-pointed path. Adding a future backend (e.g. Supernote) is one new
  crate + one match arm in the facade; zero app changes, zero engine changes.
- The load-bearing logic (folder/key mapping, recursive `.rmdoc` discovery, per-key
  page-height decode) is **unit-testable without rmapi and without a device**, behind a
  command seam.

## Non-goals

- Re-testing the PDF↔scene transform. The deterministic harness already proves
  `read_ink`/`write_ink` are mutual inverses; these tests assume that.
- Changing any rmapi invariant. The `-ni` / `--content-only` / `mget` / non-recursive
  `mkdir` / stdin-null behavior documented in `docs/remarkable-pdf-mechanics.md`
  (§3, §10) is preserved verbatim, just relocated behind the seam.
- Multi-page `.rmdoc` ink (still wrapped single-page, as today), multi-device fan-out,
  and dynamic/plugin backend loading. The backend match is a static `match` with one
  arm today.

## Architecture — four layers, two of them new

```
App<M,Msg,Cx>                         the MVU loop (unchanged; harness drives it directly)
   │
inkapp::publish / sync_once           APP-FACING, config-driven (facade crate)
   │   reads DeployConfig (TOML via env path), resolves backend → transport
   ▼
inkapp_core::sync                     GENERIC ENGINE, device-agnostic (NEW, framework)
   trait DeviceTransport { push / delete / pull }
   fn publish(app, set, &T)  fn sync_once(app, set, &T)
   │   sees only keys, PDF bytes, PDF-space Strokes — never reMarkable
   ▼
rm-device::RmTransport                reMARKABLE BACKEND (impl of DeviceTransport)
   │   owns the Remarkable transform; folder/key mapping; .rmdoc discovery + decode
   ▼
rm-device::RmCommand                  TESTABILITY SEAM (NEW)
   trait { mkdir / put_content_only / rm / mget }
   real:  Rmapi  (exact -ni / --content-only / mget per mechanics doc)
   fake:  FakeRm (filesystem-backed; no rmapi, no device)
```

Two per-device seams now exist: `Device` (coordinate/ink **transform**, already in
`inkapp-core`) and `DeviceTransport` (sync **transport**, new). A future device
implements both in its own `*-device` crate. The *generic* boundary is the trait's
location in the framework — that is what keeps apps and the engine device-agnostic.

## Crate changes

### Rename `inkapp-remarkable` → `rm-device`

reMarkable-specific crates use the `rm-` prefix (matching `rm-files`). The renamed
crate holds the `Remarkable` transform **and** the new `RmTransport` + `RmCommand`
seam (one reMarkable crate, not two — the generic boundary is the framework trait, so
splitting transform from transport would add ceremony without sharpening the seam).

Mechanical fallout (all `inkapp_remarkable` → `rm_device`):
- workspace `members` entry and the crate directory + `Cargo.toml` `name`
- `crates/inkapp/Cargo.toml` dep + the `pub use` re-export in its `lib.rs`
- `crates/inkapp-harness/Cargo.toml` dep + ~12 `use inkapp_remarkable::Remarkable`
  in `crates/inkapp-harness/tests/*` (and the message string in `transform_fidelity.rs`)
- the crate's own `tests/device.rs`
- `Cargo.lock` updates but is **never staged**

New deps on `rm-device`: `zip = "2"` (read `.rmdoc`), `tempfile = "3"` (pull scratch
dir), `tempfile`/`zip` already used widely in the workspace.

### `inkapp-core`: new `sync` module

`DeviceTransport` trait + generic `publish`/`sync_once`. No new deps (async + `futures`
already present via `runtime.rs`).

### `inkapp` facade: config resolution

Adds `DeployConfig` loading and backend resolution. New deps: `toml = "0.8"`, `serde`
(derive). Dev-deps: `tokio` + `tempfile` for the resolution/parse tests.

## Public API

### App-facing (facade)

```rust
/// Render the app's full document set and push every document to the device.
pub async fn publish<M, Msg, Cx: ConnectorSet>(app: &mut App<M, Msg, Cx>)
    -> inkapp_core::error::Result<()>;

/// Pull device ink, fold one cycle, and apply the resulting ops (push changed,
/// delete removed). Renders first to rebuild the in-memory DocSet, exactly as the
/// manual bar's separate `sync_from_device` process does today.
pub async fn sync_once<M, Msg: Clone, Cx: ConnectorSet>(app: &mut App<M, Msg, Cx>)
    -> inkapp_core::error::Result<Cycle<Msg>>;
```

Both load `DeployConfig`, resolve the transport, create a fresh `DocSet`, and delegate
to the engine. The app supplies neither a transport nor a `DocSet`.

### Configuration

`DeployConfig` is read from the TOML file whose path is in the `INKAPP_DEPLOY_CONFIG`
env var. Missing var or unreadable/unparseable file → a clear `Error` (surfaced by the
bar's `.expect`). Example `deploy.toml` (gitignored, like `.overlay.json`):

```toml
backend = "remarkable"     # optional; defaults to "remarkable"
folder  = "/ReadingQueue"  # required: device folder for this app's documents
```

```rust
#[derive(serde::Deserialize)]
pub struct DeployConfig {
    #[serde(default = "default_backend")]   // "remarkable"
    pub backend: String,
    pub folder: String,
}
```

Resolution (the only place a concrete backend is named). It returns a boxed trait
object so adding a second backend is a new arm returning a different concrete type —
which `impl Trait` could not express:

```rust
fn resolve(cfg: &DeployConfig) -> Result<Box<dyn DeviceTransport>> {
    match cfg.backend.as_str() {
        "remarkable" => Ok(Box::new(RmTransport::new(cfg.folder.clone()))),
        other => Err(Error::Config(format!("unknown deploy backend {other:?}"))),
    }
}
```

## The generic engine (`inkapp-core::sync`)

```rust
pub trait DeviceTransport {
    /// Push a rendered document (its key + PDF) to the device.
    fn push(&self, key: &str, pdf: &[u8]) -> Result<()>;
    /// Delete a document by key. Best-effort (a missing doc is fine), matching today.
    fn delete(&self, key: &str);
    /// Pull all device ink, keyed by document key, as PDF-space strokes.
    /// `page_h_by_key` lets the backend decode each doc at its own page height.
    fn pull(&self, page_h_by_key: &HashMap<String, f64>)
        -> HashMap<String, Vec<Vec<Stroke>>>;
}

pub async fn publish<M, Msg, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>, set: &mut DocSet, transport: &dyn DeviceTransport) -> Result<()>;

pub async fn sync_once<M, Msg: Clone, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>, set: &mut DocSet, transport: &dyn DeviceTransport)
    -> Result<Cycle<Msg>>;
```

`&dyn DeviceTransport` (not a generic `T`) lets the facade pass the boxed,
config-resolved backend straight through, and lets tests pass `&FakeTransport` by
coercion. The trait is object-safe: no generic methods, no `Self` returns.

Bodies are today's orchestration, made generic:

- **publish**: `app.render(set)` → `transport.push(key, pdf)` for each rendered doc →
  print a generic summary.
- **sync_once**: `app.render(set)` (rebuild the DocSet) → build `page_h_by_key` from
  `set` → `transport.pull(&page_h_by_key)` → `app.step(set, &ink)` →
  `transport.delete` for each `DocOp::Delete`, `transport.push` for each changed
  rendered doc → return the `Cycle`.

The folder is gone from the engine (it is the transport's concern), so the summary
message is backend-neutral.

## The reMarkable backend (`rm-device`)

```rust
pub struct RmTransport<C: RmCommand = Rmapi> {
    folder: String,
    device: Remarkable,   // owns the transform; this is why the device test drops its arg
    cmd: C,
}
impl RmTransport<Rmapi> {
    pub fn new(folder: impl Into<String>) -> Self;            // real rmapi
}
impl<C: RmCommand> RmTransport<C> {
    pub fn with_command(cmd: C, folder: impl Into<String>) -> Self;  // tests inject FakeRm
}
impl<C: RmCommand> DeviceTransport for RmTransport<C> { … }
```

The command seam — the entire rmapi surface, four verbs:

```rust
pub trait RmCommand {
    fn mkdir(&self, folder: &str);                          // best-effort, non-recursive
    fn put_content_only(&self, local_pdf: &Path, folder: &str) -> Result<()>;
    fn rm(&self, remote_path: &str);                        // best-effort
    fn mget(&self, folder: &str, into_dir: &Path) -> bool;  // false on failure
}
```

`Rmapi` (real) reproduces the invocations verbatim (with the existing comments citing
mechanics §3/§10): `rmapi -ni …` with stdin nulled; `put --content-only`; folder pull
via `mget` run with `into_dir` as cwd; per-ancestor `mkdir`.

Pure, device-free helpers (the load-bearing logic, unit-tested directly):

```rust
/// Recursively collect *.rmdoc under `dir` (mget nests under a remote-named subdir).
fn find_rmdocs(dir: &Path) -> Vec<PathBuf>;

/// Map each discovered .rmdoc to (key, path, page_h): stem → key; per-key page height
/// from `page_h_by_key` (0.0 if unknown, as today).
fn discover(dir: &Path, page_h_by_key: &HashMap<String, f64>) -> Vec<Discovered>;
struct Discovered { key: String, path: PathBuf, page_h: f64 }
```

`DeviceTransport::push` writes `<key>.pdf` to a temp file then `cmd.put_content_only`;
`delete` calls `cmd.rm("{folder}/{key}")`; `pull` creates a fresh temp dir, calls
`cmd.mget`, runs `discover`, and for each `Discovered` extracts the first `.rm` from the
zip and calls `self.device.read_ink(bytes, page_h)`.

## App migration (both apps)

For `reading-queue` and `agenda`:

- **Delete** `src/serve.rs` and its `pub mod serve;` in `src/lib.rs`.
- Rewrite `tests/device.rs` so each `#[ignore]` bar calls the facade directly:

```rust
#[tokio::test] #[ignore = "manual: requires a paired reMarkable + rmapi"]
async fn publish_to_device() {
    inkapp::publish(&mut build_app()).await.expect("publish");
    eprintln!("Published. …then run sync_from_device.");
}
#[tokio::test] #[ignore = "manual: …run after inking + syncing the device"]
async fn sync_from_device() {
    inkapp::sync_once(&mut build_app()).await.expect("sync");
    eprintln!("Synced.");
}
```

`build_app()` is unchanged. The bars no longer construct a `Remarkable` or a `DocSet`,
and no longer name a folder. Their doc-comments gain the one operator step: set
`INKAPP_DEPLOY_CONFIG` to a `deploy.toml` (`backend`/`folder`). The manual workflow is
otherwise unchanged.

## Error handling

- `push`/`put_content_only` return `Result`; the engine propagates; the bar's `.expect`
  preserves today's panic-on-failure UX.
- `delete`/`rm`/`mkdir` are best-effort (no-op on error), exactly as today.
- `mget` failure → `pull` returns an empty map (as today: nothing synced yet).
- Config errors (`INKAPP_DEPLOY_CONFIG` unset, file unreadable, parse failure, unknown
  backend) → `Error` with a message naming the fix.

## Testing (TDD; no rmapi, no device)

Unit tests in `rm-device`, driven by `FakeRm` (records `put`/`rm`/`mkdir`; `mget`
populates `into_dir` from a fixture):

1. `find_rmdocs` recurses into the nested subdir `mget` creates and ignores non-`.rmdoc`.
2. `discover` maps `<key>.rmdoc` → `key` and attaches the correct per-key `page_h`
   (this is the "per-key page-height decode" wiring — proven without the transform).
3. `RmTransport::push`/`delete` via `FakeRm` record `<key>.pdf` under the folder and
   `{folder}/{key}` (folder/key mapping).
4. `RmTransport::pull` via `FakeRm` (whose `mget` writes a `.rmdoc` containing `.rm`
   bytes built by `Remarkable::write_ink`) returns ink under the right key — proving the
   zip-extract + `read_ink` call path that only the `#[ignore]` bar covered before.

Framework + facade:

5. (`inkapp-core`) a `FakeTransport` driving a tiny `App` proves `publish`/`sync_once`
   orchestration end-to-end with no device.
6. (`inkapp` facade) `DeployConfig` parses from TOML (incl. default backend); resolution
   returns a transport for `"remarkable"` and errors on an unknown backend. (Building
   `RmTransport::new` touches no rmapi, so this is device-free.)

Then: both apps build, their `#[ignore]` bars compile, and `cargo test --workspace`
(via `nix develop -c`) is green.

## Sequencing (TDD order)

1. Rename `inkapp-remarkable` → `rm-device` (compile-only; workspace green).
2. `inkapp-core::sync`: `DeviceTransport` + generic engine + `FakeTransport` test (5).
3. `rm-device`: `RmCommand` + `FakeRm` + pure `find_rmdocs`/`discover` + tests (1–2).
4. `rm-device`: `RmTransport` (real `Rmapi` seam, verbatim invariants) + tests (3–4).
5. `inkapp` facade: `DeployConfig` + resolution + `publish`/`sync_once` + test (6).
6. Migrate `reading-queue` and `agenda`: delete `serve.rs`, rewrite `tests/device.rs`.
7. `cargo test --workspace` green; `cargo fmt` (pre-commit hook).
8. Record the pushed-down capability in `docs/appdx.md`.

## Conventions

- Work on branch `inkapp-device-sync` in a git worktree of `~/git/inkapp`.
- Clear native tasks before each commit (the commit path blocks otherwise; `.tasks.json`).
- Never stage `Cargo.lock`.
- Preserve every rmapi invariant from `docs/remarkable-pdf-mechanics.md` verbatim in the
  `Rmapi` seam impl.

## Future (out of scope, enabled by this design)

- A second backend = a new `*-device` crate implementing `Device` + `DeviceTransport`,
  plus one `match` arm in the facade. Apps and engine untouched.
- Multi-page `.rmdoc` ink; multi-device fan-out (one `DocSet`, per-device ink streams);
  richer `deploy.toml` (per-device knobs).
```