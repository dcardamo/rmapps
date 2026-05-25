# Theming: reading aesthetic + code-level `Theme` API

**Date:** 2026-05-25
**Status:** Design approved, pending implementation plan

## Problem

Every inkapp document renders in Typst's stock defaults: Linux Libertine at 12pt,
no heading treatment, no reading rhythm. `runtime.rs::document_source_in` hardcodes
a single `#set text(size: 12pt)` line onto every document. Apps have no way to set a
reading aesthetic, and the framework ships no curated fonts, so output looks like a
LaTeX draft rather than something you'd want to read on an e-ink device.

## Goal

Give inkapp a real reading aesthetic and a reusable, **code-level** theming API:

1. **Embed real fonts** into `InkWorld` so `#set text(font: "...")` resolves a curated
   set deterministically, with no host font search.
2. **A `Theme` type** (font families, size, leading, justify, grayscale tones) with a
   sensible `Theme::reader()` default that emits a Typst styling **prelude**, injected
   in place of the hardcoded `#set text` line. `App` carries a `Theme` (builder
   `.theme(...)`, defaulting to `reader()`).

This is a **pure code API**. It is NOT wired to `inkapp-config` — a parallel
`config-integration` branch owns config wiring and will drive `Theme` from config
later. Baking config in here would collide.

## Non-goals

- No `inkapp-config` integration of any kind.
- No changes to `components/`, transport (`rm-device`/`rm-cloud`/`sync.rs`), or the
  `Device` seam.
- No color. Tones are grayscale by construction (works on every reMarkable device).

## Design

### 1. Vendored fonts

Copy a curated subset of TTFs from `~/git/rmreader/assets/fonts` into a new
`crates/inkapp-core/assets/fonts/`:

| File | Role |
|---------------------------------|--------------------------------------------------|
| `Newsreader-Regular.ttf`        | body serif (default `Theme::reader().body`)      |
| `Newsreader-Italic.ttf`         | body italic — true italic for emphasis/quotes    |
| `Newsreader-SemiBold.ttf`       | body bold weight                                 |
| `Fraunces-Regular.ttf`          | display serif (default `Theme::reader().heading`)|
| `Fraunces-SemiBold.ttf`         | display serif bold                               |
| `HankenGrotesk-Regular.ttf`     | sans for labels — resolvable, not a default family|
| `HankenGrotesk-SemiBold.ttf`    | sans bold                                        |

~7 files, ~700KB. Mono is **not** vendored: `Theme::reader().mono` defaults to
`"DejaVu Sans Mono"`, which `typst-assets` already ships.

`InkWorld` loading: declare an `include_bytes!` array of the vendored TTFs and feed
each through the same `Font::iter(Bytes)` loop that already consumes
`typst_assets::fonts()`, pushing the faces into the same `fonts` vec **before**
`FontBook::from_fonts(&fonts)` is built. The vendored faces and the typst-assets
defaults thus share one book; family lookups (`"Newsreader"`, `"Fraunces"`,
`"Hanken Grotesk"`, `"DejaVu Sans Mono"`) resolve deterministically with no host
font search. Both `InkWorld::new` and `with_sources_and_assets` get the fonts because
`new` delegates to `with_sources_and_assets`.

### 2. `Theme` type — new `crates/inkapp-core/src/theme.rs`

```rust
/// A code-level reading aesthetic: font families, type scale, and grayscale tones.
/// Emits a Typst styling prelude injected into every document's source.
pub struct Theme {
    pub body: String,    // body text font family
    pub heading: String, // heading font family
    pub mono: String,    // monospace / raw font family

    pub size_pt: f64,    // base text size in points
    pub leading_em: f64, // paragraph leading as an em multiple
    pub justify: bool,   // justify body paragraphs

    // Grayscale tones, luma 0 (black) .. 255 (white). u8 makes grayscale structural.
    pub heading_tone: u8,
    pub body_tone: u8,
    pub muted_tone: u8,  // secondary text (quotes, captions)
    pub rule_tone: u8,   // hairlines / quote bar
}
```

`Theme::reader()` default values:

| Field          | Value               |
|----------------|---------------------|
| `body`         | `"Newsreader"`      |
| `heading`      | `"Fraunces"`        |
| `mono`         | `"DejaVu Sans Mono"`|
| `size_pt`      | `11.0`              |
| `leading_em`   | `0.75`              |
| `justify`      | `true`              |
| `heading_tone` | `26`                |
| `body_tone`    | `34`                |
| `muted_tone`   | `110`               |
| `rule_tone`    | `216`               |

A lightweight chained builder for overrides, e.g.
`Theme::reader().size_pt(12.0).justify(false)`. Each setter takes `self` by value and
returns `Self` (`#[must_use]`), mirroring the `App` builder style. `Default for Theme`
delegates to `reader()`.

