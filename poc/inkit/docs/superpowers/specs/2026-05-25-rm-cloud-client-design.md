# inkapp — Spec: `rm-cloud` reMarkable Cloud client

**Date:** 2026-05-25
**Status:** Approved (design); plan pending

## Context

inkapp currently reaches the reMarkable Cloud by **shelling out to the `rmapi`
CLI** (Go) from each app's `serve.rs` and from the spike. That works but is a
hard dependency on an external binary, a patched fork (the v4 cloud break, see
`remarkable-pdf-mechanics.md` §10), and a process boundary we can't test cleanly.

This spec defines **`crates/rm-cloud`**, a pure-Rust client for the *current*
reMarkable Cloud sync protocol. It is built to be a first-class library that
**any Rust project** can use — not only inkapp. Once it is solid, inkapp's
transport (`serve.rs`, the spike) will migrate onto it and drop the `rmapi`
dependency. That migration is **out of scope here** ("only once this works
well").

`rmapi` (`github.com/ddvk/rmapi`) was read for **protocol knowledge only** — no
code is copied. Crucially, we **reject rmapi's abstraction**: rmapi models the
cloud as a shell/filesystem (`ls`/`cd`/`get`/`put`/`mget`) because its job is
interactive file management. That shape is wrong for inkapp, whose real question
each loop is *"did the user write anything, and where?"*. Today that forces
`serve.rs` to `mget` an entire folder every cycle.

### Key prior decisions (from brainstorming)

- **Crate name `rm-cloud`**, a new crate in this workspace. It is
  reMarkable-specific, so it follows the workspace `rm-` prefix convention.
- **The cloud is natively a git-style content-addressed store**, not a
  filesystem: blobs keyed by `sha256`, tree-index blobs pointing at blobs, and a
  root "ref" with a monotonic **generation** updated by compare-and-swap. The
  crate exposes it as what it is — **immutable snapshots + `diff` + atomic CAS
  commit** — and builds the familiar path operations *on top* of that.
- **Layered, fully public API:** plumbing (blob store, snapshot, commit),
  porcelain (path-tree file ops, rmapi-compatible), and a **sync/reconcile
  layer** tuned for inkapp's working-set loop.
- **Reuse `rm-files`** for the on-disk document format (`.rm` v6 scene
  parse+write, the `.rmdoc`/dir `Bundle`, `Metadata`, `Content`). `rm-cloud`
  never re-parses scene bytes.
- **Async** throughout: tokio + reqwest (rustls).
- **Real in-process fake cloud** (axum) for tests; a **separate env-gated suite
  against the live cloud**, isolated under `rmrs-test/<run-id>`.
- **Credentials come from env vars for now** — a config system being built in a
  parallel worktree will replace the source shortly, so the crate exposes a thin
  seam rather than building token persistence.
- **Device pairing is in scope** (one-time code → device token).

### Boundaries

- `rm-cloud` does **not** depend on `inkapp-core` or any app crate. It is a leaf.
- `rm-cloud` owns the **cloud**: auth/pairing, the content-addressed blob store,
  the hash-tree (root + per-doc indexes), snapshots, generation CAS, conflict
  rebase, and the porcelain/sync layers.
- `rm-files` owns the **local document format**. `rm-cloud` converts between a
  `rm_files::Bundle` and the cloud's blob tree; it does not duplicate scene
  parsing.
- The crate keeps inkapp's invariants for free: it is **stateless per session**
  (no session DB; the snapshot is built from the cloud each run) and it never
  puts credentials into any document or blob.

## Goals (this spec)

1. A pure-Rust client for the current reMarkable Cloud sync protocol, with a
   content-addressed **snapshot** core, **path porcelain**, and an inkapp-facing
   **sync/reconcile** layer.
2. Device pairing + user-token refresh, with credentials sourced from env vars
   behind a replaceable seam, and multi-account support via independent clients.
3. An ink-preserving `put_content_only` that reimplements
   `remarkable-pdf-mechanics.md` §3 natively.
4. Correct conflict handling: CAS commit with a bounded **rebase-on-412** retry.
5. A faithful in-process **fake cloud** (real HTTP, fault injection) and an
   env-gated **real-cloud** suite isolated under `rmrs-test/<run-id>`.

## Non-goals (v1)

- Migrating inkapp's `serve.rs`/spike off `rmapi` (a later spec).
- Persistent on-disk tree cache (an in-session blob cache is enough; snapshots
  are rebuilt per run).
- Bypassing the cloud entirely (USB web UI / SSH straight to the device). That is
  a different *transport medium* and belongs behind inkapp's `Device` seam — a
  `docs/FUTURE.md` note, not v1.
- Integrating the parallel-worktree config system (we leave the `Credentials`
  seam for it).
- Annotation/PDF rendering features rmapi bundles (`geta`); rendering is inkapp's
  job, not the transport's.

## Protocol (what we reimplement)

Verified by reading rmapi's source (`config/url.go`, `api/auth.go`,
`transport/transport.go`, `api/sync15/*`):

