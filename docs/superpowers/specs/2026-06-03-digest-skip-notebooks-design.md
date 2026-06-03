# Design: skip non-pdf/epub documents in digest

**Date:** 2026-06-03
**Status:** approved, pending implementation

## Problem

`digest` is subscribed to `/Books`, which also contains native reMarkable
notebooks (e.g. *Notes-Getting to Zero*). Those notebooks get digested even
though only real source documents (PDF, EPUB) are wanted.

The document kind (`pdf` / `epub` / `notebook`) lives in the `.content`
sidecar's `fileType` field. The cloud **sync index does not carry it** — the
resolved tree only distinguishes folder (`CollectionType`) from document, built
purely from `.metadata`. A zero-fetch filter at `list_recursive` time would
therefore require enriching the index (a `.content` read per changed doc, a
schema bump) for *all* consumers (sync, reader, ls) — too broad a blast radius
for this need.

`digest` already opens the bundle in `ingest()`, which parses `.content`, so the
authoritative `fileType` is available there at no extra cost.

## Approach

Filter at `rmdigest::generate::process_doc` — the single choke point both the
reactive watch path (`run_one`) and the scheduled 6-hour sweep (`run`) funnel
through — using a **fixed allow-list `["pdf", "epub"]`**. Anything else
(notebooks, empty/unknown types) is skipped: no digest is generated or deployed.

## Changes

### 1. `rmfiles` — expose the type

In `crates/rmfiles/src/bundle/content.rs`, add to `Content`:

```rust
#[serde(default, rename = "fileType")]
pub file_type: String,
```

In `crates/rmfiles/src/bundle/mod.rs`, store `file_type` on `Bundle` (parsed
from `content`) and add:

```rust
pub fn file_type(&self) -> &str { &self.file_type }
```

This is a generic accessor — no allow-list policy lives in `rmfiles`.

### 2. `rmdigest::state` — skip sentinel

In `crates/rmdigest/src/state.rs`, add to `DocState`:

```rust
/// True if this source was skipped as an unsupported kind (e.g. a native
/// notebook). Lets a later run cheap-skip without re-fetching the bundle.
#[serde(default)]
pub skipped: bool,
```

`#[serde(default)]` keeps old state files loadable (missing field → `false`).

### 3. `rmdigest::generate::process_doc` — filter + re-fetch avoidance

New control flow:

1. Existing cheap-skip (version matches + `page_hashes` non-empty) → no fetch.
2. **New** cheap-skip: if `doc.version.is_some()` and `prev.skipped` and
   `prev.cloud_version == doc.version` → return, **no fetch**. (Stops the sweep
   re-downloading a notebook on every run.)
3. fetch + `ingest` (opens the bundle).
4. **New** allow-list gate: if `ing.bundle.file_type()` is not `"pdf"` or
   `"epub"` → set `prev.cloud_version = doc.version`, `prev.skipped = true`,
   clear `prev.page_hashes`, `state.save(...)`, return. No deploy.
5. Existing "nothing changed" skip.
6. Process + deploy as today; on success set `prev.skipped = false` (so a doc
   that was a notebook and later becomes a real document re-engages).

The allow-list is a module constant: `const SUPPORTED: [&str; 2] = ["pdf", "epub"];`

## Error handling

A doc with an empty or missing `fileType` is treated as not-allowed and skipped
— conservative, and consistent with "only do pdf and epub". Skipping is a normal
`Ok(())` return; no new error paths are introduced.

## Testing

- **rmfiles** (`bundle` tests): `file_type()` returns `"notebook"` and `"pdf"`
  from synthesized `.content` JSON; missing field yields `""`.
- **rmdigest** (`generate` tests):
  - A synthesized notebook bundle (`fileType: "notebook"`, no source PDF) →
    `run` produces **0 puts**, and the saved `DocState` has `skipped == true`.
  - With a `CountingBackend`, a second `run` over the same unchanged notebook
    performs **no second fetch** (the skip sentinel engages).
  - Existing pdf-fixture tests (`integration_two_puts_then_skip`, etc.) still
    produce their digest — both fixtures are `fileType: "pdf"`, so they remain
    green (regression guard).

## Scope / YAGNI

- No config knob — the allow-list is fixed at `pdf` + `epub` (chosen).
- No sync-index enrichment.
- Accepted cost: the *first* sight of a notebook still fetches its bundle once
  before the sentinel engages; every subsequent run cheap-skips with no fetch.
