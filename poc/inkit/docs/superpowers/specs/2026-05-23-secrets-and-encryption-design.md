# inkapp — Spec #5: Secrets store + encryption ("no cleartext tier")

**Date:** 2026-05-23
**Status:** Approved (design); plan pending

## Context

`docs/appdx.md` still opens with a **"Status: exploratory"** banner and claims "Much of it is
not built yet." After Specs #1–#4 that is now half-false: the MVU app surface, the reconcile
loop, the `Component` (render + decode), the Readwise connector, reMarkable sync, and the
worked-example `reading-queue` app are all real and tested. The agreed goal is to **make the
whole doc true** for the pieces it presents in present tense, so the "exploratory" banner can
eventually come off.

The pieces `appdx` describes as present-tense but does **not** yet have in code are:

- **Encryption** — the "State → Encryption" section's *"everything embedded is encrypted"* rule
  and the **"no cleartext tier"** claim, plus threat-model concern (1) (the share-leak case).
- **Secrets/config store** — the per-user store with three scopes.
- **`mode` axis** — `Display | Capture | Control` / `ReadOnly | Editable`.
- **Connector plugin trait** — `Arc<dyn Connector>`, interior-mutability cache, single-flight,
  recorded-and-retried writes.
- **Typst component authoring** — `.typ` files, sandboxed render-time scripting.

Explicitly **out of scope** (legitimately documented future; stays in `FUTURE.md`):
event sourcing / CRDT merge types, multi-device reconciliation, multi-user/cloud tenant
isolation, and concrete key-management mechanics (KMS, rotation).

### The build order (overarching roadmap)

Dependency edges among the in-scope pieces:

```
  S ──hard──▶ E            S = Secrets/config store
  S ──soft──▶ C            E = Encryption (no cleartext tier)
  M (independent)          C = Connector plugin trait + write queue
  T (independent)          M = mode axis
                           T = Typst component authoring
```

Agreed sequence: **S → E → C → M → T.**

1. **S** is the foundation — `E` hard-depends on it (needs a key); `C` wants it (creds).
2. **E** is the doc's loudest claim and is tightly localized to embed/manifest/readback.
3. **C** is the largest refactor (changes the app-facing runtime + every connector); done
   after `E` so `E` is not redone against a new connector shape. Only `readwise` +
   `reading-queue` exist to migrate today, so the cost is still small.
4. **M**, **T** are contained and lower-stakes; either can land anytime, so they go last.

Rejected alternatives: *refactor-first* (`C` first) front-loads the biggest bite and forces
stubbing creds with no dependency reason; *smallest-first* (`M` first) defers the flagship
credibility gap and leads with `mode`, which the worked example explicitly didn't need.

### Position in the spec arc

- **Spec #1** — Typst-readback spike (merged).
- **Spec #2** — Deterministic harness (merged).
- **Spec #3** — E2E gesture-fixture layer (merged).
- **Spec #4** — The MVU app loop (merged).
- **Spec #5 — Secrets store + encryption (this doc).** First increment of "make the doc true";
  flips the entire Encryption / no-cleartext-tier section from aspirational to real.

## This increment: S + E as one vertical slice

`S` alone flips nothing in the doc from false to true — it is plumbing. Bundled with `E`, the
increment flips the **"State → Encryption — everything embedded is encrypted"** section, the
**"no cleartext tier"** rule, and **threat-model concern (1)** from aspirational to real, in one
coherent, testable step. So Spec #5 builds both together.

### Today's reality (grounding)

- `crates/inkapp-core/src/embed.rs` writes the manifest as **plaintext JSON** into the PDF Info
  dictionary: `info.set(MANIFEST_KEY, Object::string_literal(json))`. `extract_manifest` reads
  it straight back. Region names (`done`, `tok-0`, …) are therefore in cleartext in any shared
  PDF — the exact leak the doc says is closed.
- `Manifest` (`manifest.rs`) is plain `serde` (`version: u64`, `regions: Vec<Region>`).
- There is **no per-document/per-component state field carried in the PDF yet** beyond the
  manifest's `version` marker. The doc describes an encrypted "state field" at document and
  component level; this spec introduces the *seam* for it and encrypts the manifest, and either
  (a) introduces a minimal state-field payload if one is needed to be meaningful, or (b) treats
  the manifest as the embedded blob and documents the component-state field as a thin extension
  of the same seam. **Decision: (b)** — encrypt the embedded manifest blob now (it already
  carries the structural secrets), and define the `encrypt`/`decrypt` seam so a later
  per-component state field rides the same path without a redesign. This keeps the increment
  bite-sized while making the no-cleartext claim true for what is actually embedded today.
- `rand`, `rand_chacha`, and `getrandom` are already in `Cargo.lock` transitively; adding an
  AEAD crate is the only new direct dependency.

### Architecture

**1. Secrets store — `inkapp-core::secrets` (module, not a new crate).**

A single-user, file-backed store with the three scopes the doc names:

```rust
pub struct SecretStore { /* path + in-memory cache */ }

pub enum Scope {
    ConnectorCred,   // e.g. Readwise token, CalDAV login
    DeviceAuth,      // reMarkable cloud auth, etc.
    UserKey,         // the document-state encryption key
}

impl SecretStore {
    pub fn open(path: &Path) -> Result<Self>;          // create dir/file on first use
    pub fn get(&self, scope: Scope, name: &str) -> Option<SecretBytes>;
    pub fn set(&mut self, scope: Scope, name: &str, value: SecretBytes) -> Result<()>;
    /// The per-user AEAD key; generated + persisted on first call.
    pub fn user_key(&mut self) -> Result<&Key>;
}
```

