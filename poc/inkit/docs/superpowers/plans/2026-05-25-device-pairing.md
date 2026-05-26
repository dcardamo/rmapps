# Device Pairing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace hand-set `RM_CLOUD_*` env vars with a real pairing workflow: `app pair <code>` stores a long-lived device token in `SecretStore`; `CloudTransport` reads it (env as fallback); `app secret set` populates the readwise token.

**Architecture:** Pairing + credential resolution live in `rm-device` (per `rm-`-prefix convention; already deps `rm-cloud` and `inkapp-core`). Operator CLI (`pair`, `secret`) lives in a new `inkapp::cli` module — keeps `inkapp-config` strictly TOML. Both app mains gain a top-level `Command` enum with a single `Op` arm, one-arm-per-worktree for trivial merges with sibling subcommands.

**Tech Stack:** Rust workspace, clap 4 derive, tokio, axum (in `rm-cloud`'s fake-cloud feature), `inkapp-core::secrets::SecretStore`.

**Spec:** `docs/superpowers/specs/2026-05-25-device-pairing-design.md` (commit `c6cb71c`).

**Build/test command:** `nix develop -c cargo test --workspace`. Do **not** stage `Cargo.lock`. Clear `.tasks.json` (this file's sibling) before commits per the pre-commit hook.

---

## File map

| File | Action | Responsibility |
|------|--------|----------------|
| `crates/rm-device/src/auth.rs` | create | `REMARKABLE_DEVICE_AUTH_NAME`, `pair`, `resolve_credentials`, inner `resolve_with` |
| `crates/rm-device/src/lib.rs` | modify | `pub mod auth; pub use auth::*;` |
| `crates/rm-device/src/transport.rs` | modify | add `CloudTransport::from_secrets` |
| `crates/rm-device/tests/credentials.rs` | create | resolve-credentials cases + pairing fake round-trip + from_secrets smoke |
| `crates/inkapp/src/cli.rs` | create | `OpCmd`, `SecretCmd`, `ScopeArg`, `run` |
| `crates/inkapp/src/lib.rs` | modify | re-export `cli`, `pair`, `resolve_credentials`, `REMARKABLE_DEVICE_AUTH_NAME` |
| `crates/inkapp/src/deploy.rs` | modify | `resolve_transport(backend, folder, &SecretStore)` |
| `crates/inkapp/tests/cli.rs` | create | parse `OpCmd::Pair` + `OpCmd::Secret`; `secret set` round-trip |
| `apps/reading-queue/src/main.rs` | modify | top-level `Command` enum with `Config` + `Op` arms |
| `apps/agenda/src/main.rs` | modify | identical restructure |
| `docs/appdx.md` | modify | replace env-var step with `app pair` + `app secret set` |

---

### Task 0: `rm-device::auth` — credential resolution (TDD)

**Goal:** A pure, env-injectable credential resolver in `rm-device`, with a public wrapper that reads `std::env::var`.

**Files:**
- Create: `crates/rm-device/src/auth.rs`
- Modify: `crates/rm-device/src/lib.rs`
- Create: `crates/rm-device/tests/credentials.rs` (first 3 cases only this task)

**Acceptance Criteria:**
- [ ] `REMARKABLE_DEVICE_AUTH_NAME == "remarkable"`.
- [ ] `resolve_with(secrets, get_env)` returns store device-token when present, else env, else `Err(MissingCredential)`.
- [ ] User token from env-only is acceptable (no device token required).
- [ ] Empty strings are treated as absent.

**Verify:** `nix develop -c cargo test -p rm-device --test credentials resolve_` → 4 passing tests.

**Steps:**

- [ ] **Step 1: Write failing tests in `crates/rm-device/tests/credentials.rs`**

```rust
//! Credential resolution: store-wins-over-env, env-fallback, missing-both, user-token-only.

use inkapp_core::secrets::{Scope, SecretStore};
use rm_cloud::Error as CloudError;
use rm_device::auth::{resolve_with, REMARKABLE_DEVICE_AUTH_NAME};

fn tmp_store() -> (tempfile::TempDir, SecretStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = SecretStore::open(dir.path().join("secrets.json")).unwrap();
    (dir, store)
}

#[test]
fn resolve_store_wins_over_env() {
    let (_d, mut s) = tmp_store();
    s.set(Scope::DeviceAuth, REMARKABLE_DEVICE_AUTH_NAME, b"from-store")
        .unwrap();
    let env = |k: &str| match k {
        "RM_CLOUD_DEVICE_TOKEN" => Some("from-env".into()),
        _ => None,
    };
    let creds = resolve_with(&s, env).unwrap();
    assert_eq!(creds.device_token.as_deref(), Some("from-store"));
    assert!(creds.user_token.is_none());
}

#[test]
fn resolve_falls_back_to_env_device_token() {
    let (_d, s) = tmp_store();
    let env = |k: &str| match k {
        "RM_CLOUD_DEVICE_TOKEN" => Some("from-env".into()),
        "RM_CLOUD_USER_TOKEN" => Some("user-from-env".into()),
        _ => None,
    };
    let creds = resolve_with(&s, env).unwrap();
    assert_eq!(creds.device_token.as_deref(), Some("from-env"));
    assert_eq!(creds.user_token.as_deref(), Some("user-from-env"));
}

#[test]
fn resolve_user_token_only_is_ok() {
    let (_d, s) = tmp_store();
    let env = |k: &str| match k {
        "RM_CLOUD_USER_TOKEN" => Some("just-user".into()),
        _ => None,
    };
    let creds = resolve_with(&s, env).unwrap();
    assert!(creds.device_token.is_none());
    assert_eq!(creds.user_token.as_deref(), Some("just-user"));
}

#[test]
fn resolve_missing_both_is_error() {
    let (_d, s) = tmp_store();
    let env = |_: &str| None;
    let err = resolve_with(&s, env).unwrap_err();
    assert!(matches!(err, CloudError::MissingCredential(_)));
}

#[test]
fn resolve_empty_strings_are_absent() {
    let (_d, mut s) = tmp_store();
    s.set(Scope::DeviceAuth, REMARKABLE_DEVICE_AUTH_NAME, b"")
        .unwrap();
    let env = |k: &str| match k {
        "RM_CLOUD_DEVICE_TOKEN" => Some("".into()),
        "RM_CLOUD_USER_TOKEN" => Some("".into()),
        _ => None,
    };
    let err = resolve_with(&s, env).unwrap_err();
    assert!(matches!(err, CloudError::MissingCredential(_)));
}
```

- [ ] **Step 2: Run — expect compile failure (module missing)**

```
nix develop -c cargo test -p rm-device --test credentials --no-run
```
Expected: error E0432 (unresolved import `rm_device::auth`).

- [ ] **Step 3: Create `crates/rm-device/src/auth.rs`**

```rust
//! reMarkable device pairing + credential resolution.
//!
//! Pairing calls `rm_cloud::register_device` with an 8-char one-time code from
//! <https://my.remarkable.com/device/desktop/connect> and persists the returned
//! long-lived device token into [`SecretStore`] under
//! [`Scope::DeviceAuth`] / [`REMARKABLE_DEVICE_AUTH_NAME`].
//!
//! Credential resolution prefers the stored device token, falling back to the
//! `RM_CLOUD_DEVICE_TOKEN` env var. The short-lived user token always comes
//! from `RM_CLOUD_USER_TOKEN` env (it is refreshed lazily from the device
//! token anyway).

use inkapp_core::error::{Error as CoreError, Result as CoreResult};
use inkapp_core::secrets::{Scope, SecretStore};
use rm_cloud::{register_device, Config, Credentials, Error as CloudError};

/// Name under which the reMarkable device token is stored in [`SecretStore`].
pub const REMARKABLE_DEVICE_AUTH_NAME: &str = "remarkable";

/// Pair this machine with a reMarkable using an 8-char one-time code.
///
/// `config` is taken as a parameter so tests can point at the in-process fake
/// cloud via `Config::single_host(...)`. Production callers pass
/// `Config::from_env()`.
pub async fn pair(
    secrets: &mut SecretStore,
    config: &Config,
    code: &str,
) -> Result<(), CloudError> {
    let http = reqwest::Client::new();
    let token = register_device(&http, config, code).await?;
    secrets
        .set(Scope::DeviceAuth, REMARKABLE_DEVICE_AUTH_NAME, token.as_bytes())
        .map_err(|e| CloudError::Http(format!("secrets: {e}")))?;
    Ok(())
}

/// Resolve credentials with store-takes-precedence-over-env semantics.
/// Public wrapper around [`resolve_with`] using `std::env::var`.
pub fn resolve_credentials(secrets: &SecretStore) -> Result<Credentials, CloudError> {
    resolve_with(secrets, |k| std::env::var(k).ok())
}

/// Inner, env-injectable resolver — race-free in parallel tests.
///
/// Rules:
/// - `device_token`: store value at `Scope::DeviceAuth / REMARKABLE_DEVICE_AUTH_NAME`
///   if present, else `get_env("RM_CLOUD_DEVICE_TOKEN")`.
/// - `user_token`: `get_env("RM_CLOUD_USER_TOKEN")` only.
/// - Empty strings are treated as absent.
/// - Returns `MissingCredential` when neither a device token nor a user token is found.
pub fn resolve_with(
    secrets: &SecretStore,
    get_env: impl Fn(&str) -> Option<String>,
) -> Result<Credentials, CloudError> {
    fn non_empty(s: Option<String>) -> Option<String> {
        s.filter(|v| !v.is_empty())
    }

    let stored = secrets
        .get(Scope::DeviceAuth, REMARKABLE_DEVICE_AUTH_NAME)
        .map_err(|e: CoreError| CloudError::Http(format!("secrets: {e}")))?
        .and_then(|b| String::from_utf8(b).ok());

    let device_token = non_empty(stored).or_else(|| non_empty(get_env("RM_CLOUD_DEVICE_TOKEN")));
    let user_token = non_empty(get_env("RM_CLOUD_USER_TOKEN"));

    if device_token.is_none() && user_token.is_none() {
        return Err(CloudError::MissingCredential(
            "no paired device and no RM_CLOUD_* env tokens",
        ));
    }
    Ok(Credentials {
        device_token,
        user_token,
    })
}

// `pair`'s integration test lives in tests/credentials.rs (uses FakeCloud).
// `resolve_with`'s unit tests likewise — they're in tests/credentials.rs to
// keep the test surface co-located.

#[doc(hidden)]
pub use inkapp_core as _unused_marker; // silence unused-dep warnings if any
```

Note: drop the `_unused_marker` line if `inkapp_core` is already used in the file (it is, via `Scope`/`SecretStore`). Verify by running clippy at end of task.

Also note: `CoreResult` is imported but unused above; remove it. The map_err uses `CoreError` which IS used. Final imports should be:

```rust
use inkapp_core::error::Error as CoreError;
use inkapp_core::secrets::{Scope, SecretStore};
use rm_cloud::{register_device, Config, Credentials, Error as CloudError};
```

- [ ] **Step 4: Wire the module in `crates/rm-device/src/lib.rs`**

Read the current top of `lib.rs`, then add (alongside existing `pub mod` lines):

```rust
pub mod auth;
pub use auth::{pair, resolve_credentials, resolve_with, REMARKABLE_DEVICE_AUTH_NAME};
```

- [ ] **Step 5: Run tests — expect 5 passing**

```
nix develop -c cargo test -p rm-device --test credentials -- resolve_
```

- [ ] **Step 6: Clippy + fmt**

```
nix develop -c cargo clippy -p rm-device --all-targets -- -D warnings
nix develop -c cargo fmt
```

- [ ] **Step 7: Clear native task, commit**

```
TaskUpdate this task → completed
git add crates/rm-device/src/auth.rs crates/rm-device/src/lib.rs crates/rm-device/tests/credentials.rs
git commit -m "rm-device: resolve_credentials + REMARKABLE_DEVICE_AUTH_NAME (store-or-env)"
```

---

### Task 1: `rm-device::auth::pair` — integration test against FakeCloud

**Goal:** Prove pairing end-to-end: `register_device` → store → reopen-from-disk round-trip.

**Files:**
- Modify: `crates/rm-device/tests/credentials.rs` (append).

**Acceptance Criteria:**
- [ ] `pair(secrets, &Config::single_host(fake.base), "ABCD1234")` stores `b"device-token-for-ABCD1234"` under `Scope::DeviceAuth / "remarkable"`.
- [ ] Value survives `SecretStore::open` reopen.

**Verify:** `nix develop -c cargo test -p rm-device --test credentials pair_` → passes.

**Steps:**

- [ ] **Step 1: Append failing test**

```rust
use inkapp_core::secrets::SecretStore as Store;
use rm_cloud::fake::FakeCloud;
use rm_cloud::Config;
use rm_device::auth::pair;

#[tokio::test]
async fn pair_stores_device_token_and_survives_reopen() {
    let cloud = FakeCloud::spawn().await;
    let config = Config::single_host(&cloud.base);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");

    {
        let mut secrets = Store::open(&path).unwrap();
        pair(&mut secrets, &config, "ABCD1234").await.unwrap();
        assert_eq!(
            secrets
                .get(
                    inkapp_core::secrets::Scope::DeviceAuth,
                    rm_device::auth::REMARKABLE_DEVICE_AUTH_NAME,
                )
                .unwrap()
                .unwrap(),
            b"device-token-for-ABCD1234"
        );
    }

    // Reopen from disk — same value.
    let reopened = Store::open(&path).unwrap();
    assert_eq!(
        reopened
            .get(
                inkapp_core::secrets::Scope::DeviceAuth,
                rm_device::auth::REMARKABLE_DEVICE_AUTH_NAME,
            )
            .unwrap()
            .unwrap(),
        b"device-token-for-ABCD1234"
    );
}

#[tokio::test]
async fn pair_overwrites_previous_token() {
    let cloud = FakeCloud::spawn().await;
    let config = Config::single_host(&cloud.base);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");
    let mut secrets = Store::open(&path).unwrap();
    pair(&mut secrets, &config, "FIRST123").await.unwrap();
    pair(&mut secrets, &config, "SECOND12").await.unwrap();
    assert_eq!(
        secrets
            .get(
                inkapp_core::secrets::Scope::DeviceAuth,
                rm_device::auth::REMARKABLE_DEVICE_AUTH_NAME,
            )
            .unwrap()
            .unwrap(),
        b"device-token-for-SECOND12"
    );
}
```

Confirm `tempfile`, `tokio` features (`macros`, `rt-multi-thread`), and `rm-cloud` with `fake` are already in `crates/rm-device/Cargo.toml` `[dev-dependencies]` — they are (verified in plan prep).

- [ ] **Step 2: Run — both pass**

```
nix develop -c cargo test -p rm-device --test credentials pair_
```

- [ ] **Step 3: Full crate clippy + fmt**

```
nix develop -c cargo clippy -p rm-device --all-targets -- -D warnings && nix develop -c cargo fmt
```

- [ ] **Step 4: Clear task, commit**

```
git add crates/rm-device/tests/credentials.rs
git commit -m "rm-device: pair() integration test against FakeCloud"
```

---

### Task 2: `CloudTransport::from_secrets`

**Goal:** A production constructor that resolves credentials (store-or-env) and builds the `Client` correctly for either device-token or env-only-user-token cases.

**Files:**
- Modify: `crates/rm-device/src/transport.rs` (add one ctor).
- Modify: `crates/rm-device/tests/credentials.rs` (one smoke test).

**Acceptance Criteria:**
- [ ] `CloudTransport::from_secrets(secrets, folder)` returns `Ok` when a device token is stored, even with no env.
- [ ] Returns `Err(Transport(..))` when neither store nor env have any token.
- [ ] Uses `Client::from_device_token` when device token resolved; `Client::from_user_token` for user-only-env path.

**Verify:** `nix develop -c cargo test -p rm-device --test credentials from_secrets_` → passes.

**Steps:**

- [ ] **Step 1: Append failing tests**

```rust
use rm_device::CloudTransport;

#[tokio::test]
async fn from_secrets_succeeds_with_stored_device_token() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");
    let mut secrets = Store::open(&path).unwrap();
    secrets
        .set(
            inkapp_core::secrets::Scope::DeviceAuth,
            rm_device::auth::REMARKABLE_DEVICE_AUTH_NAME,
            b"stored-tok",
        )
        .unwrap();
    // SAFETY: clear env so this test is independent of host shell.
    std::env::remove_var("RM_CLOUD_DEVICE_TOKEN");
    std::env::remove_var("RM_CLOUD_USER_TOKEN");
    let t = CloudTransport::from_secrets(&secrets, "/X");
    assert!(t.is_ok(), "expected Ok, got {:?}", t.err().map(|e| e.to_string()));
}

#[test]
fn from_secrets_errors_when_nothing_set() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");
    let secrets = Store::open(&path).unwrap();
    std::env::remove_var("RM_CLOUD_DEVICE_TOKEN");
    std::env::remove_var("RM_CLOUD_USER_TOKEN");
    let err = CloudTransport::from_secrets(&secrets, "/X").unwrap_err();
    assert!(
        err.to_string().contains("rm-cloud") || err.to_string().contains("credential"),
        "unexpected error: {err}"
    );
}
```

- [ ] **Step 2: Implement in `crates/rm-device/src/transport.rs`**

Add (alongside `from_env`):

```rust
impl CloudTransport {
    /// A transport with credentials resolved from `SecretStore` (preferred) or
    /// `RM_CLOUD_*` env (fallback). The endpoint config comes from
    /// `Config::from_env()` like [`from_env`]; tests that need a fake host can
    /// still set `RM_CLOUD_HOST`, or construct via [`with_client`] directly.
    pub fn from_secrets(
        secrets: &inkapp_core::secrets::SecretStore,
        folder: impl Into<String>,
    ) -> Result<Self> {
        use rm_cloud::{Client, Config};
        let creds = crate::auth::resolve_credentials(secrets)
            .map_err(|e| Error::Transport(format!("rm-cloud: {e}")))?;
        let config = Config::from_env();
        let client = match (creds.device_token, creds.user_token) {
            (Some(d), _) => Client::from_device_token(config, d),
            (None, Some(u)) => Client::from_user_token(config, u),
            // resolve_credentials returns MissingCredential before reaching here.
            (None, None) => unreachable!("resolve_credentials guarantees at least one token"),
        };
        Ok(Self::with_client(client, folder))
    }
}
```

- [ ] **Step 3: Run — passes**

```
nix develop -c cargo test -p rm-device --test credentials from_secrets_
```

- [ ] **Step 4: Full credentials.rs run, clippy, fmt**

```
nix develop -c cargo test -p rm-device --test credentials
nix develop -c cargo clippy -p rm-device --all-targets -- -D warnings && nix develop -c cargo fmt
```

- [ ] **Step 5: Clear task, commit**

```
git add crates/rm-device/src/transport.rs crates/rm-device/tests/credentials.rs
git commit -m "rm-device: CloudTransport::from_secrets (store-or-env credentials)"
```

---

### Task 3: `inkapp` facade re-exports + `resolve_transport` signature change

**Goal:** Thread `&SecretStore` through `resolve_transport` and surface the pairing facade from `inkapp`. Update the one existing call site (test) so the workspace still builds.

**Files:**
- Modify: `crates/inkapp/src/lib.rs`
- Modify: `crates/inkapp/src/deploy.rs`

**Acceptance Criteria:**
- [ ] `inkapp::resolve_transport("remarkable", folder, &secrets)` routes to `CloudTransport::from_secrets`.
- [ ] `inkapp::{pair, resolve_credentials, REMARKABLE_DEVICE_AUTH_NAME}` re-exported.
- [ ] Existing in-file test updated and passing.

**Verify:** `nix develop -c cargo test -p inkapp` → green (app mains will be fixed in Task 5; this task targets the library crate only).

**Steps:**

- [ ] **Step 1: Update `crates/inkapp/src/deploy.rs`**

```rust
//! Device-agnostic on-device deployment facade. Apps resolve the `[device]`
//! backend (from `config.toml`) plus their own target folder, build a transport
//! via [`resolve_transport`], and pass it to [`publish`] / [`sync_once`]. This is
//! the only place a concrete device backend is named, so `inkapp-config` never
//! needs to depend on a `*-device` crate.

use inkapp_core::connector::ConnectorSet;
use inkapp_core::error::{Error, Result};
use inkapp_core::runtime::{App, Cycle, DocSet};
use inkapp_core::secrets::SecretStore;
use inkapp_core::sync::{self, DeviceTransport};

use rm_device::CloudTransport;

/// Resolve a backend identifier + device folder + secret store into a concrete
/// transport. The single place backends are named; a new device family adds
/// one arm and one `*-device` crate. Errors on an unknown backend.
///
/// The reMarkable transport prefers a stored device token (paired via
/// [`crate::pair`]); it falls back to `RM_CLOUD_*` env vars for CI / one-shot use.
pub fn resolve_transport(
    backend: &str,
    folder: String,
    secrets: &SecretStore,
) -> Result<Box<dyn DeviceTransport>> {
    match backend {
        "remarkable" => Ok(Box::new(CloudTransport::from_secrets(secrets, folder)?)),
        other => Err(Error::Config(format!("unknown deploy backend {other:?}"))),
    }
}

/// Render the app's document set and push every document over the given transport.
pub async fn publish<M, Msg, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    transport: &dyn DeviceTransport,
) -> Result<()> {
    let mut set = DocSet::default();
    sync::publish(app, &mut set, transport).await
}

/// Pull device ink, fold one cycle, and apply the resulting ops over the transport.
pub async fn sync_once<M, Msg: Clone, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    transport: &dyn DeviceTransport,
) -> Result<Cycle<Msg>> {
    let mut set = DocSet::default();
    sync::sync_once(app, &mut set, transport).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_store() -> (tempfile::TempDir, SecretStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = SecretStore::open(dir.path().join("secrets.json")).unwrap();
        (dir, store)
    }

    #[test]
    fn resolve_routes_known_and_rejects_unknown_backends() {
        let (_d, secrets) = empty_store();

        // Unknown backend → clear config error.
        match resolve_transport("supernote", "/X".into(), &secrets) {
            Err(e) => assert!(
                e.to_string().contains("unknown deploy backend"),
                "unexpected error: {e}"
            ),
            Ok(_) => panic!("an unknown backend must not resolve"),
        }

        // Known "remarkable": with no credentials in store OR env, this MUST fail —
        // but with a credential error, NOT the unknown-backend error.
        // SAFETY: single-threaded test; env vars cleared.
        std::env::remove_var("RM_CLOUD_DEVICE_TOKEN");
        std::env::remove_var("RM_CLOUD_USER_TOKEN");
        if let Err(e) = resolve_transport("remarkable", "/X".into(), &secrets) {
            assert!(
                !e.to_string().contains("unknown deploy backend"),
                "remarkable should be a known backend, got: {e}"
            );
        }
    }
}
```

- [ ] **Step 2: Update `crates/inkapp/src/lib.rs`** — append the pairing re-exports near the existing `pub use rm_device::Remarkable;`:

```rust
pub use rm_device::Remarkable;
pub use rm_device::{pair, resolve_credentials, CloudTransport, REMARKABLE_DEVICE_AUTH_NAME};

pub mod cli;  // <-- added next task; declare now to avoid a follow-up touch
```

Note: if `pub mod cli;` is added now without `cli.rs` existing, the build fails. Defer that ONE line to Task 4. For this task, only add the `pub use rm_device::{pair, ...};` line.

- [ ] **Step 3: Build + test the inkapp crate**

```
nix develop -c cargo test -p inkapp --lib
```
Expect: green. (`--lib` skips the integration tests/cli.rs which doesn't exist yet.)

- [ ] **Step 4: Clippy + fmt**

```
nix develop -c cargo clippy -p inkapp --lib -- -D warnings && nix develop -c cargo fmt
```

- [ ] **Step 5: Clear task, commit**

```
git add crates/inkapp/src/lib.rs crates/inkapp/src/deploy.rs
git commit -m "inkapp: thread SecretStore through resolve_transport; re-export pair"
```

---

### Task 4: `inkapp::cli` module — `OpCmd`, `SecretCmd`, `run`

**Goal:** A new operator CLI in the `inkapp` facade with `pair <code>` and `secret set|list` subcommands.

**Files:**
- Create: `crates/inkapp/src/cli.rs`
- Modify: `crates/inkapp/src/lib.rs` (add `pub mod cli;`)
- Create: `crates/inkapp/tests/cli.rs`

**Acceptance Criteria:**
- [ ] `OpCmd::try_parse_from(["op", "pair", "ABCD1234"])` works.
- [ ] `OpCmd::try_parse_from(["op", "secret", "set", "connector", "readwise-reader", "tok"])` works.
- [ ] `inkapp::cli::run(SecretCmd::Set { scope: Connector, name: "x", value: "y" }, path)` writes the value, reopen returns `b"y"`.
- [ ] `secret list` after a `set` prints `connector  x` and never the value.

**Verify:** `nix develop -c cargo test -p inkapp --test cli` → green.

**Steps:**

- [ ] **Step 1: Confirm `clap` is a dep of `inkapp`.** Today `inkapp` deps `inkapp-config` with `features = ["cli"]` which pulls clap transitively — but transitive deps are NOT usable directly. We must add `clap` to `crates/inkapp/Cargo.toml` `[dependencies]`:

Append to `[dependencies]` in `crates/inkapp/Cargo.toml`:
```toml
clap = { version = "4", features = ["derive"] }
```

- [ ] **Step 2: Create `crates/inkapp/src/cli.rs`**

```rust
//! Operator-setup CLI for inkapp apps: pair the device, store secrets.
//!
//! Mounted by app binaries under a top-level `Op` subcommand. Device-neutral
//! by design *except* the `Pair` arm, which calls reMarkable-specific pairing
//! via [`rm_device::pair`] (re-exported as [`crate::pair`]).

use std::path::PathBuf;

use clap::{Subcommand, ValueEnum};

use inkapp_core::secrets::{Scope, SecretStore};
use rm_cloud::Config;

/// Top-level operator commands.
#[derive(Subcommand, Debug)]
pub enum OpCmd {
    /// Pair this machine with a reMarkable using an 8-char code from
    /// https://my.remarkable.com/device/desktop/connect.
    Pair {
        /// The 8-character one-time code shown in the browser.
        code: String,
    },
    /// Manage the per-user secret store (`secrets.json`).
    #[command(subcommand)]
    Secret(SecretCmd),
}

/// Subcommands of `secret`.
#[derive(Subcommand, Debug)]
pub enum SecretCmd {
    /// Store a secret. Scope is `connector` or `device-auth`.
    Set {
        #[arg(value_enum)]
        scope: ScopeArg,
        name: String,
        value: String,
    },
    /// List `(scope, name)` pairs in the store. Values are never printed.
    List,
}

/// CLI-facing subset of [`Scope`] — `UserKey` is deliberately not exposed.
#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum ScopeArg {
    Connector,
    DeviceAuth,
}

impl From<ScopeArg> for Scope {
    fn from(s: ScopeArg) -> Self {
        match s {
            ScopeArg::Connector => Scope::ConnectorCred,
            ScopeArg::DeviceAuth => Scope::DeviceAuth,
        }
    }
}

/// Dispatch an operator command against the secret store at `secrets_path`.
/// Returns a process exit code (0 = success).
pub async fn run(cmd: OpCmd, secrets_path: PathBuf) -> std::io::Result<i32> {
    match cmd {
        OpCmd::Pair { code } => {
            let mut secrets = SecretStore::open(&secrets_path)
                .map_err(|e| std::io::Error::other(format!("open secrets: {e}")))?;
            let config = Config::from_env();
            match crate::pair(&mut secrets, &config, &code).await {
                Ok(()) => {
                    println!("paired: device token stored at {}", secrets_path.display());
                    Ok(0)
                }
                Err(e) => {
                    eprintln!("pair failed: {e}");
                    Ok(1)
                }
            }
        }
        OpCmd::Secret(SecretCmd::Set { scope, name, value }) => {
            let mut secrets = SecretStore::open(&secrets_path)
                .map_err(|e| std::io::Error::other(format!("open secrets: {e}")))?;
            secrets
                .set(scope.into(), &name, value.as_bytes())
                .map_err(|e| std::io::Error::other(format!("write secret: {e}")))?;
            println!("stored {scope:?} / {name}");
            Ok(0)
        }
        OpCmd::Secret(SecretCmd::List) => {
            // Re-opening to avoid a long-lived store handle; reads are cheap.
            let secrets = SecretStore::open(&secrets_path)
                .map_err(|e| std::io::Error::other(format!("open secrets: {e}")))?;
            // Iterate by trying each CLI-visible scope.
            for (scope_arg, label) in [
                (ScopeArg::Connector, "connector"),
                (ScopeArg::DeviceAuth, "device-auth"),
            ] {
                for name in secrets.names(scope_arg.into()) {
                    println!("{label}  {name}");
                }
            }
            Ok(0)
        }
    }
}
```

The `list` arm uses a method `SecretStore::names(scope) -> Vec<String>` that does NOT exist yet. Add it in the same task:

- [ ] **Step 3: Add `SecretStore::names` in `crates/inkapp-core/src/secrets.rs`**

After the existing `set` method:

```rust
/// Names stored under `scope`. Values are NOT returned (operator listing).
pub fn names(&self, scope: Scope) -> Vec<String> {
    self.data.section(scope).keys().cloned().collect()
}
```

- [ ] **Step 4: Wire `pub mod cli;` in `crates/inkapp/src/lib.rs`**

Append:
```rust
pub mod cli;
```

- [ ] **Step 5: Create `crates/inkapp/tests/cli.rs`**

```rust
//! Parse + behavior tests for the operator CLI (`pair`, `secret`).

use clap::Parser;
use inkapp::cli::{run, OpCmd, ScopeArg, SecretCmd};
use inkapp_core::secrets::{Scope, SecretStore};

#[derive(Parser, Debug)]
#[command(name = "test-op")]
struct TestCli {
    #[command(subcommand)]
    op: OpCmd,
}

#[test]
fn parses_pair_code() {
    let cli = TestCli::try_parse_from(["test-op", "pair", "ABCD1234"]).unwrap();
    match cli.op {
        OpCmd::Pair { code } => assert_eq!(code, "ABCD1234"),
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn parses_secret_set_connector() {
    let cli = TestCli::try_parse_from([
        "test-op",
        "secret",
        "set",
        "connector",
        "readwise-reader",
        "rw-token-xyz",
    ])
    .unwrap();
    match cli.op {
        OpCmd::Secret(SecretCmd::Set { scope, name, value }) => {
            assert!(matches!(scope, ScopeArg::Connector));
            assert_eq!(name, "readwise-reader");
            assert_eq!(value, "rw-token-xyz");
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn parses_secret_set_device_auth() {
    let cli = TestCli::try_parse_from([
        "test-op", "secret", "set", "device-auth", "remarkable", "tok",
    ])
    .unwrap();
    assert!(matches!(
        cli.op,
        OpCmd::Secret(SecretCmd::Set {
            scope: ScopeArg::DeviceAuth,
            ..
        })
    ));
}

#[tokio::test]
async fn secret_set_round_trips_through_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");
    run(
        OpCmd::Secret(SecretCmd::Set {
            scope: ScopeArg::Connector,
            name: "readwise-reader".into(),
            value: "tok".into(),
        }),
        path.clone(),
    )
    .await
    .unwrap();

    let s = SecretStore::open(&path).unwrap();
    assert_eq!(
        s.get(Scope::ConnectorCred, "readwise-reader")
            .unwrap()
            .unwrap(),
        b"tok"
    );
}

#[tokio::test]
async fn secret_list_returns_zero_after_set() {
    // We don't capture stdout here — just confirm exit code and that List
    // doesn't error on a populated store.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.json");
    run(
        OpCmd::Secret(SecretCmd::Set {
            scope: ScopeArg::DeviceAuth,
            name: "remarkable".into(),
            value: "tok".into(),
        }),
        path.clone(),
    )
    .await
    .unwrap();
    let code = run(OpCmd::Secret(SecretCmd::List), path).await.unwrap();
    assert_eq!(code, 0);
}
```

- [ ] **Step 6: Run tests**

```
nix develop -c cargo test -p inkapp-core --lib names      # confirms new method
nix develop -c cargo test -p inkapp --test cli
```

- [ ] **Step 7: Clippy + fmt**

```
nix develop -c cargo clippy -p inkapp --all-targets -- -D warnings
nix develop -c cargo clippy -p inkapp-core --all-targets -- -D warnings
nix develop -c cargo fmt
```

- [ ] **Step 8: Clear task, commit**

```
git add crates/inkapp-core/src/secrets.rs crates/inkapp/src/cli.rs crates/inkapp/src/lib.rs crates/inkapp/tests/cli.rs crates/inkapp/Cargo.toml
git commit -m "inkapp: operator CLI (pair, secret set/list)"
```

(Do NOT stage `Cargo.lock`.)

---

### Task 5: App mains — top-level `Command` enum

**Goal:** Restructure both app binaries to a top-level `Command` enum with `Config` + `Op` arms; thread `&secrets` into `resolve_transport`.

**Files:**
- Modify: `apps/reading-queue/src/main.rs`
- Modify: `apps/agenda/src/main.rs`

**Acceptance Criteria:**
- [ ] `cargo build -p reading-queue -p agenda` succeeds.
- [ ] `<app> --help` lists `config` and `op` subcommands.
- [ ] Running `<app>` with no subcommand still proceeds to publish (existing behavior preserved when env or stored creds present).

**Verify:** `nix develop -c cargo build -p reading-queue -p agenda && nix develop -c cargo test --workspace`.

**Steps:**

- [ ] **Step 1: Rewrite `apps/reading-queue/src/main.rs`**

```rust
//! Assemble and run the reading-queue app from configuration.
//!
//! Subcommands:
//!   - `config ...`  — TOML config CLI (inkapp_config).
//!   - `op pair|secret ...` — operator setup (pair device, store secrets).
//! No subcommand: resolve the configured instance, render docs, publish to the device.

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
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// TOML config-file operations.
    #[command(subcommand)]
    Config(inkapp_config::cli::ConfigCmd),
    /// Operator setup: pair device, manage secrets.
    /// (Sibling worktrees add their own arms here — keep additive.)
    #[command(subcommand)]
    Op(inkapp::cli::OpCmd),
}

#[tokio::main]
async fn main() {
    let args = Cli::parse();
    let cfg_path = ConfigStore::default_path().expect("config path");

    match args.command {
        Some(Command::Config(cmd)) => {
            let code = cli::run(cmd, cfg_path).expect("config command");
            std::process::exit(code);
        }
        Some(Command::Op(op)) => {
            let secrets_path = SecretStore::default_path().expect("secrets path");
            let code = inkapp::cli::run(op, secrets_path)
                .await
                .expect("op command");
            std::process::exit(code);
        }
        None => {}
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

    let transport =
        inkapp::resolve_transport(&device.backend, app_cfg.device_folder.clone(), &secrets)
            .expect("resolve device transport");
    inkapp::publish(&mut application, transport.as_ref())
        .await
        .expect("publish to device");
    println!(
        "reading-queue[{instance}]: published to {} ({})",
        app_cfg.device_folder, device.backend
    );
}
```

Note: `SecretStore::default_path()` is currently private (`fn`, not `pub fn`) in `inkapp-core/src/secrets.rs`. Make it `pub`:

- [ ] **Step 2: Make `SecretStore::default_path` public**

In `crates/inkapp-core/src/secrets.rs`, change `fn default_path()` → `pub fn default_path()`. Comment lightly: "Public so binaries can locate the same file the store opens by default."

- [ ] **Step 3: Apply the same restructure to `apps/agenda/src/main.rs`**

Differences vs reading-queue:
- `use agenda::{update, view, App, AppConfig, Connectors};`
- `#[command(name = "agenda")]`
- Connectors is synchronous: `let connectors = Connectors::from_config(&store, &app_cfg).expect("wire connectors from config");` (no `.await`, no `&secrets`, no `cache_dir`).
- Final `println!` says `agenda[{instance}]: published to ...`.

All other code (Cli/Command enums, subcommand dispatch, secrets/key/transport/publish) is identical.

- [ ] **Step 4: Build + run workspace tests**

```
nix develop -c cargo build -p reading-queue -p agenda
nix develop -c cargo test --workspace
```

If `tests/device.rs` in either app fails to compile because of the `resolve_transport` signature, update those test files too — add a `SecretStore` (tempfile) and pass it. (Most likely they call `resolve_transport` or `publish` with similar patterns.)

- [ ] **Step 5: Clippy + fmt**

```
nix develop -c cargo clippy --workspace --all-targets -- -D warnings
nix develop -c cargo fmt
```

- [ ] **Step 6: Clear task, commit (do NOT stage Cargo.lock)**

```
git add apps/reading-queue/src/main.rs apps/agenda/src/main.rs crates/inkapp-core/src/secrets.rs
# Plus any apps/*/tests/device.rs updates that were needed:
git add apps/reading-queue/tests/device.rs apps/agenda/tests/device.rs 2>/dev/null || true
git commit -m "apps: top-level Command enum with Op subcommand"
```

---

### Task 6: `docs/appdx.md` — record the pairing capability

**Goal:** Update the developer-experience spec to reflect that pairing is now native; env vars are a fallback.

**Files:**
- Modify: `docs/appdx.md`

**Acceptance Criteria:**
- [ ] A subsection (or paragraph) under the deployment / build-order area documents `app pair <code>` and `app secret set connector <name> <token>` as the fresh-machine flow.
- [ ] Notes `RM_CLOUD_DEVICE_TOKEN` / `RM_CLOUD_USER_TOKEN` remain supported as a fallback.
- [ ] Marks the device-pairing item built (matching the appdx convention of striking items off the build-order list).

**Verify:** `nix develop -c cargo test --workspace` still green; visual inspection of the diff.

**Steps:**

- [ ] **Step 1: Locate the right anchor in `docs/appdx.md`**

```
grep -n -E "RM_CLOUD_|pair|device.*token|deploy" docs/appdx.md | head -30
```

Find the deployment-recipe / build-order section that today mentions setting `RM_CLOUD_*` env vars (or the build-order item for device pairing).

- [ ] **Step 2: Edit in place**

Replace the env-var instruction with (adapt wording to the surrounding voice):

```
**Fresh machine setup.** Once per machine, pair the device and store any
connector secrets:

```sh
app pair ABCD1234                                  # 8-char code from
                                                   # my.remarkable.com/device/desktop/connect
app secret set connector readwise-reader <token>   # for the readwise reader
```

The device token is persisted to `~/.config/inkapp/secrets.json` (mode `0600`)
under `Scope::DeviceAuth / "remarkable"`. `RM_CLOUD_DEVICE_TOKEN` and
`RM_CLOUD_USER_TOKEN` env vars remain supported as a fallback for CI or
one-shot use.
```

If there is a build-order checklist, mark the device-pairing line as done in the existing style (e.g. `[x]` or struck through).

- [ ] **Step 3: Full workspace test (sanity)**

```
nix develop -c cargo test --workspace
```

- [ ] **Step 4: Final fmt-check (what the pre-commit hook runs)**

```
nix develop -c cargo fmt --check
nix develop -c cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 5: Clear `.tasks.json` of completed tasks; commit**

The plan's sibling `.tasks.json` must show no `pending`/`in_progress` rows for this work before the pre-commit hook will allow a commit. Update statuses to `completed`, then:

```
git add docs/appdx.md
git commit -m "appdx: record native device pairing (replaces RM_CLOUD_* env vars)"
git log --oneline -8
```

---

## Self-Review

**Spec coverage** — every spec section maps to a task:

| Spec section | Task |
|--------------|------|
| Public surface: `rm-device::auth` (const, `pair`, `resolve_credentials`) | 0, 1 |
| Public surface: `CloudTransport::from_secrets` | 2 |
| `inkapp` re-exports + `deploy::resolve_transport(secrets)` | 3 |
| `inkapp::cli` module (OpCmd, SecretCmd, ScopeArg, run) | 4 |
| App CLI restructure (top-level Command enum) | 5 |
| Credentials precedence table | covered by Task 0 unit tests |
| Tests (pairing fake, credentials, cli) | 0, 1, 2, 4 |
| docs/appdx update | 6 |

**Placeholder scan** — none (no TBD/TODO/"appropriate"/"similar to"; all code shown).

**Type consistency** — `pair(secrets, config, code)` is the signature in Task 0, 1, 4 (CLI handler). `resolve_with`/`resolve_credentials` signatures match across Task 0 and Task 2. `SecretStore::names` introduced in Task 4 and used only there. `default_path` made `pub` in Task 5 step 2 and used in Task 5 step 1.

**Two callouts the implementer should respect:**

1. In Task 0 step 3, the example file ends with a `#[doc(hidden)] pub use inkapp_core as _unused_marker;` line. **Delete it** — it was a copy-paste guard; `inkapp_core` is already used via `Scope`/`SecretStore`, so no marker is needed. The "Final imports should be" block immediately below shows the correct top-of-file imports.

2. In Task 5 step 4, if `apps/*/tests/device.rs` still type-checks (because it doesn't call `resolve_transport` directly), no change is needed there — the `git add ... 2>/dev/null || true` handles either case.
