# Secrets store + encryption ("no cleartext tier") Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `appdx.md`'s "no cleartext tier" real — embed the structural manifest into PDFs as ciphertext sealed with a per-user key drawn from a file-backed secrets store.

**Architecture:** Two new `inkapp-core` modules — `crypto` (XChaCha20-Poly1305 `seal`/`open` over a 32-byte `Key`) and `secrets` (a file-backed `SecretStore` with three scopes that mints+persists the per-user key). `embed.rs` seals the manifest JSON before writing it to the PDF Info dictionary (as a hex string) and opens it on extract. The `Key` threads through `render_document` and `App`; the builder gains a type-state `.key(Key)` before `.build()`. Downstream crates (harness, facade, reading-queue) pass a key explicitly.

**Tech Stack:** Rust, `chacha20poly1305` (RustCrypto, pure-Rust AEAD), `getrandom` (CSPRNG), `base64` (secrets-file byte values), `lopdf` (PDF Info dict), `serde_json`.

---

### Task 1: `crypto` module — `Key`, `seal`, `open`

**Goal:** A self-contained AEAD seam: `seal(&Key, &[u8]) -> Vec<u8>` (nonce‖ciphertext‖tag) and `open(&Key, &[u8]) -> Result<Vec<u8>>`, with a typed error on any failure.

**Files:**
- Create: `crates/inkapp-core/src/crypto.rs`
- Modify: `crates/inkapp-core/src/error.rs` (add `Crypto` variant)
- Modify: `crates/inkapp-core/src/lib.rs` (add `pub mod crypto;` + re-exports)
- Modify: `crates/inkapp-core/Cargo.toml` (add `chacha20poly1305`, `getrandom`)
- Test: `crates/inkapp-core/src/crypto.rs` (inline `#[cfg(test)]`)

**Acceptance Criteria:**
- [ ] `seal` then `open` with the same key returns the original plaintext.
- [ ] `open` with a different key returns `Err(Error::Crypto(_))`, no panic.
- [ ] `open` on tampered bytes (flip one byte) returns `Err(Error::Crypto(_))`.
- [ ] Two `seal` calls on identical plaintext+key produce different outputs (random nonce).
- [ ] `Key::from_bytes([u8; 32])` constructs a key.

**Verify:** `nix develop -c cargo test -p inkapp-core crypto:: -- --nocapture` → all pass

**Steps:**

- [ ] **Step 1: Add dependencies**

In `crates/inkapp-core/Cargo.toml`, under `[dependencies]` add:

```toml
chacha20poly1305 = "0.10"
getrandom = "0.2"
```

- [ ] **Step 2: Add the `Crypto` error variant**

In `crates/inkapp-core/src/error.rs`, add to the `Error` enum (after `Readback`):

```rust
    #[error("encryption/decryption failed: {0}")]
    Crypto(String),
```

- [ ] **Step 3: Write the failing tests (in the new file)**

Create `crates/inkapp-core/src/crypto.rs` with the test module first:

```rust
//! The encryption seam: AEAD `seal`/`open` over a per-user [`Key`]. Everything
//! the framework embeds in a PDF (the manifest, and later per-component state)
//! goes through here, so the device — and any third party we share a PDF with —
//! sees only ciphertext. The framework holds the key (from the secrets store)
//! and is the only reader.

use chacha20poly1305::aead::Aead;
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};

use crate::error::{Error, Result};

/// XChaCha20-Poly1305 uses a 24-byte nonce; we prepend it to each ciphertext.
const NONCE_LEN: usize = 24;

/// A 32-byte symmetric key. Construct from raw bytes (tests / advanced callers)
/// or obtain the per-user key from [`crate::secrets::SecretStore`].
#[derive(Clone)]
pub struct Key([u8; 32]);

impl Key {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Key(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Seal `plaintext` under `key`. Output is `nonce (24B) ‖ ciphertext ‖ tag`.
pub fn seal(key: &Key, plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new((&key.0).into());
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce).map_err(|e| Error::Crypto(e.to_string()))?;
    let xnonce = XNonce::from_slice(&nonce);
    let ct = cipher
        .encrypt(xnonce, plaintext)
        .map_err(|e| Error::Crypto(e.to_string()))?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Open a blob produced by [`seal`]. Verifies the auth tag; a wrong key or any
/// tampering yields `Error::Crypto`.
pub fn open(key: &Key, sealed: &[u8]) -> Result<Vec<u8>> {
    if sealed.len() < NONCE_LEN {
        return Err(Error::Crypto("sealed blob shorter than nonce".into()));
    }
    let (nonce, ct) = sealed.split_at(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new((&key.0).into());
    cipher
        .decrypt(XNonce::from_slice(nonce), ct)
        .map_err(|e| Error::Crypto(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_a() -> Key {
        Key::from_bytes([7u8; 32])
    }

    #[test]
    fn round_trips() {
        let pt = b"the queue lives in Readwise";
        let sealed = seal(&key_a(), pt).unwrap();
        assert_eq!(open(&key_a(), &sealed).unwrap(), pt);
    }

    #[test]
    fn wrong_key_fails() {
        let sealed = seal(&key_a(), b"secret").unwrap();
        let other = Key::from_bytes([8u8; 32]);
        assert!(matches!(open(&other, &sealed), Err(Error::Crypto(_))));
    }

    #[test]
    fn tampering_fails() {
        let mut sealed = seal(&key_a(), b"secret").unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 0xff;
        assert!(matches!(open(&key_a(), &sealed), Err(Error::Crypto(_))));
    }

    #[test]
    fn nonce_randomizes_output() {
        let a = seal(&key_a(), b"same").unwrap();
        let b = seal(&key_a(), b"same").unwrap();
        assert_ne!(a, b, "each seal must use a fresh random nonce");
    }
}
```