- Backing format: a single JSON (or TOML) file under a config dir, keyed by `(scope, name)`,
  values base64. **Plain on-disk for now** — at-rest protection of the store file itself, KMS,
  and rotation are deferred (the doc marks them future). File perms set to `0600`.
- The store is the *only* place the per-user key is minted. It is generated with the OS CSPRNG
  on first `user_key()` and persisted under `Scope::UserKey`.

**2. Encryption seam — `inkapp-core::crypto`.**

```rust
pub struct Key([u8; 32]);
pub fn seal(key: &Key, plaintext: &[u8]) -> Vec<u8>;          // nonce ‖ ciphertext ‖ tag
pub fn open(key: &Key, sealed: &[u8]) -> Result<Vec<u8>>;     // verifies tag; typed error
```

- AEAD: **XChaCha20-Poly1305** (24-byte random nonce per `seal`, so no nonce-reuse bookkeeping;
  pure-Rust, no system OpenSSL). Candidate crate: `chacha20poly1305`. (AES-GCM via `aes-gcm`
  is the fallback if a dependency conflict appears; decided at plan time.)
- Nonce is generated per call from the OS CSPRNG and prepended to the output. `open` splits it
  back off, decrypts, and verifies the Poly1305 tag — a wrong/missing key or tampered bytes
  yields a typed `Error`, never a panic.

**3. Wiring into embed/readback.**

- `embed_manifest`: serialize manifest → JSON bytes → `seal(key, bytes)` → store the sealed
  blob in the Info dict (base64 string or PDF byte-string) under `MANIFEST_KEY`. No plaintext
  region names ever hit the PDF.
- `extract_manifest` / `readback.rs`: read the sealed blob → `open(key, blob)` → deserialize.
- The render and readback paths gain access to the `Key`. `App`/the builder/`run()` opens the
  `SecretStore` and threads the key into the render + readback calls. The app author does **not**
  see ciphertext — they keep returning a plain `Manifest`/`Documents`; the framework seals on
  write and opens on read.

### Data flow

```
render:   view → Manifest → JSON → seal(user_key) → PDF Info dict   (ciphertext only)
readback: PDF Info dict → open(user_key) → JSON → Manifest → attribute ink → decode
```

The `Key` comes from `SecretStore::user_key()`, opened once per app run from the config path.

### Error handling

- `crypto::open` failures (wrong key, truncated, tampered tag) → a new typed `Error::Crypto`
  variant, surfaced on the readback path; the loop reports it rather than panicking.
- `SecretStore::open` failures (unreadable/corrupt file, bad perms) → `Error::Secrets`.
- Backward read of a **plaintext** legacy manifest is **not** supported — this is pre-release;
  there are no persisted prod PDFs to migrate. (Noted explicitly so no one adds a cleartext
  fallback that would silently reopen the leak.)

### Testing (how we prove the doc is now true)

- **No-cleartext assertion:** render a `reading-queue` document, read the raw PDF bytes, assert
  the Info-dict manifest entry contains **none** of the known region-name substrings (`done`,
  `tok-`, article ids) — i.e. the structural secrets are not in plaintext.
- **Round-trip:** render with key → readback with the *same* key → identical `Manifest`
  (and ink attribution still works end to end).
- **Wrong/missing key:** `open` with a different key → `Err(Error::Crypto)`, no panic; readback
  with a fresh store (no key match) fails cleanly.
- **Store round-trip:** `set`/`get` across all three scopes survives reopen from disk;
  `user_key()` is stable across reopen and distinct per store path; file mode is `0600`.
- **Harness e2e still green:** the existing multi-cycle and on-device round-trip tests pass with
  encryption on the path (key sourced from a test `SecretStore`).

### Components and their boundaries

| Unit                       | Does                                              | Depends on            |
|----------------------------|---------------------------------------------------|-----------------------|
| `secrets::SecretStore`     | Persist/fetch per-user secrets in three scopes    | filesystem, `crypto`  |
| `crypto::{seal,open,Key}`  | AEAD seal/open with per-call nonce                | AEAD crate, CSPRNG    |
| `embed` (modified)         | Seal manifest into / open from PDF Info dict      | `crypto`              |
| `readback` (modified)      | Open manifest before attributing ink              | `crypto`, `embed`     |
| runtime `App`/builder/run  | Open the store once, thread `Key` to embed/readback | `secrets`           |

Each is testable in isolation: `crypto` with raw bytes, `SecretStore` against a temp dir,
`embed`/`readback` with a fixed `Key`, the runtime via the existing harness.

## Out of scope (deferred, not forgotten)

- Multi-user key isolation, KMS, key rotation, at-rest encryption of the store file itself.
- *Using* `DeviceAuth` (we store it; wiring reMarkable auth through the store is a later step).
- A distinct per-component state-field payload (the seam supports it; no payload exists to
  encrypt yet — added when a feature needs it).
- Event sourcing, mode axis, connector plugin trait, Typst authoring — later specs in the order
  above.

## Acceptance criteria

- [ ] `inkapp-core::secrets::SecretStore` persists the three scopes to a `0600` file and mints a
      stable per-user key.
- [ ] `inkapp-core::crypto` provides `seal`/`open` (XChaCha20-Poly1305) with per-call nonce and
      a typed error on failure.
- [ ] `embed.rs` writes only sealed manifest bytes; `extract_manifest`/`readback.rs` open them.
- [ ] No-cleartext test: known region-name substrings absent from the raw PDF.
- [ ] Round-trip test green; wrong-key test returns `Error::Crypto` without panicking.
- [ ] Existing harness e2e (multi-cycle + on-device round-trip) passes with encryption on.
- [ ] `appdx.md`'s Encryption section updated to present tense where now true (and the build
      order recorded), with remaining future items kept explicit.