`Theme::prelude(&self) -> String` emits the styling block that replaces the current
`#set text(size: 12pt)` line. Shape (exact Typst finalized under TDD so it compiles):

```typst
#set text(font: "<body>", size: <size_pt>pt, fill: luma(<body_tone>))
#set par(leading: <leading_em>em, justify: <justify>)
#show heading: set text(font: "<heading>", fill: luma(<heading_tone>))
#show heading.where(level: 1): set text(size: 1.6em)
#show heading.where(level: 2): set text(size: 1.3em)
#show raw: set text(font: "<mono>")
#show quote: it => block(
  inset: (left: 1em),
  stroke: (left: 0.5pt + luma(<rule_tone>)),
  text(fill: luma(<muted_tone>), style: "italic", it.body),
)
```

Page geometry is **not** part of the prelude: `geom` owns `#set page(...)`, the theme
owns type. `document_source_in` continues to emit the `#set page` line, then the
theme prelude in place of the old `#set text` line, then component renders.

### 3. Threading — sibling `&Theme` parameter

`Theme` flows through the render API exactly like `geom` already does — as a sibling
parameter on the `_in*` family:

- `document_source_in(doc, geom, theme)`
- `compile_document_in(doc, geom, theme)`
- `compile_document_in_with_assets(doc, geom, theme, assets)`
- `render_document_in(doc, version, key, geom, theme)`
- `render_document_in_with_assets(doc, version, key, geom, theme, assets)`

The convenience wrappers default the theme to `Theme::reader()`:

- `document_source(doc)` → `document_source_in(doc, PageGeom::default(), &Theme::reader())`
- `compile_document(doc)` → likewise
- `render_document(doc, version, key)` → likewise

`App` gains a `theme: Theme` field and a `BuilderReady::theme(Theme)` setter
(default `Theme::reader()`). `App::render` and `App::step` pass `&self.theme` into
`render_document_in_with_assets` at their two call sites. `App::new` takes the theme
as an additional argument (the `#[allow(clippy::too_many_arguments)]` already present
covers the wider signature; the builder remains the ergonomic path).

Re-export `Theme` from `inkapp-core/src/lib.rs` and from the `inkapp` facade so apps
read `use inkapp::Theme;`.

### 4. Determinism

`InkWorld::today` already returns `None`, and the font set is fixed and embedded, so
identical source produces byte-identical PDF. `Theme::reader()` is a pure constant, so
the defaulting wrappers are equally deterministic. The determinism test below locks
this in.

## Testing (TDD)

All via `nix develop -c cargo test --workspace` (plain `cargo` fails — the image
pipeline pulls dav1d).

1. **Vendored font in the book** (`inkapp-core`, `world.rs` tests): build an
   `InkWorld` and assert its `FontBook` contains the `"Newsreader"` family.
2. **Themed doc compiles to a non-empty PDF** (`inkapp-core`): a document whose source
   uses `#set text(font: "Newsreader")` (or goes through `document_source` with
   `reader()`) compiles and `document_to_pdf` yields non-empty bytes.
3. **Determinism** (`inkapp-core`): render the same document twice via
   `render_document` → byte-identical PDF.
4. **Harness styled golden** (`inkapp-harness`, new test): assemble
   `Theme::reader().prelude()` + a `#set page` line + a content snippet exercising a
   level-1 heading, justified body, a block quote, and a raw span; `compile_to_document`,
   then `inspect(&doc, &manifest, &[])`, then `assert_golden("theme_reader", &png)`.
   First run writes and fails (review the PNG, commit it, re-run green).

Existing exerciser/e2e goldens build raw `#set page` sources via `compile_to_document`
and bypass `document_source_in`, so they are unaffected by the new default. Any
byte-exact golden that *does* ride a defaulting wrapper must be regenerated inside the
devshell (the new reader() styling is the intended output, not a regression).

## Files touched

- New: `crates/inkapp-core/assets/fonts/*.ttf` (vendored), `crates/inkapp-core/src/theme.rs`.
- Modified: `crates/inkapp-core/src/world.rs` (load vendored fonts + book test),
  `crates/inkapp-core/src/runtime.rs` (thread `theme`, builder `.theme()`, App field),
  `crates/inkapp-core/src/lib.rs` (module + re-export), `crates/inkapp/src/lib.rs`
  (facade re-export), `docs/appdx.md` (record the capability — final step).
- New tests as above.

## Process / conventions

- Work in a git worktree of `~/git/inkapp` (currently `jobless-frog`).
- Track progress in `.tasks.json`; keep the native task list empty (the commit hook
  blocks on open native tasks).
- Do **not** stage `Cargo.lock`.
- Definition of done includes updating `docs/appdx.md` to mark the theming capability
  built and make the doc true.