- [ ] **Step 4: Register the module + exports**

In `crates/inkapp-core/src/lib.rs`, add `pub mod crypto;` to the module list (after `pub mod core`-adjacent entries, e.g. after `pub mod component;`), and add to the re-export block:

```rust
pub use crypto::{open, seal, Key};
```

- [ ] **Step 5: Run tests**

Run: `nix develop -c cargo test -p inkapp-core crypto:: -- --nocapture`
Expected: 4 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/inkapp-core/src/crypto.rs crates/inkapp-core/src/error.rs crates/inkapp-core/src/lib.rs crates/inkapp-core/Cargo.toml Cargo.lock
git commit -m "inkapp-core: crypto module (XChaCha20-Poly1305 seal/open over a 32-byte Key)"
```

---

### Task 2: `secrets` module — `SecretStore`, three scopes, per-user key

**Goal:** A single-user, file-backed secrets store with three scopes (connector creds, device auth, user key); `user_key()` mints+persists a random per-user `Key` on first use.

**Files:**
- Create: `crates/inkapp-core/src/secrets.rs`
- Modify: `crates/inkapp-core/src/error.rs` (add `Secrets` variant)
- Modify: `crates/inkapp-core/src/lib.rs` (add `pub mod secrets;` + re-exports)
- Modify: `crates/inkapp-core/Cargo.toml` (add `base64`; dev-dep `tempfile`)
- Test: `crates/inkapp-core/src/secrets.rs` (inline `#[cfg(test)]`)

**Acceptance Criteria:**
- [ ] `set`/`get` round-trips for all three scopes across a close+reopen of the same path.
- [ ] `user_key()` returns a stable key across reopen (persisted), and distinct keys for distinct paths.
- [ ] The backing file is created with mode `0600` (unix).
- [ ] `open_default()` honors the `INKAPP_SECRETS_PATH` env override.

**Verify:** `nix develop -c cargo test -p inkapp-core secrets:: -- --nocapture` → all pass

**Steps:**

- [ ] **Step 1: Add dependencies**

In `crates/inkapp-core/Cargo.toml`, add to `[dependencies]`:

```toml
base64 = "0.22"
```

And add a dev-dependencies section (or extend it) at the end of the file:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Add the `Secrets` error variant**

In `crates/inkapp-core/src/error.rs`, add to the `Error` enum:

```rust
    #[error("secret store failed: {0}")]
    Secrets(String),
```

- [ ] **Step 3: Write the module with failing tests**

Create `crates/inkapp-core/src/secrets.rs`:

