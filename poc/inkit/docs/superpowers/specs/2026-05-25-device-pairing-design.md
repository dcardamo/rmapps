# Device Pairing — Spec

Status: draft, awaiting plan
Author: brainstorming session 2026-05-25
Branch: `setup-superpowers` (worktree `2ymhz306/normal-jaguar`)

## Problem

Connecting an inkapp deployment to a real reMarkable today requires the operator
to hand-set `RM_CLOUD_DEVICE_TOKEN` (and sometimes `RM_CLOUD_USER_TOKEN`) in the
environment before every run. `rm-cloud::register_device` already exists and the
fake-cloud test harness already echoes a deterministic device token on pairing,
but no path in inkapp drives it: `Scope::DeviceAuth` in `SecretStore` is defined
and unused, and `CloudTransport` only knows `from_env`. We also have no CLI to
populate the readwise token in `secrets.json` on a fresh machine — the existing
config CLI manages `config.toml` only.

Goal: a fresh machine can be brought online with two commands —
`app pair <code>` and `app secret set connector <name> <token>` — and the
existing env-var path keeps working as a fallback.

## Non-goals

- At-rest encryption / KMS / rotation for the secrets file (future; see appdx).
- A device-neutral pairing abstraction (pairing is fundamentally per-device family;
  reMarkable is the only supported device today).
- Duplicating the user-token refresh path, which `rm-cloud` already covers and tests.

## Architecture & placement

Three placement calls drove the design; all are reversible if the spec review
disagrees.

1. **Pairing implementation in `rm-device`**, not `inkapp` itself. Per the project
   convention "anything that knows the reMarkable cloud transport belongs in an
   `rm-`-prefixed crate", and because `rm-device` already deps both `rm-cloud`
   and `inkapp-core` (where `SecretStore` lives). The "facade in inkapp"
   requirement is met by a thin re-export.
2. **Operator CLI (`pair`, `secret`) in a new `inkapp::cli` module**, not in
   `inkapp-config::cli`. `inkapp-config` has no `inkapp-core` dependency today and
   stays focused on TOML config; `inkapp` already deps everything needed.
3. **One top-level `Op` arm in each app's `Command` enum**, nesting `pair` and
   `secret` underneath. Each parallel worktree adds exactly one arm, so merges
   between sibling worktrees that also add subcommands are trivial.

## Public surface

### `rm-device::auth` (new module, ~40 lines)

```rust
/// Name under which the reMarkable device token is stored in SecretStore.
pub const REMARKABLE_DEVICE_AUTH_NAME: &str = "remarkable";

/// Pair this machine with a reMarkable using an 8-char one-time code from
/// https://my.remarkable.com/device/desktop/connect. Calls
/// `rm_cloud::register_device` and persists the returned long-lived device
/// token under `Scope::DeviceAuth / REMARKABLE_DEVICE_AUTH_NAME`.
pub async fn pair(secrets: &mut SecretStore, code: &str) -> Result<()>;

/// Resolve credentials with store-takes-precedence-over-env semantics:
///   device_token: Scope::DeviceAuth / "remarkable", else RM_CLOUD_DEVICE_TOKEN
///   user_token : RM_CLOUD_USER_TOKEN env only (short-lived; refreshed lazily)
/// Returns `Error::MissingCredential` when neither source yields a device token
/// AND no user token is present.
pub fn resolve_credentials(secrets: &SecretStore) -> Result<rm_cloud::Credentials>;
```

`pair` builds the `rm_cloud::Config` from `Config::from_env` so the
fake-cloud-host env override used by tests applies identically to the pairing
HTTP call.

### `rm-device::transport` (additive on `CloudTransport`)

```rust
impl CloudTransport {
    /// Build with credentials resolved from the store-or-env (preferred).
    /// Existing `from_env` is unchanged; existing tests/flows keep working.
    pub fn from_secrets(secrets: &SecretStore, folder: impl Into<String>) -> Result<Self>;
}
```

Internally: `resolve_credentials(secrets)` → pick `Client::from_device_token`
when a device token is present, else `Client::from_user_token` for the env-only
user-token case → wrap with `Self::with_client(client, folder)`.

