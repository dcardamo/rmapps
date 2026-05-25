# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

inkapp is a Rust framework for building **interactive apps on pen-based document devices**.
The framework is **device-agnostic by design** and intended to support multiple device
families (reMarkable, Supernote, Boox); **reMarkable is the only device supported at the
outset.** The core loop: render a document (PDF) → sync to device → user reads/writes with
the pen → sync annotated ink files back → parse the ink → map strokes to named page regions
→ run app logic → re-render. It turns a passive e-ink reader into an app surface, the way
CGI turned a static server into a web app. See `docs/why.md` and `docs/how-it-works.md` for
the thesis and the full loop.

**Apps must never see anything reMarkable-specific.** An app's entire surface — model,
messages, `update`, `view`, components, connectors — is device-neutral. The framework
absorbs every device detail (coordinate transforms, ink-file formats, transport) behind the
`Device` seam, so the same app runs unchanged on any supported device. If app code ever
needs to know it's running on a reMarkable, that's a framework leak to fix, not an app
concern.

## Commands

All commands run inside the Nix dev shell. The `Makefile` is the source of truth:

```
make test        # nix develop -c cargo test          (whole workspace)
make build       # nix develop -c cargo build
make fmt         # nix develop -c cargo fmt
make fmt-check   # cargo fmt --check  (what the pre-commit hook runs)
make clippy      # cargo clippy --all-targets -- -D warnings  (warnings are errors)
make hooks       # install .githooks (pre-commit = fmt --check)
```

If `direnv` is active (`.envrc` = `use flake`) the shell is already loaded and bare
`cargo` works. Otherwise prefix with `nix develop -c`.

- Single crate: `nix develop -c cargo test -p inkapp-core`
- Single test: `nix develop -c cargo test -p inkapp-harness <test_name>`
- The workspace has both `crates/` and `apps/` members — when changing shared types,
  verify with `cargo test --workspace`, not `-p`, or you will miss app-side breakage.

`make clippy` treats warnings as errors; keep it clean before committing.

## Architecture

It is an **MVU (Model-View-Update) framework**, adapted from Elm/web frameworks to pen
devices. `docs/appdx.md` is the canonical developer-experience spec and the best
architecture read — it explains MVU-for-pen-devices, the worked example, and the design
rationale. The pieces:

- **`crates/inkapp-core`** — the device-agnostic framework. Everything except transport
  lives here. Key modules:
  - `runtime.rs` — the `App` builder (`app(model).connector(cx).update(f).view(g).key(k).build()`)
    and the MVU driver. `App::render` produces the initial doc set; `App::step` runs one
    cycle: refresh connectors → decode ink against the *pre-fold* view + stored manifest →
    fold messages through `update` → re-render → reconcile by key → flush writes.
  - `component.rs` — the `Component` trait: `render` (emits Typst + `<region>` metadata)
    and `decode` (turns attributed ink into `Msg`s). Render and decode are co-located.
  - `manifest.rs` / `embed.rs` — the **manifest**: named regions (bounding boxes in
    PDF points) + version marker + app state, recovered from Typst's laid-out frames and
    **embedded (encrypted) in the PDF**. State lives in the document, not a server DB.
  - `connector.rs` — the `Connector` plugin seam (`refresh`/`flush`, async, `Arc`-shared).
    Network reads happen in `refresh` (into a cache); app-facing methods are sync and hit
    the warm cache; writes enqueue and `flush` drains them with retry.
  - `mode.rs` — the `Mode { ReadOnly, Editable }` axis carried as a component *field*, so
    render and decode branch on the same value.
  - `render.rs` / `world.rs` — Typst compiled **as a library crate** (not the CLI), with a
    multi-file Typst world and the `#region` prelude (`typst/region.typ`). Fonts are
    embedded via `typst-assets` — no system fonts needed.
  - `readback.rs` — `attribute` (strokes → regions by containment) and `guard_version`
    (reject ink from a stale manifest version).
  - `cache.rs` — durable cache primitive wrapping **foyer** (hybrid memory+disk), sha256
    integrity. Used by connectors for warm-restart/offline reads.
  - `crypto.rs` / `secrets.rs` — manifest sealing and the per-user `SecretStore`.
    **Invariant: secrets never go in the manifest or the PDF** — users may share documents.

- **`crates/rm-files`** — pure-Rust reader/writer for the reMarkable `.rm` v6 scene format
  (ink strokes, highlights) and the document bundle. No framework deps.