```rust
//! The per-user secrets/config store. Three scopes — connector credentials,
//! per-device auth, and the per-user encryption key — persisted to a single
//! `0600` JSON file. Single-user and plaintext-on-disk for now; at-rest
//! protection, KMS, rotation, and tenant isolation are future (see appdx).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::crypto::Key;
use crate::error::{Error, Result};

/// Where a secret lives. Each maps to a top-level section in the store file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    ConnectorCred,
    DeviceAuth,
    UserKey,
}

impl Scope {
    fn section(self) -> &'static str {
        match self {
            Scope::ConnectorCred => "connector_cred",
            Scope::DeviceAuth => "device_auth",
            Scope::UserKey => "user_key",
        }
    }
}

/// The fixed name under which the per-user key is stored in `UserKey`.
const USER_KEY_NAME: &str = "default";

/// On-disk shape: section -> name -> base64(bytes).
#[derive(Default, Serialize, Deserialize)]
struct Data {
    #[serde(default)]
    connector_cred: BTreeMap<String, String>,
    #[serde(default)]
    device_auth: BTreeMap<String, String>,
    #[serde(default)]
    user_key: BTreeMap<String, String>,
}

impl Data {
    fn section_mut(&mut self, scope: Scope) -> &mut BTreeMap<String, String> {
        match scope {
            Scope::ConnectorCred => &mut self.connector_cred,
            Scope::DeviceAuth => &mut self.device_auth,
            Scope::UserKey => &mut self.user_key,
        }
    }
    fn section(&self, scope: Scope) -> &BTreeMap<String, String> {
        match scope {
            Scope::ConnectorCred => &self.connector_cred,
            Scope::DeviceAuth => &self.device_auth,
            Scope::UserKey => &self.user_key,
        }
    }
}

/// A file-backed secret store.
pub struct SecretStore {
    path: PathBuf,
    data: Data,
}

impl SecretStore {
    /// Open (or initialize) the store at `path`. A missing file is treated as
    /// empty; it is created on the first write.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let data = match std::fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|e| Error::Secrets(e.to_string()))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Data::default(),
            Err(e) => return Err(Error::Secrets(e.to_string())),
        };
        Ok(Self { path, data })
    }

    /// Open the store at the default location: `$INKAPP_SECRETS_PATH`, else
    /// `$XDG_CONFIG_HOME/inkapp/secrets.json`, else `$HOME/.config/inkapp/secrets.json`.
    pub fn open_default() -> Result<Self> {
        Self::open(Self::default_path()?)
    }

    fn default_path() -> Result<PathBuf> {
        if let Ok(p) = std::env::var("INKAPP_SECRETS_PATH") {
            return Ok(PathBuf::from(p));
        }
        let base = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            PathBuf::from(xdg)
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".config")
        } else {
            return Err(Error::Secrets("no HOME or XDG_CONFIG_HOME set".into()));
        };
        Ok(base.join("inkapp").join("secrets.json"))
    }

    /// Fetch a secret's raw bytes.
    pub fn get(&self, scope: Scope, name: &str) -> Option<Vec<u8>> {
        self.data
            .section(scope)
            .get(name)
            .and_then(|b64| base64::engine::general_purpose::STANDARD.decode(b64).ok())
    }

    /// Store a secret and persist the file.
    pub fn set(&mut self, scope: Scope, name: &str, value: &[u8]) -> Result<()> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(value);
        self.data.section_mut(scope).insert(name.to_string(), b64);
        self.persist()
    }

    /// The per-user encryption key, generated and persisted on first call.
    pub fn user_key(&mut self) -> Result<Key> {
        if let Some(bytes) = self.get(Scope::UserKey, USER_KEY_NAME) {
            let arr: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| Error::Secrets("stored user key is not 32 bytes".into()))?;
            return Ok(Key::from_bytes(arr));
        }
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).map_err(|e| Error::Secrets(e.to_string()))?;
        self.set(Scope::UserKey, USER_KEY_NAME, &bytes)?;
        Ok(Key::from_bytes(bytes))
    }

    fn persist(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| Error::Secrets(e.to_string()))?;
            }
        }
        let json = serde_json::to_vec_pretty(&self.data).map_err(|e| Error::Secrets(e.to_string()))?;
        std::fs::write(&self.path, &json).map_err(|e| Error::Secrets(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| Error::Secrets(e.to_string()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        (dir, path)
    }

    #[test]
    fn set_get_round_trips_all_scopes_across_reopen() {
        let (_d, path) = tmp();
        {
            let mut s = SecretStore::open(&path).unwrap();
            s.set(Scope::ConnectorCred, "readwise", b"tok").unwrap();
            s.set(Scope::DeviceAuth, "remarkable", b"auth").unwrap();
            s.set(Scope::UserKey, "default", b"k").unwrap();
        }
        let s = SecretStore::open(&path).unwrap();
        assert_eq!(s.get(Scope::ConnectorCred, "readwise").unwrap(), b"tok");
        assert_eq!(s.get(Scope::DeviceAuth, "remarkable").unwrap(), b"auth");
        assert_eq!(s.get(Scope::UserKey, "default").unwrap(), b"k");
    }

    #[test]
    fn user_key_is_stable_across_reopen() {
        let (_d, path) = tmp();
        let first = SecretStore::open(&path).unwrap().user_key().unwrap();
        let second = SecretStore::open(&path).unwrap().user_key().unwrap();
        assert_eq!(first.as_bytes(), second.as_bytes());
    }

    #[test]
    fn user_key_distinct_per_path() {
        let (_d1, p1) = tmp();
        let (_d2, p2) = tmp();
        let a = SecretStore::open(&p1).unwrap().user_key().unwrap();
        let b = SecretStore::open(&p2).unwrap().user_key().unwrap();
        assert_ne!(a.as_bytes(), b.as_bytes());
    }

    #[cfg(unix)]
    #[test]
    fn file_is_0600() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, path) = tmp();
        let mut s = SecretStore::open(&path).unwrap();
        s.set(Scope::ConnectorCred, "x", b"y").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn open_default_honors_env_override() {
        let (_d, path) = tmp();
        std::env::set_var("INKAPP_SECRETS_PATH", &path);
        let mut s = SecretStore::open_default().unwrap();
        s.set(Scope::ConnectorCred, "x", b"y").unwrap();
        std::env::remove_var("INKAPP_SECRETS_PATH");
        assert!(path.exists());
    }
}
```

