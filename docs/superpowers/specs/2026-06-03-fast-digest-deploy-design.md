# Fast digest deploy: broadcast + stable-UUID upsert

**Date:** 2026-06-03
**Status:** Approved (design)

## Problem

After annotating a PDF/epub on the reMarkable, the regenerated digest takes
**30+ seconds** to appear back on the device, and the user intermittently sees
**multiple digest files** that later converge to one.

## Measurement (root cause)

Timed `rmapps digest` on *Getting to Zero (PDF)*, warm blob cache, on saturn:

| Stage   | Time   | Notes                                              |
|---------|--------|----------------------------------------------------|
| fetch   | 0.59s  | snapshot resolve + changed-blob download           |
| extract | 0.33s  | parse marks                                        |
| render  | 0.35s  | pdftotext + per-note pdftoppm + typst compile      |
| upload  | 2.99s  | `replace` = 2 commits, ~7 serial blob PUTs         |
| total   | ~4.3s  |                                                    |

The *build* is already fast (~0.7s). The user-perceived 30s+ is **not** in this
4.3s at all — it is the gap between the daemon finishing its silent upload and
the device discovering the new digest on its own periodic poll. Two structural
causes, both in the deploy path:

1. **No device notification.** Digest deploy goes through
   `Cloud::replace` → `client.rm()` + `client.put()`, both of which call the
   non-broadcasting `commit`. The reMarkable is never told to sync, so it only
   pulls the digest on its next poll cycle (tens of seconds to minutes).
   Broadcast was deliberately disabled ("the watch daemon must not self-notify"),
   but that caution is now obsolete: `reconcile_pass` (watch/mod.rs:126–148)
   already suffix-excludes digest outputs from routing, so a broadcast wakes the
   daemon into exactly one harmless no-op reconcile (a single snapshot fetch) and
   can never re-trigger a digest.

2. **`replace` churns the doc UUID.** Delete-old + create-new-with-a-fresh-UUID
   produces the transient duplicate files (the device briefly sees both during
   the cloud's eventual consistency, and the reactive watch can overlap the
   periodic sweep), and costs two commits instead of one. `DocState` already
   carries a vestigial `digest_uuids: Vec<String>` field that was scaffolded for
   stable-UUID reuse but never wired.

`put_content_only` is **not** the fix: it swaps only the `.pdf` blob and keeps the
old `.content`, whose page list/count goes stale as highlights accumulate and the
digest grows pages. The correct primitive is a **full upsert that reuses the
existing UUID**: rebuild `.content`/`.metadata` for the new PDF, keep the id.
`commit::apply` upserts by id (`by_id.insert`), so re-putting an existing id
fully replaces that doc's blob set in a single commit — verified.

## Design

Scope (per decision): broadcast + stable-UUID only. **No** blob-PUT
parallelization in `rm-cloud`'s `commit_with` this round (keeps blast radius to
the digest deploy path; the residual ~3s rmapps-side upload is acceptable next to
the 30s win).

### Component 1 — `rm-cloud`: no change

`put_broadcast(DocFiles)` already exists and routes through `commit_broadcast`.
`DocFiles.id` is a public field and `DocFiles::new_pdf` assigns a fresh v4 UUID
that we can override. No new rm-cloud surface is required.

### Component 2 — `Cloud::deploy_digest` (apps/rmapps/src/cloud.rs)

New method, the single entry point for deploying a digest:

```rust
/// Deploy the digest PDF as a single broadcasting commit, reusing `prev_uuid`
/// when it still exists so the device updates the doc in place (no duplicate
/// flashing) and is notified to sync immediately. Returns the UUID the digest
/// now lives under (to be persisted in DocState.digest_uuids).
pub fn deploy_digest(
    &self,
    folder: &str,
    name: &str,
    pdf: Vec<u8>,
    prev_uuid: Option<&str>,
) -> Result<String>
```

Behavior:

1. `folder_id = ensure_folder(folder)`.
2. Build `let mut df = DocFiles::new_pdf(name, &folder_id, pdf)` — this rebuilds
   `.content` with the new PDF's correct page list, so growing digests stay
   correct.
