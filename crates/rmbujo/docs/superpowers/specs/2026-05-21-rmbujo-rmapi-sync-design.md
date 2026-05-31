# rmbujo Phase 2a — reMarkable cloud sync (rmapi) — Design Spec

**Date:** 2026-05-21
**Status:** Approved for planning
**Author:** Dan (with Claude)

## Summary

Phase 2a gives `rmbujo` a working **reMarkable cloud sync** by filling in the
`rmapi` deploy backend behind the `Deployer` seam that Phase 1 already stubbed.
Two operations: an **initial deploy** (upload a year's PDFs to a cloud folder)
and a **non-destructive refresh** (replace a PDF's content in place while
preserving the user's handwriting and any pages they inserted on-device).

The non-destructive refresh is the whole point of the project's deterministic
output, and it rides on `rmapi put --content-only` — a real, documented rmapi
command. Because that mechanism has never been verified against reMarkable's new
**v4 cloud schema** on the actual target device, this phase is **gated on a
lifecycle spike**: prove the round-trip preserves ink and inserted pages before
writing the production backend. If the spike fails, we stop and rethink the
"regenerate-and-re-sync" model before building on it.

**ICS rendering is explicitly out of scope** — it becomes Phase 2b, a separate
brainstorm built on the refresh mechanism this phase proves.

## Goals

- Upload a generated year to the reMarkable cloud with one command.
- Refresh an already-uploaded, **already-annotated** document so the new
  background lands on the correct pages and the user's ink + inserted pages
  survive.
- Prove the refresh actually works on the v4 cloud + Paper Pro Move before
  depending on it.
- Keep `rmbujo` self-contained and open-source: vendor the rmapi build fix; no
  dependency on Dan's private dotfiles.
- Establish the page-stability invariant the rest of the project (ICS) must honor.

## Non-goals

- **No ICS / calendar rendering** (Phase 2b).
- No reMarkable cloud API reimplementation — we shell out to the `rmapi` binary,
  which owns auth and sync.
- No automated *device* testing — the physical round-trip (annotating, inserting
  pages, observing the screen) is the manual spike. All **software** is tested
  automatically with a fake `rmapi` shim.
- No `--force` (full replace) deploy mode — refresh is always content-only;
  initial upload is a plain `put`.

## Background

### rmapi build state (as of 2026-05-21)

- nixpkgs (nixos-unstable) ships **rmapi 0.0.32**. It already has `put
  --content-only` (the feature landed upstream 2025-06-21, present in 0.0.32 and
  0.0.33).
- 0.0.32 is **broken against my.remarkable.com's v4 sync schema**: the cloud
  rejects rmapi's `rm-filename` HTTP header with HTTP 400 (`failed to build
  documents tree ... status 400`) on every call. Confirmed universal, not
  account-specific.
- The fix merged upstream **2026-05-20** (PR #63 → master `0a69a608b`), but there
  is **no tagged release yet** and nixpkgs has not bumped.
- Dan's dotfiles already work around this with `overlays/rmapi.nix`, which applies
  PR #65's patch (functionally equivalent fix) to the nixpkgs rmapi. The result is
  **0.0.32 + v4 fix + `--content-only`** = fully working.

**Decision:** vendor a copy of that overlay into rmbujo. Drop it once nixpkgs
ships ≥ a release containing the v4 fix.

### The `--content-only` mechanism

`rmapi put --content-only <pdf> <dir>` replaces **only** the PDF content of an
existing cloud document, **preserving annotations and metadata**. `--force` does a
full replace (drops annotations); the two are mutually exclusive. If the target
document doesn't exist, the command creates a new one. The cloud document is
matched by folder + document name (the local file's basename without extension).

### Why non-destructive refresh is plausible

On reMarkable a "PDF" is a bundle: the `<uuid>.pdf` background, per-page
handwriting files `<uuid>/<page-uuid>.rm` (keyed by stable **page UUID**), and a
`<uuid>.content` JSON that maps each visible page to a **PDF page index**. Ink is
bound to a page by UUID; the background is bound to a page by PDF index. So
replacing the PDF blob with one of the **same page count and order** keeps ink on
the right pages — even when the user inserted notebook pages mid-document (those
get their own entries; existing redirect indices are unchanged). Changing page
count or order breaks the mapping. `--content-only` is rmapi's implementation of
this blob-swap; the spike verifies it end-to-end on v4 + the Move.

### Operational hazards (carried from prior reMarkable work)

- **Token clobber:** rmapi 0.0.32 writes empty tokens to its conf on a refresh
  failure or any empty-stdin auth prompt, bricking subsequent calls. Mitigations:
  always pass `-ni` to non-pairing commands; never invoke an auth-requiring command
  with empty stdin; snapshot `~/.config/rmapi/rmapi.conf` after pairing and restore
  if it goes empty.
- **Plain `put` = new UUID:** a flagless `put` creates a *new* document. In-place
  behavior requires `--content-only` (preserve) or `--force` (replace).
- **Official cloud auth** has no stored credential — a fresh pairing code from
  <https://my.remarkable.com/device/desktop/connect> is required.

## Architecture

Three components, in dependency order. §2 (spike) gates §3 (productionize).

### 1. rmapi provisioning (flake + auth)

- Add `nix/overlays/rmapi.nix` (vendored from dotfiles), wire it into the flake:
  `import nixpkgs { inherit system; overlays = [ (import ./nix/overlays/rmapi.nix) ]; }`.
- Add `pkgs.rmapi` to `devShells.default.buildInputs`.
- The overlay's header comment records provenance and the removal condition
  (drop when nixpkgs ships the v4 fix).
- **Auth (Dan, one-time, on neptune):** run `rmapi` interactively, paste a pairing
  code from the connect URL. Verify with `rmapi -ni ls` (pairing alone is not proof
  — the v4 bug let pairing succeed while every call 400'd). Snapshot the conf.

The built package may later wrap the binary so `rmapi` is on `PATH` at runtime;
for this phase the dev shell suffices, and the backend errors clearly if `rmapi`
is absent.

### 2. Lifecycle spike (verification — Dan-in-the-loop)

A throwaway, manual round-trip on the real Paper Pro Move. Steps:

1. `cargo run -- new` (or reuse an existing year) to produce one month PDF; pick
   one, e.g. `2026.05 May.pdf`.
2. `rmapi mkdir /rmbujo-spike` ; `rmapi put "2026.05 May.pdf" /rmbujo-spike`.
3. **Dan:** open it on the Move, write annotations on page 1 (the day list) **and**
   insert a blank page in the middle of the document; let it sync.
4. Regenerate the same PDF with a *visible* change (e.g. tweak the month header
   text or color) keeping the **same page count and order**.
5. `rmapi put --content-only "2026.05 May.pdf" /rmbujo-spike`.
6. **Dan:** sync the Move and verify:
   - (a) annotations are still on the pages they were written on,
   - (b) the inserted page is still where it was inserted,
   - (c) the regenerated background is visibly updated.
7. **Failure characterization:** repeat the refresh with a PDF that has a
   *different* page count and record exactly what breaks (mis-mapped backgrounds,
   error, etc.). This cements the stability invariant.

**Output:** `docs/superpowers/spikes/2026-05-21-rmapi-lifecycle-spike.md`
documenting the exact working command sequence, the observed `.content`/`.rm`
behavior, and the page-structure constraint.

**Go/no-go gate:** if step 6 fails (ink lost, inserted page lost, or background
not updated), STOP. The "regenerate-and-re-sync" model is in question and must be
rethought before §3.

### 3. Productionize — `deploy/rmapi.rs`

A new `RmapiDeployer` implementing the existing `Deployer` trait, returned by
`get_deployer` when `config.deploy.backend == "rmapi"`:

```rust
fn deploy(&self, paths: &[PathBuf]) -> Result<()>   // initial upload
fn refresh(&self, paths: &[PathBuf]) -> Result<()>  // non-destructive update
```

- `deploy`: `rmapi -ni mkdir <target_folder>` once (idempotent — "already exists"
  is treated as success), then `rmapi -ni put <pdf> <target_folder>` for each path.
- `refresh`: `rmapi -ni put --content-only <pdf> <target_folder>` for each path.
  (`--content-only` creates the doc if absent, so a first-ever `refresh` still
  works.)
- Shared helper builds the argv, runs the command, captures stderr, and applies the
  conf-empty guard (snapshot/restore + bounded retry). All invocations use `-ni`.
- **Preflight errors** (clear, actionable): `rmapi` not on `PATH`; conf
  missing/empty (point to the pairing URL); `target_folder` blank.

`cli.rs` already calls `deploy()` from `new` and `refresh()` from the regenerate
path — no change there beyond `get_deployer` learning the `"rmapi"` arm.

### Wizard

`wizard.rs` gains two prompts (each prefilled with its default), appended after the
existing questions:

- **Deploy backend:** `none` (default) | `rmapi`.
- **Target folder** (only if `rmapi` chosen): default `/<year>`.

These populate `config.deploy`. The Phase 1 behavior (writing `backend = "none"`)
remains the default, so non-syncing users are unaffected.

## Invocation model (unchanged surface)

```
rmbujo new                    # wizard → generate → deploy() (rmapi: mkdir + put)
rmbujo path/to/rmbujo.toml    # regenerate → refresh() (rmapi: put --content-only)
```

With `backend = "none"` both deploy/refresh are no-ops, exactly as today.

## Error handling & determinism

- Deploy/refresh are **side effects**, separate from PDF generation; they never
  affect the byte-deterministic PDF output or its tests.
- Generation runs to completion and writes all PDFs **before** any upload; a deploy
  failure leaves valid PDFs on disk and surfaces a clear error.
- The conf-empty guard prevents the token-clobber bug from cascading across the ~15
  uploads of a year.
- **Stability invariant (project-wide):** regeneration must keep page count and
  per-index page meaning identical for a given config, so `--content-only` lands
  backgrounds on the right pages. Phase 2b (ICS) must repaint only existing pages —
  a heavy event day clips/overflows within its page, never adds/removes/reorders
  pages.

## Testing

All software is tested via `cargo test`; the device round-trip is the manual spike.

- **Fake `rmapi` shim:** tests prepend a temp dir to `PATH` containing an `rmapi`
  script that appends its `argv` to a log file and exits 0 (or, for negative tests,
  non-zero / empties the conf). Rust tests then:
  - assert `deploy` issues `mkdir <folder>` once then `put <pdf> <folder>` per PDF;
  - assert `refresh` issues `put --content-only <pdf> <folder>` per PDF;
  - assert every invocation includes `-ni`;
  - assert the conf-empty guard restores the snapshot and retries.
- **Preflight tests:** missing `rmapi` (empty `PATH`) and empty conf each produce
  the expected clear error.
- **Config/wizard:** `deploy.backend = "rmapi"` round-trips through TOML; the
  wizard's new prompts populate `deploy` correctly (driven via the existing wizard
  test seam).
- **`get_deployer`:** returns `RmapiDeployer` for `"rmapi"`, `LocalDeployer` for
  `"none"`, errors otherwise.

## Phase 2b (deferred — design intent only)

ICS subscriptions rendered into the day-list / future-log backgrounds, built on the
proven refresh: regenerate with current feed data → `refresh()` re-syncs without
disturbing ink. Its own brainstorm. The stability invariant above is its hard
constraint.

## Rationale

- **rmapi over a native cloud client.** rmapi already implements v4 auth, sync, and
  the `--content-only` blob-swap. Reimplementing the reMarkable cloud API in Rust
  would duplicate a moving target for no benefit. The cost is a shell-out dependency
  and the operational hazards above, which are mitigated, not eliminated.
- **Spike before backend.** The entire value proposition (refresh without losing
  handwriting) is unverified on the v4 cloud + Move. Proving it on one throwaway doc
  is cheap; discovering it doesn't work after building ICS on top would be expensive.
- **Vendor the overlay.** Keeps the open-source repo self-contained and reproducible
  while upstream/nixpkgs catch up; trivially removable later.