- [ ] **Step 4: Register the module + exports**

In `crates/inkapp-core/src/lib.rs`, add `pub mod secrets;` and:

```rust
pub use secrets::{Scope, SecretStore};
```

- [ ] **Step 5: Run tests**

Run: `nix develop -c cargo test -p inkapp-core secrets:: -- --nocapture`
Expected: all secrets tests pass. (The `open_default` test sets/removes a process env var; it is fine in the default single-threaded-per-test cargo harness, but if flakiness appears under parallelism, that is addressed by the env var being set+read+removed within one test.)

- [ ] **Step 6: Commit**

```bash
git add crates/inkapp-core/src/secrets.rs crates/inkapp-core/src/error.rs crates/inkapp-core/src/lib.rs crates/inkapp-core/Cargo.toml Cargo.lock
git commit -m "inkapp-core: file-backed SecretStore (three scopes, per-user key, 0600)"
```

---

### Task 3: Encrypt the embedded manifest + thread `Key` through the runtime

**Goal:** `embed_manifest`/`extract_manifest` seal/open the manifest (stored as a PDF hex string); `render_document` and `App` carry a `Key`; the builder gains `.key(Key) -> BuilderReady`. The `inkapp-core` crate compiles and all its tests pass with the manifest encrypted.

**Files:**
- Modify: `crates/inkapp-core/src/embed.rs`
- Modify: `crates/inkapp-core/src/runtime.rs` (`render_document`, `App::new`, `App::render`, `App::step`, builder)
- Modify: `crates/inkapp-core/tests/embed.rs`
- Modify: `crates/inkapp-core/tests/render_walk.rs`
- Modify: `crates/inkapp-core/tests/loop_driver.rs`

**Acceptance Criteria:**
- [ ] `embed_manifest(pdf, manifest, &key)` writes only sealed bytes; the raw PDF contains none of the region-name substrings.
- [ ] `extract_manifest(pdf, &key)` round-trips the manifest; wrong key → `Err(Error::Crypto(_))`.
- [ ] `render_document(doc, version, &key)` and the builder `.key(k).build()` chain compile.
- [ ] `nix develop -c cargo test -p inkapp-core` is fully green.

**Verify:** `nix develop -c cargo test -p inkapp-core` → all pass

**Steps:**

- [ ] **Step 1: Update `embed.rs` to seal/open (write the new behavior + signatures)**

Replace the body of `crates/inkapp-core/src/embed.rs` with:

```rust
use lopdf::{Dictionary, Document, Object, StringFormat};

use crate::crypto::{open, seal, Key};
use crate::error::{Error, Result};
use crate::manifest::Manifest;

/// Info-dictionary key under which the *sealed* manifest is stored.
const MANIFEST_KEY: &[u8] = b"InkappManifest";

/// Seal the manifest and embed it in the PDF's Info dictionary as a hex string.
/// Nothing readable (region names, version) reaches the PDF.
pub fn embed_manifest(pdf: &[u8], manifest: &Manifest, key: &Key) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(manifest).map_err(|e| Error::Manifest(e.to_string()))?;
    let sealed = seal(key, &json)?;
    let mut doc = Document::load_mem(pdf).map_err(|e| Error::Manifest(e.to_string()))?;

    let info_id = match doc.trailer.get(b"Info") {
        Ok(obj) => obj.as_reference().map_err(|_| {
            Error::Manifest("Info trailer entry is not an indirect reference".into())
        })?,
        Err(_) => {
            let id = doc.add_object(Object::Dictionary(Dictionary::new()));
            doc.trailer.set("Info", Object::Reference(id));
            id
        }
    };
    if let Ok(Object::Dictionary(info)) = doc.get_object_mut(info_id) {
        // Hexadecimal string keeps arbitrary ciphertext bytes binary-safe in PDF.
        info.set(MANIFEST_KEY, Object::String(sealed, StringFormat::Hexadecimal));
    } else {
        return Err(Error::Manifest("Info object is not a dictionary".into()));
    }

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| Error::Manifest(e.to_string()))?;
    Ok(out)
}

/// Extract and open the sealed manifest from the PDF's Info dictionary.
pub fn extract_manifest(pdf: &[u8], key: &Key) -> Result<Manifest> {
    let doc = Document::load_mem(pdf).map_err(|e| Error::Manifest(e.to_string()))?;
    let info_id = match doc.trailer.get(b"Info") {
        Ok(obj) => obj.as_reference().map_err(|_| {
            Error::Manifest("Info trailer entry is not an indirect reference".into())
        })?,
        Err(_) => return Err(Error::Manifest("no Info dictionary".into())),
    };
    let info = doc
        .get_object(info_id)
        .and_then(|o| o.as_dict())
        .map_err(|e| Error::Manifest(e.to_string()))?;
    let sealed = info
        .get(MANIFEST_KEY)
        .and_then(|o| o.as_str())
        .map_err(|e| Error::Manifest(format!("manifest key missing: {e}")))?;
    let json = open(key, sealed)?;
    serde_json::from_slice(&json).map_err(|e| Error::Manifest(e.to_string()))
}
```