3. Decide the id:
   - If `prev_uuid` is `Some(u)` **and** `u` exists in the current snapshot →
     `df.id = u`. Hot path: a single broadcasting upsert, no sweep, no `get`.
   - Else (first deploy under the new scheme, or the recorded UUID was deleted on
     device) → keep the fresh `df.id`, and **sweep** any pre-existing same-named
     docs in the folder (`doc_ids_in(folder_id, name)` → `rm` each) so we
     converge away from the current duplicate state. This branch costs extra
     commits but only runs once per source doc (or after a manual delete).
4. `self.rt.block_on(self.client.put_broadcast(df))` — one broadcasting commit.
5. Return `df.id`.

### Component 3 — `rmdigest::deploy::Backend` trait (crates/rmdigest/src/deploy.rs)

Replace `fn put(&self, pdf: &Path, folder: &str, name: &str) -> Result<()>` with:

```rust
/// Deploy the digest PDF, reusing `prev_uuid` if still present. Returns the UUID
/// the digest now lives under (empty string for backends without a UUID concept).
fn deploy_digest(
    &self,
    pdf: &Path,
    folder: &str,
    name: &str,
    prev_uuid: Option<&str>,
) -> Result<String>;
```

`LocalBackend::deploy_digest` writes `<out>/<name>.pdf` (its existing behavior)
and returns `String::new()` — local output has no cloud UUID and never
broadcasts.

### Component 4 — `CloudBackend` adapter (apps/rmapps/src/cloud_adapters.rs)

`deploy_digest` delegates to `self.cloud.deploy_digest(folder, name,
std::fs::read(pdf)?, prev_uuid)`.

### Component 5 — `generate::process_doc` (crates/rmdigest/src/generate.rs)

At the upload site:

```rust
let uuid = backend.deploy_digest(&digest_file, &doc.folder, &digest_name,
                                 prev.digest_uuids.first().map(String::as_str))?;
// persist after success, alongside cloud_version / page_hashes:
prev.digest_uuids = if uuid.is_empty() { vec![] } else { vec![uuid] };
```

Dry-run path is unchanged (returns before deploy). State is still persisted only
after a successful deploy.

## Data flow

```
device annotation
  → device syncs up + broadcasts        (device→daemon half; already fast)
  → daemon WS wakeup → reconcile → digest job
  → fetch → extract → render
  → Cloud::deploy_digest (stable UUID, ONE broadcasting commit)
  → reMarkable receives broadcast → pulls updated digest in place (~1–2s)
```

The daemon also receives its own broadcast → one reconcile → digest output is
suffix-filtered → no job → baseline advances. Harmless.

## Error handling

- **Stale `prev_uuid`** (digest deleted on device): snapshot lookup misses → fall
  to the create+sweep branch, re-record the new UUID. Self-heals.
- **Deploy fails**: `process_doc` returns the error before persisting state, so
  `digest_uuids`/`page_hashes` are not advanced and the next run retries (existing
  behavior — broadcast does not change the persist-after-success ordering).
- **Sweep partial failure**: best-effort `rm` per duplicate (matches today's
  `replace_in`); a leftover converges on the next create-branch run.

## Testing

- **rmdigest unit/integration** (generate.rs tests): update the in-crate fake
  backends (`FakeBackend`, `CountingBackend`) to the new `deploy_digest`
  signature. New assertions:
  - stable UUID: two successive runs over a changed fixture return the *same*
    UUID and the second passes `prev_uuid = Some(first)`.
  - the returned UUID is persisted into `DocState.digest_uuids`.
  - dry-run still deploys nothing and persists no UUID.
- **rmapps cloud-level** (against the `rm-cloud` `fake` feature / `FakeCloud`):
  - `deploy_digest` with `prev_uuid = None` creates one doc and **broadcasts**
    (assert the fake's broadcast count increments).
  - `deploy_digest` with a valid `prev_uuid` reuses that id (snapshot still has a
    single doc with that id; no new id appears) and broadcasts.
  - create-branch sweeps a pre-seeded same-named duplicate down to one doc.
  - (verify `FakeCloud` exposes a broadcast counter; if absent, add a minimal one
    in the fake — it already tracks per-hash blob get counts.)
- **Full suite**: `make test` (or the crate test commands) green.

## Out of scope

- Parallelizing `commit_with`'s serial blob PUTs (residual ~3s rmapps-side).
- Reader deploy (same non-broadcast pattern, separate crate/use-case).
- Any change to the device→daemon (upstream) latency half.