### `rm-cloud`

No change to public surface. Existing `register_device`, `Credentials`,
`Client::from_device_token`, `Client::from_user_token`, `Config::from_env` are
all sufficient.

### `inkapp` facade

- `pub use rm_device::{pair, resolve_credentials, REMARKABLE_DEVICE_AUTH_NAME};`
- `deploy::resolve_transport` signature gains `secrets: &SecretStore`:
  ```rust
  pub fn resolve_transport(
      backend: &str,
      folder: String,
      secrets: &SecretStore,
  ) -> Result<Box<dyn DeviceTransport>>;
  ```
  Routes `"remarkable"` to `CloudTransport::from_secrets(secrets, folder)`. The
  two app mains already open a `SecretStore` for the user key, so threading is
  a one-parameter change.

### `inkapp::cli` (new module, ~80 lines)

```rust
#[derive(clap::Subcommand)]
pub enum OpCmd {
    /// Pair this machine with a reMarkable using an 8-char code from
    /// https://my.remarkable.com/device/desktop/connect.
    Pair { code: String },
    /// Manage secrets (read/write secrets.json).
    #[command(subcommand)]
    Secret(SecretCmd),
}

#[derive(clap::Subcommand)]
pub enum SecretCmd {
    /// Store a secret. Scope is one of: connector | device-auth.
    Set { scope: ScopeArg, name: String, value: String },
    /// List secret names by scope (values are never printed).
    List,
}

#[derive(clap::ValueEnum, Clone, Copy)]
pub enum ScopeArg { Connector, DeviceAuth }

pub async fn run(cmd: OpCmd, secrets_path: PathBuf) -> Result<i32>;
```

Notes:

- `Secret Set` covers the readwise case (`app secret set connector readwise-reader <token>`)
  and any future connector secret without further per-app code.
- `Secret List` exists for operator sanity checks. Values are never printed —
  only the `(scope, name)` pairs — so it's safe to dump to a terminal.
- `Scope::UserKey` is intentionally not exposed to the CLI: the per-user
  encryption key is auto-generated and must not be operator-edited.
- `run` returns an `i32` exit code, matching `inkapp_config::cli::run`'s shape.

## App CLI restructure (the merge point)

Today: `struct Cli { instance, config: Option<cli::ConfigCmd> }`.

After:

```rust
#[derive(clap::Parser)]
#[command(name = "<app>")]
struct Cli {
    #[arg(long, global = true)]
    instance: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Config-file operations.
    #[command(subcommand)]
    Config(inkapp::cli::ConfigCmd),
    // <-- This worktree adds exactly this arm:
    /// Pair the device, or manage secrets (operator setup).
    #[command(subcommand)]
    Op(inkapp::cli::OpCmd),
    // <-- Sibling worktrees add their arms here; one line per worktree,
    //     so merges are trivial.
}
```