- [ ] **Step 2: Thread the key through `runtime.rs`**

In `crates/inkapp-core/src/runtime.rs`:

(a) Update the import line `use crate::embed::embed_manifest;` to also bring in the key:

```rust
use crate::crypto::Key;
use crate::embed::embed_manifest;
```

(b) Change `render_document` to take a key and pass it to `embed_manifest`:

```rust
/// Render one document to a [`RenderedDoc`] at `version`, sealing its manifest
/// with `key`.
pub fn render_document<M>(doc: &Document<M>, version: u64, key: &Key) -> Result<RenderedDoc> {
    let src = document_source(doc);
    let compiled = compile_to_document(&src)?;
    let page_h = compiled
        .pages
        .first()
        .map(|p| p.frame.height().to_pt())
        .unwrap_or(0.0);
    let manifest = recover_regions(&compiled)?.with_version(version);
    let pdf = embed_manifest(&document_to_pdf(&compiled)?, &manifest, key)?;
    Ok(RenderedDoc {
        key: doc.key.clone(),
        pdf,
        manifest,
        page_h,
        hash: hash_str(&src),
    })
}
```

(c) Add a `key` field to `App` and accept it in `App::new`:

```rust
pub struct App<M, Msg, Cx> {
    pub model: M,
    pub connectors: Cx,
    update: UpdateFn<M, Msg, Cx>,
    view: ViewFn<M, Msg, Cx>,
    version: u64,
    key: Key,
}

impl<M, Msg, Cx> App<M, Msg, Cx> {
    pub fn new(
        model: M,
        connectors: Cx,
        update: UpdateFn<M, Msg, Cx>,
        view: ViewFn<M, Msg, Cx>,
        key: Key,
    ) -> Self {
        Self {
            model,
            connectors,
            update,
            view,
            version: 1,
            key,
        }
    }
```

(d) In `App::render`, pass the key:

```rust
            let rd = render_document(doc, self.version, &self.key)?;
```

(e) In `App::step`, pass the key (the single call inside the phase-3 loop):

```rust
            next_rendered.push(render_document(doc, self.version, &self.key)?);
```

- [ ] **Step 3: Add the `.key()` builder type-state**

In `crates/inkapp-core/src/runtime.rs`, replace `BuilderFull`'s `impl` block (the one with `build`) so `.key()` produces a `BuilderReady` that owns `build`:

```rust
impl<M, Msg, Cx> BuilderFull<M, Msg, Cx> {
    /// Supply the per-user key the framework seals manifests with. Tests pass a
    /// fixed `Key::from_bytes(..)`; apps pass `SecretStore::user_key()`.
    pub fn key(self, key: Key) -> BuilderReady<M, Msg, Cx> {
        BuilderReady {
            model: self.model,
            connectors: self.connectors,
            update: self.update,
            view: self.view,
            key,
        }
    }
}

pub struct BuilderReady<M, Msg, Cx> {
    model: M,
    connectors: Cx,
    update: UpdateFn<M, Msg, Cx>,
    view: ViewFn<M, Msg, Cx>,
    key: Key,
}

impl<M, Msg, Cx> BuilderReady<M, Msg, Cx> {
    pub fn build(self) -> App<M, Msg, Cx> {
        App::new(self.model, self.connectors, self.update, self.view, self.key)
    }
}
```

(Remove the old `BuilderFull::build`; `build` now lives only on `BuilderReady`.)

- [ ] **Step 4: Update `tests/embed.rs`**

Edit `crates/inkapp-core/tests/embed.rs`:

(a) Change the import:

```rust
use inkapp_core::crypto::Key;
use inkapp_core::embed::{embed_manifest, extract_manifest};
```

