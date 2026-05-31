# inkapp — Spec #1: Monorepo Foundation + Typst Spike + Docs

**Date:** 2026-05-22
**Status:** Approved (design); plan pending

## Context

`inkapp` is a framework for building **applications for pen-based document devices**
(reMarkable first; Supernote/Boox later). An app presents itself as a document the user
reads and writes on with a pen; the device syncs the annotated document back; code
running on the desktop/server side processes the ink and produces the next document or
takes actions. It is **web 1.0 / CGI for ink**: PDF + `.rm` instead of HTML, server-side
processing instead of JavaScript, sync instead of HTTP.

This loop is already proven in two reference repos (`~/git/rmreader`, `~/git/rmbujo`)
that render PDFs, embed state, push via `rmapi`, read highlighter strokes back, and act
on them. **Those repos are reference material only** — the framework will be built fresh,
and the apps (e.g. a journal app, `bujo`) will be rebuilt on top of it later. The one
exception is `~/git/rmfiles`, a clean pure-Rust `.rm` v6 parser, which we reuse.

### Key prior decisions (from brainstorming)
- **Rust monorepo**, not Python. No Chromium and no other heavy runtime dependencies.
  (Note: `fulgur`/Blitz, used by the reference apps, is a pure-Rust HTML/CSS engine — *not*
  Chromium — but we are choosing Typst regardless; see below.)
- **Typst** is the intended render engine, pending this spike. The honest case for it:
  compiled as a *library*, Typst exposes laid-out document frames with element positions,
  which makes recovering region geometry tractable; it is purpose-built for beautiful
  typesetting; and it avoids an HTML/CSS layer. Its weakness — pulling HTML *content* into
  Typst — is a spike bar.
- **Apps are device-agnostic.** Device-specific *infrastructure* crates may carry a device
  name (`rmfiles`); *apps* never do. The journal app is `bujo`, not `rmbujo`.
- The per-app processing code is called a **handler** (CGI mental model: a handler consumes
  the synced document and emits the next one).
- **State lives in the document** (an embedded manifest), never secrets — users may share
  documents with third parties.

## Goals (this spec)

Stand up the greenfield Rust monorepo and **prove Typst can power the framework**, while
writing the foundational docs in parallel.

## Non-goals (deferred to later specs)

- Framework crates: the `handler` API, render trait, sync trait, device trait.
- The device abstraction layer (generalizing `.rm`/`rmapi` for Supernote/Boox).
- Building the `bujo` app or any other app.
- CLI and desktop binaries.

These are named here only to keep the foundation from blocking them; none are built now.

## A. Monorepo foundation

```
inkapp/
  Cargo.toml          # cargo workspace
  flake.nix  Makefile # one dev shell; one `make test`; clippy; fmt
  crates/
    rmfiles/          # absorbed from ~/git/rmfiles — the .rm v6 parser (reuse, don't rebuild)
  spikes/
    typst-readback/   # the spike crate (see section B)
  docs/               # narrative docs (see section C)
```

- **Absorb `rmfiles`** as-is into `crates/rmfiles` (clean, pure-Rust, hard-won `.rm` v6
  knowledge). Preserve its tests. After it builds in the workspace, `~/git/rmfiles` is
  deleted by the user.
- **One nix flake** providing the Rust toolchain, the fonts Typst needs at render time
  (Typst itself is a Cargo dependency of the spike, not a nix package), and the patched
  `rmapi` (the v4 cloud break requires the PR #63/#65 patch; mirror the overlay approach
  used by `rmreader`/`rmbujo`'s flakes).
- **`make test`** runs the workspace test suite and the spike's automated checks. `make
  clippy` and `make fmt` mirror the reference repos' Makefiles.
- The directories `cli/`, `desktop/`, and the framework crates are intentionally absent;
  they arrive with their own specs.

## B. Typst spike (`spikes/typst-readback`)

Uses **Typst as a library crate**, not the `typst` CLI, so the spike can introspect the
laid-out document directly.

### Bars (the framework needs all "must" bars to pass to adopt Typst)

1. **(must) Region rects.** Render a document containing labelled elements; recover their
   bounding boxes in **final PDF-point coordinates** from Typst's laid-out frames. This is
   what lets the framework embed a manifest mapping page regions → meaning, so pen strokes
   read back later can be mapped to intent. **Make-or-break.**
2. **(must) Tappable links.** Internal page→page links and external URL links survive in
   the output PDF as real annotations.
3. **(must) HTML → Typst.** Take a representative chunk of HTML (e.g. an article body) and
   get it into a Typst document acceptably. Evaluate the realistic path (convert
   HTML → Typst markup) and document its limits.
4. **(must) On-device quality.** A sample page looks great on the reMarkable.
5. **(must) content-only invariant.** A Typst-produced PDF respects the leading-page-index
   rule from `remarkable-pdf-mechanics.md`, so re-rendering and re-pushing via
   `rmapi --content-only` does not scramble existing on-device ink.
6. **(stretch) Full loop.** Render → apply a real highlighter stroke on-device → read the
   `.rm` back via `rmfiles` → confirm the stroke lands in the expected region rect. Only if
   cheap; the `.rm` read path is already proven independently.

### Method
- **On-device steps (bar 4, and the device half of bar 5):** model the push on rmreader's
  `deploy/rmapi.rs`, which is known-good against the v4 cloud.
- **Automation:** bars 1, 2, 3, and the PDF-structure half of bar 5 run under `make test`
  with no device attached. Device-dependent steps are a documented manual run so the spike
  remains a real automated test that degrades gracefully without hardware.
- **Candidate approaches for bar 1** (the spike picks one and records why): Typst
  `query`/`locate`/labels surfaced via the compiler's introspection; reading element
  positions from the laid-out frames returned by the Typst library; deriving rects from PDF
  named destinations / link annotations as a fallback.

### Output
A dated findings document in `docs/superpowers/spikes/` containing a clear **go/no-go on
Typst**, the chosen approach for bar 1, the HTML→Typst limits found, and — if no-go — what
adopting Typst would require.

## C. docs/ (in parallel with the spike)

Foundational narrative, writable concurrently since most of it does not depend on spike
results:

- `docs/why.md` — the problem; why pen-based document devices deserve a real app framework;
  the limits of static-only documents.
- `docs/how-it-works.md` — the render → sync → readback → process → re-render loop; the
  state-in-document manifest; the secrets-never-in-documents rule.
- `docs/inspiration.md` — web 1.0 / CGI; TUI and web frameworks as prior art; Typst.
- `docs/glossary.md` — app, **handler**, document/bundle, manifest, region, readback, sync,
  device.
- **Move** `remarkable-pdf-mechanics.md` (currently in `~/git/rmbujo/docs/`) into `docs/`;
  it is load-bearing reference for the whole project.

## Done when

- The monorepo builds and all tests pass, with `rmfiles` absorbed and its tests green.
- The spike delivers a documented go/no-go on Typst: rects, links, HTML→Typst, and the
  content-only structure proven automatically; on-device quality verified on hardware or
  explicitly flagged as a pending manual check.
- The `docs/` narrative set (why, how-it-works, inspiration, glossary) exists, and
  `remarkable-pdf-mechanics.md` has been moved in.

## Risks

- **Bar 1 (region rects) is the project's central technical bet.** If Typst cannot yield
  reliable PDF-coordinate geometry for labelled elements, the readback model needs rework
  and Typst may not be the engine. The spike exists to settle this before more is built.
- **HTML→Typst (bar 3)** may prove lossy; the spike documents how lossy and whether it's
  acceptable for the content sources we care about.
