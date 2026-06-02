# Design: eliminate duplicate cloud-folder creation

**Date:** 2026-06-02
**Status:** Approved — ready for implementation plan

## Problem

`rmapps` accumulates duplicate same-named folders at the reMarkable cloud root
(confirmed live: 4× `Readwise`, 3× `2026`). They never converge: `rmapps ls
/Readwise` only resolves the *first* match, so documents scattered across the
duplicate folders become invisible to the tooling while the device shows every
copy. (The live duplicates have already been moved to cloud trash manually; this
work prevents recurrence.)

## Root cause

Folder creation goes through `Client::mkdir_p`
(`crates/rm-cloud/src/porcelain/fs.rs:64`): it resolves one snapshot and, if a
folder of the target name is absent under the parent, mints a new one with a
fresh UUID. Every `Cloud::upsert`/`replace`/`create_if_missing`
(`apps/rmapps/src/cloud.rs`) calls `ensure_folder` → `mkdir_p` internally.

The confirmed duplication is **intra-run multiplicity** — one run resolves the
same destination path many times, each resolve an independent "create if not
found" decision:

- **reader** (`apps/rmapps/src/reader.rs`): `library_folder` and `feed_folder`
  are both `/Readwise`, so the upload loop calls `replace("/Readwise", …)` twice
  back-to-back → two `ensure_folder("/Readwise")` calls.
- **bujo** (`apps/rmapps/src/bujo.rs`): every PDF deploys to the single `target`
  (e.g. `/2026`) via `upsert`/`create_if_missing`, so one whole-year run calls
  `ensure_folder("/2026")` up to 14×.

Under the reMarkable cloud's eventual consistency, a later resolve in the same
run can fail to observe the folder an earlier resolve just created (the
generation has moved but the new folder is not yet in the served root index), so
it mints a second folder. Once two same-named folders exist, nothing converges
them.

The fix: resolve each distinct destination path **once per run** and reuse the
folder id for every put. With a single `mkdir` decision per folder per run, the
race has no opening.

## Non-goals (decided)

- **No `dedup-folders` cleanup subcommand.** The live mess is already handled;
  prevention plus the existing doc-level sweep in `replace` is sufficient.
  Revisit only if duplicates recur.
- **No authoritative cache-bypass resolve in `mkdir`.** Scheduled runs are
  spaced minutes/hours apart — far beyond the seconds-scale propagation lag — so
  the cross-run window is not worth hardening once resolve-once is in. An
  eventual-consistency window cannot be fully closed by a read anyway.
- The existing doc-level duplicate sweep (`doc_ids_in` in `replace`) is
  untouched.

## Design

### Scope constraint: the `watch` daemon's `Cloud` is long-lived

`watch::run` (`apps/rmapps/src/watch/mod.rs:295`) builds **one** `Cloud` and
reuses it for the daemon's entire lifetime across all tasks. A `Cloud`-lifetime
folder-id cache would therefore be wrong — a folder can be trashed and recreated
between tasks hours apart, and a stale cached id would deploy into a deleted
folder. The resolver cache must be scoped to a single run/task, not the `Cloud`.

### 1. Id-based deploy methods on `Cloud` (`apps/rmapps/src/cloud.rs`)

Add three methods that accept an already-resolved `folder_id` and never call
`ensure_folder`:

- `upsert_in(folder_id: &str, name: &str, pdf: Vec<u8>) -> Result<()>`
- `replace_in(folder_id: &str, name: &str, pdf: Vec<u8>) -> Result<()>`
- `create_if_missing_in(folder_id: &str, name: &str, pdf: Vec<u8>) -> Result<()>`

Refactor the existing path-based `upsert`/`replace`/`create_if_missing` into
thin wrappers: `let id = self.ensure_folder(folder)?;` then delegate to the
matching `_in` method. Behaviour of the path-based methods is unchanged, so
single-shot callers (digest, push) need no edits.

### 2. Run-scoped folder resolver

A small helper that memoizes path → folder-id for the duration of one run:

```rust
struct FolderIds<'a> {
    cloud: &'a Cloud,
    ids: HashMap<String, String>,
}

impl<'a> FolderIds<'a> {
    fn new(cloud: &'a Cloud) -> Self { … }

    /// Resolve `path` to a folder id, creating it on first miss; cached after.
    fn get(&mut self, path: &str) -> Result<String> {
        if let Some(id) = self.ids.get(path) { return Ok(id.clone()); }
        let id = self.cloud.ensure_folder(path)?;
        self.ids.insert(path.to_string(), id.clone());
        Ok(id)
    }
}
```

The first `get(path)` performs the one `ensure_folder` (hence the one possible
`mkdir`); every later `get(path)` returns the cached id with no cloud call. Lives
in `cloud.rs` next to `Cloud`.

### 3. Caller changes

- **reader.rs**: construct one `FolderIds` for the upload run; the upload loop
  becomes `cl.replace_in(&folders.get(folder)?, &doc_name(pdf)?, read(pdf)?)`.
  Library and Feed both map to `/Readwise`, so `ensure_folder` runs once. Where
  the readback write path (`cloud_adapters`) deploys to the same folder, route it
  through the same resolver if reachable without contorting the trait boundary;
  otherwise leave readback as-is (it is best-effort and not a confirmed
  offender).
- **bujo.rs**: construct one `FolderIds`; all three deploy loops (whole-year
  upsert, only-month upsert, create-if-missing extras) use
  `*_in(&folders.get(&target)?, …)`. The single `target` resolves once for all
  PDFs in the run.

### 4. Tests

- **Fake staleness seam** (`crates/rm-cloud/src/fake`): add
  `inject_stale_root_reads(n)`. The next *n* root GETs report the current
  generation but serve the *previous* root-index hash, reproducing "generation
  moved but the newly created folder is not yet visible." This is the only way to
  exercise the eventual-consistency path against the otherwise
  immediately-consistent fake. Implementation: track the prior root hash on each
  root PUT and a `stale_root_gets` counter consumed by the root GET handler.
- **Bug-lock test**: two back-to-back `ensure_folder("/Readwise")` calls under
  injected staleness yield **two** `Readwise` folders — locks in the underlying
  defect so a regression cannot silently reappear.
- **Fix test**: the resolver-based deploy of two docs to `/Readwise` under the
  same injected staleness yields **exactly one** `Readwise` folder, because the
  resolver makes only one `ensure_folder` call and the stale second read never
  happens.
- **bujo guard**: deploying N docs to a single `target` creates exactly one
  folder.
- `cargo test --workspace` passes.

## Verification

- Automated: the tests above (fake cloud, `rm-cloud` `fake` feature).
- Live: against the real cloud, on saturn stop `rmapps-watch` first (it holds the
  single-instance lock), run a reader and a bujo sync, then `rmapps ls /` and
  `rmapps ls /Readwise` / `rmapps ls /2026` to confirm exactly one folder of each
  name; restart `rmapps-watch`.

## Files touched

- `apps/rmapps/src/cloud.rs` — three `_in` methods, wrapper refactor, `FolderIds`.
- `apps/rmapps/src/reader.rs` — resolver in the upload loop.
- `apps/rmapps/src/bujo.rs` — resolver in the three deploy loops.
- `crates/rm-cloud/src/fake/mod.rs` (+ `handlers.rs`) — `inject_stale_root_reads`.
- Tests in `cloud.rs` and/or a `fake_*` integration test.