### Endpoints

| Purpose            | Method | URL                                              | Auth   |
|--------------------|--------|--------------------------------------------------|--------|
| New device token   | POST   | `{auth}/token/json/2/device/new`                 | none   |
| New user token     | POST   | `{auth}/token/json/2/user/new`                   | device |
| Get root ref       | GET    | `{sync}/sync/v4/root`                            | user   |
| Put root ref (CAS) | PUT    | `{sync}/sync/v3/root`                            | user   |
| Get/Put blob       | GET/PUT| `{sync}/sync/v3/files/<hash>`                    | user   |

Default hosts: `auth = https://webapp-prod.cloud.remarkable.engineering`,
`sync = https://internal.cloud.remarkable.com`. All overridable via env
(`RM_CLOUD_HOST` overrides all; the protocol module also honors the `RMAPI_*`
names for familiarity) so tests point the client at the fake cloud.

### Auth

- `register_device(code)` POSTs `{code, deviceDesc, deviceID:<uuid>}` → device
  token (long-lived, opaque string).
- `refresh_user_token()` POSTs with the device bearer → user token (short-lived).
  All sync calls use the user bearer; a 401 triggers one transparent refresh.

### Blobs & hashing (must match byte-for-byte)

- Blobs are content-addressed: blob name = `sha256(content)` as lowercase hex.
- `GET/PUT /sync/v3/files/<hash>` carry an `rm-filename` header; the
  `root.docSchema` upload uses `content-type: text/plain; charset=UTF-8`.
- **File hash** = `sha256(file bytes)`.
- **Doc hash** = `sha256( concat( hex_decode(file.hash) for files sorted by
  file id ) )`.
- **Root hash** = `sha256( serialized root-index bytes )`.

### Tree (index blob format)

Colon-delimited lines.

- **Root index (schema `4`):** line 1 = `4`; line 2 = `0:.:<docCount>:<totalSize>`;
  then one line per doc: `<docHash>:0:<docId>:<numFiles>:<size>`.
- **Per-doc index (schema `3`):** line 1 = `3`; then one line per file:
  `<fileHash>:0:<fileId>:0:<size>`.
- Writes always emit schema 4 for the root (current servers reject new v3-format
  root uploads); per-doc indexes are schema 3. The reader accepts both.

### Document model

A document is a per-doc index blob pointing at file blobs:
`<id>.metadata`, `<id>.content`, the payload (`<id>.pdf` / `.epub`),
`<id>/<page-uuid>.rm` (one per annotated page), `<id>.pagedata`. A **folder** is a
document whose `.content`/metadata `CollectionType = "CollectionType"` with no
payload. This maps 1:1 to `rm_files::Bundle`.

### Root ref / generation

`GET /sync/v4/root` → `{hash, generation}`. `PUT /sync/v3/root` with
`{hash, generation, broadcast}` → new generation, or **HTTP 412** if the supplied
generation is stale (compare-and-swap conflict). 401 → unauthorized,
404 → not found, 409 → conflict.

## Architecture

Three layers in one crate; lower layers are usable on their own.