(b) In `manifest_round_trips_through_pdf`, supply a key and assert no cleartext, then round-trip:

```rust
    let key = Key::from_bytes([3u8; 32]);
    let embedded = embed_manifest(&pdf, &manifest, &key).unwrap();
    assert!(embedded.starts_with(b"%PDF"));
    // No-cleartext tier: the region name must not appear in the raw PDF bytes.
    assert!(
        !embedded.windows(4).any(|w| w == b"done"),
        "region name leaked into the PDF in cleartext"
    );
    let got = extract_manifest(&embedded, &key).unwrap();
    assert_eq!(got, manifest);
```

(c) In `extract_from_unembedded_pdf_errors`, pass a key:

```rust
    let key = Key::from_bytes([3u8; 32]);
    assert!(
        extract_manifest(&pdf, &key).is_err(),
        "plain PDF has no manifest to extract"
    );
```

(d) Add a wrong-key test at the end of the file:

```rust
#[test]
fn extract_with_wrong_key_fails() {
    use inkapp_core::error::Error;
    let doc = compile_to_document("#set page(width: 100pt, height: 100pt)\nhi").unwrap();
    let pdf = document_to_pdf(&doc).unwrap();
    let manifest = Manifest { version: 1, regions: vec![] };
    let embedded = embed_manifest(&pdf, &manifest, &Key::from_bytes([1u8; 32])).unwrap();
    let got = extract_manifest(&embedded, &Key::from_bytes([2u8; 32]));
    assert!(matches!(got, Err(Error::Crypto(_))), "wrong key must fail to open");
}
```

- [ ] **Step 5: Update `tests/render_walk.rs`**

In `crates/inkapp-core/tests/render_walk.rs`, add a key import and pass it to every `render_document` and `extract_manifest` call:

```rust
use inkapp_core::crypto::Key;
```

Then, at the three `render_document(&doc(), 1)` sites, change to `render_document(&doc(), 1, &Key::from_bytes([5u8; 32]))`, and at the `extract_manifest(&rd.pdf)` site change to `extract_manifest(&rd.pdf, &Key::from_bytes([5u8; 32]))`. Use the same byte value across the file so extract matches embed. (If the file defines a helper or shares `rd` across assertions, define `let key = Key::from_bytes([5u8; 32]);` once at the top of each test and reuse `&key`.)

- [ ] **Step 6: Update `tests/loop_driver.rs`**

In `crates/inkapp-core/tests/loop_driver.rs`, add `.key(Key::from_bytes([9u8; 32]))` immediately before each `.build()` (two sites), and add the import:

```rust
use inkapp_core::crypto::Key;
```

Example (the `.view(view)` → `.build()` chain becomes):

```rust
        .view(view)
        .key(Key::from_bytes([9u8; 32]))
        .build();
```

- [ ] **Step 7: Run the core crate tests**

Run: `nix develop -c cargo test -p inkapp-core`
Expected: all pass (crypto, secrets, embed incl. no-cleartext + wrong-key, render_walk, loop_driver).

- [ ] **Step 8: Commit**

```bash
git add crates/inkapp-core/src/embed.rs crates/inkapp-core/src/runtime.rs crates/inkapp-core/tests/embed.rs crates/inkapp-core/tests/render_walk.rs crates/inkapp-core/tests/loop_driver.rs
git commit -m "inkapp-core: seal the embedded manifest; thread Key through render_document/App/builder"
```

---

### Task 4: Update downstream crates (harness, facade, reading-queue)

**Goal:** Every crate that embeds/extracts a manifest or builds an `App` supplies a key, so the whole workspace compiles and `make test` is green with encryption on the path. `reading-queue`'s `main` and on-device test source the key from the `SecretStore`.

**Files:**
- Modify: `crates/inkapp-harness/src/recording.rs` (`render_template`, `render_calibration` take `&Key`)
- Modify: `crates/inkapp-harness/tests/common/mod.rs`
- Modify: `crates/inkapp-harness/tests/extract.rs`
- Modify: `crates/inkapp-harness/tests/templates.rs`
- Modify: `crates/inkapp-harness/tests/app_loop.rs`
- Modify: `crates/inkapp/src/lib.rs` (re-export `crypto`, `secrets` items)
- Modify: `apps/reading-queue/src/main.rs`
- Modify: `apps/reading-queue/tests/device.rs`

**Acceptance Criteria:**
- [ ] `crates/inkapp-harness/src/recording.rs` template fns take `key: &Key` and pass it to `embed_manifest`.
- [ ] All harness tests pass a matching key to `extract_manifest`.
- [ ] `reading-queue` `main` and `device.rs` build the app via `SecretStore` + `.key(...)`.
- [ ] `make test` (whole workspace) is green.