Dispatch in `main`: if `command` is `Some`, run the subcommand and `exit`
before any wiring (matches today's `config`-subcommand behaviour). Else fall
through to the existing publish/render flow, now threading `&secrets` into
`resolve_transport`.

Applied identically to `apps/reading-queue/src/main.rs` and
`apps/agenda/src/main.rs`.

## Credential resolution rules (precise)

For `resolve_credentials(secrets) -> Result<Credentials>`:

| `Scope::DeviceAuth / "remarkable"` | `RM_CLOUD_DEVICE_TOKEN` | `RM_CLOUD_USER_TOKEN` | Result |
|------------------------------------|-------------------------|-----------------------|--------|
| present                            | _any_                   | _any_                 | `device_token = store value`, `user_token = env if set else None` |
| absent                             | present                 | _any_                 | `device_token = env value`, `user_token = env if set else None` |
| absent                             | absent                  | present               | `device_token = None`, `user_token = env value` (env-only path, no auto-refresh — matches existing `Client::from_env` behaviour) |
| absent                             | absent                  | absent                | `Err(Error::MissingCredential("RM_CLOUD_DEVICE_TOKEN or paired device"))` |

Empty strings are treated as absent (matches existing `Credentials::from_env`).
The store path takes precedence over env for the device token specifically;
the user token always comes from env because it is short-lived (a stored
user-token would be stale within an hour anyway and is not the durable
credential).

## Tests (TDD, no real cloud)

Three files, all green under `nix develop -c cargo test --workspace`.

### `crates/inkapp/tests/pairing.rs`

1. Spin up the `rm-cloud` fake cloud (`rm-cloud/src/fake`) via its existing
   axum-on-`127.0.0.1:0` helper; capture the bound base URL.
2. Set `RM_CLOUD_HOST=<url>` so `rm_cloud::Config::from_env()` resolves to the
   fake.
3. Open a `SecretStore` at a tempdir path.
4. `inkapp::pair(&mut secrets, "ABCD1234").await.unwrap();`
5. Assert: `secrets.get(Scope::DeviceAuth, REMARKABLE_DEVICE_AUTH_NAME).unwrap()`
   round-trips to `b"device-token-for-ABCD1234"` (the fake's deterministic
   echo).
6. Reopen the store from disk → same value (persisted, not just in-memory).

### `crates/rm-device/tests/credentials.rs`

Three `resolve_credentials` cases (env management single-threaded; each test
clears both env vars on entry):

- **store-wins-over-env**: set store token + `RM_CLOUD_DEVICE_TOKEN` to
  different strings → resolved `device_token` equals the store value.
- **env-fallback**: store empty + env set → resolved `device_token` equals env.
- **missing-both**: store empty + env unset → `Err(MissingCredential)`.

Plus one `CloudTransport::from_secrets` smoke test pointed at the fake host:
construct, call `mkdir_p` (already exercised by transport's other tests) to
prove the resolved client is functional, with only a stored secret (no env).

### `crates/inkapp/tests/cli.rs`

- `OpCmd::try_parse_from` for `pair ABCD1234` → `Pair { code: "ABCD1234" }`.
- `OpCmd::try_parse_from` for `secret set connector readwise-reader tok` →
  `Secret(Set { scope: Connector, name: "readwise-reader", value: "tok" })`.
- End-to-end: call `inkapp::cli::run(parsed_secret_set, tmp_path)` →
  `SecretStore::open(tmp_path).get(...)` returns `b"tok"`.
- `secret list` after the round-trip prints the `(scope, name)` pair (and
  never the value).

Existing tests (notably `inkapp/src/deploy.rs::resolve_routes_known_and_rejects_unknown_backends`)
update their `resolve_transport` call sites to pass a temp `SecretStore`. Their
assertions don't otherwise change.

## Conventions

- TDD: each component is added test-first.
- Pre-commit hook blocks on open native tasks (`.tasks.json`). Clear them
  before each commit.
- Do **not** stage `Cargo.lock`. Any dep bumps it needs are a separate commit
  outside this work.
- Final step: `docs/appdx.md` — replace the "set RM_CLOUD_* env vars" line in
  the deployment recipe with the `app pair <code>` + `app secret set connector
  readwise-reader <token>` flow; note that env vars remain a supported
  fallback for CI / one-shot use.

## Implementation order (preview for the plan)

1. `rm-device::auth` module (`pair`, `resolve_credentials`, name constant) — test first against the fake.
2. `CloudTransport::from_secrets` — smoke test against the fake.
3. `inkapp` re-exports + `deploy::resolve_transport(secrets)` signature change — update existing call sites.
4. `inkapp::cli` module + tests for parsing and the `secret set` round-trip.
5. Both app `main.rs`: top-level `Command` enum with `Config` + `Op` arms.
6. `docs/appdx.md` update.
7. Run `nix develop -c cargo test --workspace`; clear `.tasks.json`; commit (no `Cargo.lock`).

## Out of scope (deliberate)

- `secret rm` / `secret get`. `Set` and `List` cover the bootstrap workflow;
  delete-and-rewrite is fine for now.
- A `pair status` command. Operator can `secret list` to see if `remarkable` is
  set.
- Re-pairing flow (overwrites are silent today; that matches the existing
  `SecretStore::set` semantics).