```
plumbing  content-addressed truth
          ├─ BlobStore   get_blob(hash) / put_blob(hash, name, bytes)
          ├─ Snapshot    immutable { generation, root_hash, docs: tree }  + diff()
          └─ commit()    build new tree → upload blobs → CAS root-put (rebase on 412)
porcelain rmapi-compatible path view, built on snapshots
          └─ Fs          ls / stat / get(->Bundle) / put(Bundle) /
                         put_content_only / mkdir / mv / rm
sync      inkapp's working-set loop, built on snapshot diff
          └─ reconcile   declarative: "make the cloud match this target set;
                         report which keys' ink changed since a prior snapshot"
```

### Module layout

```
crates/rm-cloud/
  Cargo.toml          # tokio, reqwest(rustls), serde, serde_json, sha256, uuid,
                      #   thiserror; rm-files; [feature fake] -> axum (optional)
  src/
    lib.rs            # re-exports + module docs
    error.rs          # Error/Result (thiserror)
    config.rs         # endpoint URLs + env overrides
    auth.rs           # register_device, refresh_user_token, Credentials seam
    transport.rs      # reqwest wrapper: bearer, rm-filename, status->Error
    plumbing/
      blob.rs         # BlobStore + sha256 helpers
      index.rs        # Entry, schema 3/4 parse+serialize, hashing rules
      snapshot.rs     # Snapshot (immutable), build_from_cloud(), diff()
      commit.rs       # mutate a snapshot -> CAS root-put with rebase-on-412
    porcelain/
      fs.rs           # path resolution + ls/stat/mkdir/mv/rm
      document.rs     # get()->Bundle, put(Bundle), put_content_only(id, pdf)
    sync.rs           # declarative working-set reconcile (inkapp layer)
    client.rs         # Client: from_device_token / from_user_token / from_env
    fake/             # [feature fake] axum fake cloud + fault injection
```

### Core types (sketch)

```rust
pub struct Client { /* http, creds, base urls */ }

impl Client {
    pub fn from_device_token(token: impl Into<String>) -> Self;
    pub fn from_user_token(token: impl Into<String>) -> Self;
    /// Reads RM_CLOUD_DEVICE_TOKEN (and optional RM_CLOUD_USER_TOKEN).
    pub fn from_env() -> Result<Self>;
    pub async fn register_device(auth_base: &str, code: &str) -> Result<String>; // device token

    pub async fn snapshot(&self) -> Result<Snapshot>;
    pub fn fs<'a>(&'a self, snap: &'a Snapshot) -> Fs<'a>;
    pub async fn sync(&self, target: WorkingSet, since: Option<&Snapshot>) -> Result<SyncReport>;
}

/// Immutable view of the whole account at one generation.
pub struct Snapshot { pub generation: i64, pub root_hash: String, /* docs */ }
impl Snapshot {
    pub fn diff(&self, other: &Snapshot) -> TreeDiff; // added/removed/changed doc ids
    pub fn doc(&self, id: &str) -> Option<&DocEntry>;
}

pub struct TreeDiff { pub added: Vec<String>, pub removed: Vec<String>, pub changed: Vec<String> }
```

### Credentials seam (env for now)

A small `Credentials` trait yields the device/user tokens; the **only concrete
impl now reads env vars** (`RM_CLOUD_DEVICE_TOKEN`, optional
`RM_CLOUD_USER_TOKEN`). The future config system plugs in by implementing the
trait. **Multi-account = multiple independent `Client`s**, each constructed from
its own token value; there is no global state, so parallel clients to different
accounts simply work.

## Key operations

### Snapshot & diff (plumbing core)

`Client::snapshot()` does `GET /root`, fetches the root index blob, and parses it
into doc entries (each with its hash). `Snapshot::diff` compares two snapshots'
doc-id→hash maps to produce added/removed/changed doc ids. For a changed doc, the
fs/sync layers fetch that doc's per-doc index and diff *its* file entries to find
exactly which `.rm`/`.pdf`/etc. blobs moved. Everything is content-addressed, so
unchanged blobs are skipped by construction.

### Atomic commit (plumbing)

A mutation produces a new desired tree: hash and upload any new blobs, build the
new per-doc and root indexes, then `PUT /root` with the snapshot's generation.
**On 412**, re-fetch the root, rebase the change onto the new snapshot (3-way by
doc id — well-defined because the store is content-addressed and id-keyed), and
retry (bounded attempts + small backoff). Multiple document changes commit as a
**single** root-put → atomic publish.