**Verify:** `make test` → all pass (excluding the `#[ignore]`-gated manual device tests)

**Steps:**

- [ ] **Step 1: `recording.rs` — thread the key through the two template builders**

In `crates/inkapp-harness/src/recording.rs`:

(a) Update the import:

```rust
use inkapp_core::crypto::Key;
use inkapp_core::embed::embed_manifest;
```

(b) Change `render_template`'s signature and its `embed_manifest` call:

```rust
pub fn render_template(entry: &CatalogEntry, key: &Key) -> Result<Vec<u8>> {
```

...and at its end:

```rust
    embed_manifest(&pdf, &manifest, key)
```

(c) Change `render_calibration` the same way:

```rust
pub fn render_calibration(key: &Key) -> Result<Vec<u8>> {
```

...and at its end:

```rust
    embed_manifest(&pdf, &manifest, key)
```

(d) `synth_calibration` (line ~401) calls `render_calibration` internally. Give it a key param too and forward it:

```rust
pub fn synth_calibration(device: &dyn Device, key: &Key) -> Result<(Vec<u8>, Vec<u8>)> {
```

...and update its internal `render_calibration()` call to `render_calibration(key)`. (If `synth_calibration` has no callers in `src`, only tests call it; update those in the steps below.)

- [ ] **Step 2: Define one shared test key for the harness tests**

In `crates/inkapp-harness/tests/common/mod.rs`, add near the top (after imports):

```rust
use inkapp_core::crypto::Key;

/// Fixed key used across harness tests so embed and extract agree.
pub fn test_key() -> Key {
    Key::from_bytes([42u8; 32])
}
```

Then update the two `extract_manifest(&pdf)` calls in `regen_fixture` to `extract_manifest(&pdf, &test_key())`, and the `render_template(entry)` call to `render_template(entry, &test_key())`.

- [ ] **Step 3: Update the harness test files**

- `crates/inkapp-harness/tests/extract.rs`: the test renders a template and extracts. Use the shared key. Add `use common::test_key;` if `common` is declared (`mod common;`); otherwise define a local `Key::from_bytes([42u8; 32])`. Change `extract_manifest(&pdf)` → `extract_manifest(&pdf, &test_key())` and any `render_template(entry)`/`render_calibration()` → pass `&test_key()`.
- `crates/inkapp-harness/tests/templates.rs`: same — pass `&test_key()` (or a local `[42u8; 32]` key) to both `extract_manifest` calls and to whatever produced the PDFs (`render_template`).
- `crates/inkapp-harness/tests/app_loop.rs`: add `.key(inkapp_core::crypto::Key::from_bytes([42u8; 32]))` before `.build()` (line ~72).

For each file, the rule is: the key passed to `extract_manifest` MUST equal the key used to embed (the template/app key). Using the single `[42u8; 32]` value everywhere in the harness tests satisfies this.

- [ ] **Step 4: Re-export crypto/secrets from the facade**

In `crates/inkapp/src/lib.rs`, extend the re-export to include the new surface so apps can use it as `inkapp::{Key, SecretStore, Scope}`:

```rust
pub use inkapp_core::{
    app, document_source, render_document, App, Cycle, DocSet, Key, RenderedDoc, Scope,
    SecretStore,
};
```

(Keep the existing `Remarkable` and any other re-exports already present; only add `Key`, `Scope`, `SecretStore` to the list.)

- [ ] **Step 5: `reading-queue` `main.rs` — source the key from the store**

In `apps/reading-queue/src/main.rs`, update imports and the builder chain:

```rust
use inkapp::{app, DocSet, SecretStore};
use reading_queue::{update, view, App, Connectors};

fn main() {
    let key = SecretStore::open_default()
        .and_then(|mut s| s.user_key())
        .expect("open secrets store / load user key");
    let mut application = app(App)
        .connector(Connectors::persisted(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/.overlay.json"
        )))
        .update(update)
        .view(view)
        .key(key)
        .build();
    let mut set = DocSet::default();
    let rendered = application.render(&mut set).expect("render");
    println!("reading-queue: rendered {} document(s)", rendered.len());
}
```

- [ ] **Step 6: `reading-queue` `device.rs` test — give `build_app` a key**

In `apps/reading-queue/tests/device.rs`, update `build_app` to source a key. Since this is a manual `#[ignore]` test that round-trips real PDFs through the device across two processes, the key must be stable between runs — so use the default `SecretStore` (same as `main`), not a random one:

```rust
use inkapp::{app, App as Framework, DocSet, Remarkable, SecretStore};
```

```rust
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
```

- [ ] **Step 7: Run the whole workspace test suite**

Run: `make test`
Expected: green. (Check `Makefile` first for the exact command; the manual device tests are `#[ignore]` and excluded.)