- **`crates/rm-cloud`** — pure-Rust client for the current reMarkable Cloud sync protocol
  (content-addressed blob store, root ref with compare-and-swap by generation). Exposes
  immutable `Snapshot`s + `diff`, an atomic `commit` (rebase-on-412), rmapi-style path ops
  (`ls`/`get`/`put`/`mkdir`/`mv`/`rm`/`put_content_only`), and a declarative working-set
  `sync` for app loops. Reuses `rm-files` for the `.rmdoc` bundle; owns nothing of the local
  scene format. Tested against an in-process axum fake cloud (behind the `fake` feature) and
  an env-gated live-cloud suite isolated under `rmrs-test/<run-id>`. reMarkable-specific →
  `rm-` prefix. No framework/app deps. Intended to replace shelling out to the `rmapi` CLI
  (the `serve.rs` migration is a later spec). See `docs/rm-cloud-protocol.md`.

- **`crates/inkapp-remarkable`** — the `Device` impl for reMarkable: the PDF↔device
  coordinate transform and `.rm` read/write. The `Device` trait (`device.rs` in core) is
  intentionally minimal — **it covers ink coordinate mapping and parsing only, not
  transport**. Sync/transport (shelling out to `rmapi`) lives in each app's `serve.rs`,
  not the framework. **Naming exception:** this crate is reMarkable-specific and should
  follow the `rm-` prefix convention below (a rename to `rm-…` is pending); until then it
  is the one crate that violates the rule.

- **`crates/inkapp`** — the thin app-authoring **facade**: re-exports the core surface plus
  the default `Remarkable` device, so apps read the way the docs show. Apps depend on this.

- **Connectors** — `inkapp-readwise-reader` (live HTTP + durable cache, cassette mode for
  tests), `inkapp-ics` (read-only calendar feed), `inkapp-localcal` (writable local
  calendar). These are the connector archetypes (read-only feed vs. writable store).

- **`crates/inkapp-harness`** — in-software loop **simulator** and a layers inspector.
  Drives the full loop without hardware by substituting ink at the `Device` seam. This is
  where end-to-end and transform-fidelity tests live. Tests run *without* a device.

- **`apps/reading-queue`** — the worked example from `appdx.md` (Readwise-backed).
  **`apps/agenda`** — the mode-axis example (read-only feed + editable calendar). Each app
  is `lib.rs` (model/msg/update/view + components) + `main.rs` + `serve.rs` (rmapi transport).

- **`spikes/typst-readback`** — legacy proof-of-concept (uses system fonts + `pdftoppm`).
  Not framework runtime; don't model new code on it.

### Two load-bearing invariants

1. **State lives in the document.** The embedded manifest is the authoritative record of
   the document at write time; the handler interprets ink against it with no out-of-band
   store. Don't add a session database.
2. **Secrets never enter a document.** Manifest carries structure only (region names,
   boxes, versions, non-sensitive app state). Credentials live in `SecretStore`.

## Development workflow

This project is built **spec-first** in numbered increments. Each feature has a design
spec in `docs/superpowers/specs/` and a task plan in `docs/superpowers/plans/`. The
**definition of done for any build-order item is updating `docs/appdx.md`** to mark it
built and make the doc true — every spec ends by reconciling appdx. When implementing a
planned item, read its spec and plan first, and update appdx when done.

`docs/` is the design corpus (`why`, `how-it-works`, `glossary`, `appdx`,
`remarkable-pdf-mechanics`, `FUTURE`). `remarkable-pdf-mechanics.md` holds the
on-device-verified rules the sync layer must respect (content-only updates, leading-page
invariant, ink preservation) — consult it before touching transport.

## Conventions

- **Apps are strictly device-agnostic.** App crates never carry a device name, never depend
  on a device crate (`rm-*` / `inkapp-remarkable`), and expose no reMarkable-specific types
  in their API. They depend only on the `inkapp` facade; the framework picks the device.
- **reMarkable-specific crates carry an `rm-` prefix** (e.g. `rm-files`). Anything that
  knows the reMarkable `.rm` format, coordinate space, bundle layout, or `rmapi` transport
  belongs in an `rm-`-prefixed crate. Device-neutral framework code stays in `inkapp-core` /
  `inkapp`. (Current exception: `inkapp-remarkable`, pending rename — see Architecture.)
- Pre-commit hook runs `cargo fmt --check`; an open task list can also block the hook
  (commit only with tasks closed). Implementers do **not** stage `Cargo.lock` — leave it
  to a separate dependency-bump commit.
- The default page geometry is 420×560pt, 16pt margin (override via `App` builder `.page()`).