### `put_content_only(doc_id, new_pdf)` (porcelain — inkapp's critical path)

Native reimplementation of mechanics §3: fetch the doc index, find the `.pdf`
entry, upload the new PDF blob, update that entry's hash/size, rehash the doc,
update the root entry, rehash root, CAS-put root. It **never** writes `.content`
or any `.rm` → ink and page order are preserved byte-for-byte. This is the
operation behind "an updated article keeps the user's annotations."

### `get` / `put` / `mkdir` / `mv` / `rm` (porcelain)

`get(id) -> rm_files::Bundle` downloads all of a doc's blobs and assembles a
Bundle. `put(Bundle)` uploads a fresh document (new UUID). `mkdir` creates a
folder doc. `mv`/rename edits the `.metadata` blob (parent / visibleName) only.
`rm` removes the doc from the tree and commits. All terminate in a CAS commit.

### `sync(target, since)` (inkapp reconcile layer)

Declarative: given a target working set (key → desired payload + metadata) and
an optional prior snapshot, compute the minimal blob uploads + a single commit to
make the cloud match the target, and report which keys' ink changed since
`since`. This is the layer inkapp's `serve.rs` will eventually call instead of
`mget`-ing whole folders. The no-op fast path: if `GET /root` shows the
generation unchanged from `since`, return an empty report without fetching
anything else.

## Testing

Four tiers; the fake cloud is the workhorse.

1. **Unit (pure, fast).** Hashing rules against golden hashes (file/doc/root);
   index schema 3/4 parse↔serialize round-trips; snapshot `diff` cases
   (add/remove/change); path resolution; HTTP status→`Error` mapping.

2. **Fake-cloud integration (real HTTP).** An axum **`FakeCloud`** behind the
   `fake` feature: in-process, ephemeral port (`127.0.0.1:0`), storing blobs in a
   map keyed by hash and enforcing root generation CAS (returns 412 on stale
   generation). Exposed publicly under the feature so **downstream projects can
   test their own code against it**. Tests drive the full lifecycle over the real
   reqwest path:
   - pair → empty snapshot → `mkdir` → `put` → `ls`/`stat`
   - `get` round-trip: uploaded `Bundle` bytes == downloaded `Bundle` bytes
   - **`put_content_only`**: assert the `.rm` and `.content` blobs are unchanged
     and only the `.pdf` blob hash moved (fidelity to mechanics §3)
   - snapshot `diff`: after a mutation, the diff reports exactly the changed doc
   - **fault injection**: a competing root bump forces a **412**; assert the
     client re-mirrors, rebases, and the commit ultimately succeeds
   - 401 → refresh-and-retry once; 404 mapping

3. **Concurrency.** Parallel `put`s and concurrent blob up/download against the
   fake; assert the final snapshot is consistent and the generation advances
   monotonically.

4. **Real-cloud (env-gated, `#[ignore]`d).** The same lifecycle against the live
   cloud, entirely inside **`rmrs-test/<run-id>`** (run-id = a fresh UUID per run,
   so parallel runs never collide). Token from `RM_CLOUD_DEVICE_TOKEN`; the suite
   **skips with a clear message if unset** so CI and other contributors need no
   credentials. **Leave-on-failure:** teardown deletes the run folder on success
   and keeps it on failure for debugging; a separate
   `sweep_stale_test_folders` utility removes old `rmrs-test/*`.

Fidelity fixtures reuse `rm-files`/harness bundle fixtures so the
`put_content_only` test runs against a real annotated bundle.

## Definition of done

- `crates/rm-cloud` builds, `make clippy` clean, `make fmt-check` clean.
- All four test tiers present; tiers 1–3 run in `make test` (real-cloud tier is
  `#[ignore]`d / env-gated).
- The crate is added to the workspace and documented.
- Docs reconciled: a new `docs/rm-cloud-protocol.md` capturing the on-the-wire
  rules (sibling to `remarkable-pdf-mechanics.md`); a short `rm-cloud` entry in
  the Architecture section of `CLAUDE.md`; a one-line `docs/FUTURE.md` note for
  the deferred direct-device transport axis. (inkapp's `serve.rs` migration and
  any `docs/appdx.md` changes are a later spec — `rm-cloud` is not app-facing
  DX.)