- [ ] **Step 8: Commit**

```bash
git add crates/inkapp-harness/src/recording.rs crates/inkapp-harness/tests/ crates/inkapp/src/lib.rs apps/reading-queue/src/main.rs apps/reading-queue/tests/device.rs
git commit -m "harness/facade/reading-queue: pass per-user key through embed/extract + App builds"
```

---

### Task 5: Update `appdx.md` — Encryption section to present tense

**Goal:** Reflect the now-real encryption in `appdx.md`: the Encryption/no-cleartext-tier claims move to present tense, the worked-example `main()` shows sourcing the key from the secrets store, and the build order is recorded. Remaining future items stay explicit.

**Files:**
- Modify: `docs/appdx.md`

**Acceptance Criteria:**
- [ ] The "State → Encryption" and "no cleartext tier" passages no longer read as aspirational for what is built (manifest sealing).
- [ ] The worked-example `main()` shows `SecretStore` + `.key(...)`.
- [ ] A short note records the build order (S→E→C→M→T) and what remains future (mode, connector plugin trait, Typst authoring; and the already-future event sourcing / multi-user).
- [ ] The top "Status: exploratory" banner is updated to note encryption + secrets are now built (not removed wholesale — `mode`, connector-plugin, and Typst authoring are still ahead).

**Verify:** `grep -n "no cleartext tier\|SecretStore\|build order" docs/appdx.md` shows the updated passages; manual read confirms accuracy.

**Steps:**

- [ ] **Step 1: Update the worked-example `main()` (around line 495-503)**

Replace the `fn main()` snippet so the key is sourced from the store:

```rust
fn main() {
    // The framework seals everything it embeds with this per-user key.
    let key = SecretStore::open_default().unwrap().user_key().unwrap();
    inkapp::app(App)
        .connector(Readwise::new(token))   // this is what makes `cx.readwise` exist
        .update(update)
        .view(view)
        .key(key)                          // per-user encryption key
        .run();                            // the framework owns the loop, forever
}
```

(Keep the surrounding prose; add one sentence noting the key comes from the secrets store and seals the embedded manifest/state.)

- [ ] **Step 2: Make the Encryption section present-tense for what's built**

In the "State" section's Encryption passage (around lines 339-354), adjust wording so the *manifest sealing* is described as implemented (it is, after Tasks 1–4): e.g. change framing like "everything embedded is encrypted" to keep the rule but add a concrete note that the structural manifest is sealed with XChaCha20-Poly1305 using the per-user key from `SecretStore`, and that the per-component state field rides the same seam when added. Keep the third-party-share threat framing.

- [ ] **Step 3: Record the build order + update the status banner**

Near the top (the "Status: exploratory" banner, lines 3-6), update to note that the secrets store and embedded-manifest encryption are now built, while `mode`, the connector plugin trait, and Typst component authoring remain ahead. Add a short "Build order" note (can live near the banner or in the Open-questions parking lot): **S (secrets) → E (encryption) [done] → C (connector plugin trait) → M (mode axis) → T (Typst authoring)**, with event sourcing / multi-user explicitly future per `FUTURE.md`.

- [ ] **Step 4: Verify references**

Run: `grep -n "no cleartext tier\|SecretStore\|Build order\|\.key(key)" docs/appdx.md`
Expected: matches in the Encryption passage, the worked example, and the build-order note.

- [ ] **Step 5: Commit**

```bash
git add docs/appdx.md
git commit -m "appdx: encryption + secrets now built; worked-example sources key from SecretStore; record build order"
```

---

## Self-Review notes

- **Spec coverage:** secrets store (Task 2) ✓, encryption seam (Task 1) ✓, manifest sealing in embed/readback (Task 3) ✓, key wiring through runtime/builder (Task 3–4) ✓, no-cleartext + round-trip + wrong-key tests (Task 3) ✓, store round-trip/0600/stable-key tests (Task 2) ✓, harness e2e still green (Task 4) ✓, appdx update (Task 5) ✓. Out-of-scope items (mode, connector trait, Typst authoring, event sourcing, multi-user) are intentionally absent.
- **Readback note:** the in-memory loop (`App::step`) keeps the manifest in `DocSet` and does not re-extract from the PDF, so decryption only matters where a PDF is read back (harness `common/mod.rs`, manual device test). Those paths get a matching key in Task 4.
- **Type consistency:** `Key`, `Key::from_bytes`, `seal`/`open`, `SecretStore::{open, open_default, get, set, user_key}`, `Scope`, `render_document(_, _, &Key)`, builder `.key(Key) -> BuilderReady` are used identically across tasks.
- **No plaintext fallback:** deliberately omitted (pre-release; reopening it would re-leak), per spec.
