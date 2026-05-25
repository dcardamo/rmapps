# HTML→Typst article content pipeline (`crates/inkapp-content`)

**Date:** 2026-05-25
**Branch:** `html2typst`
**Status:** design approved, ready for implementation plan

## Goal

A reusable HTML→Typst article pipeline so apps render real Readwise articles
(structured `html_content`) instead of whitespace-split plaintext. Delivered as a
new crate `crates/inkapp-content` with two pieces:

1. A **pure HTML→Typst converter** that parses Readwise `html_content` into
   structured Typst markup (headings, bold/italic, links, lists, blockquotes,
   code/pre, paragraphs, figures), sanitizing dangerous content first.
2. An **`Article` component** (`impl inkapp_core::Component`) that replaces
   `HighlightableText` for real articles: it still subdivides highlightable prose
   into per-token `tok-<i>` regions so freeform-highlighter ink decodes back to
   span text, renders prior highlights pre-marked, and decodes to coalesced span
   strings.

The reading-queue app is rewired onto `Article` (HTML path when `html_content` is
present, plaintext fallback otherwise).

## Non-goals / seam with the parallel image worktree

This crate does **not** fetch images or serve asset bytes. The only seam with the
parallel `imgfetch` worktree is the **image contract**: for each `<img src=URL>`
the converter emits `#image("/assets/{key}.png", width: 100%)` where
`key = first 16 hex chars of sha256(URL)`, and returns a `Vec<(key, url)>` of every
referenced image. The image worktree fetches those URLs and teaches `InkWorld` to
serve `/assets/*`. Tests here are network-free and use image-free HTML for any path
that compiles Typst.

## Crate & dependencies

New workspace member `crates/inkapp-content`:

- `inkapp-core` — `Component`, `RenderCx`, `RegionInk`, `Manifest`, the shared
  token recipe and decode helpers (below).
- `scraper` — html5ever-backed DOM, robust to malformed real-world HTML. Already
  resolved in `Cargo.lock`.
- `sha2` — image keys. Already a direct dep of `inkapp-core`.

Modules: `convert.rs` (pure transform) and `article.rs` (the Component).

## The converter — `convert(html: &str, highlights: &[String]) -> Converted`

Pure. Parses with `scraper`, walks the DOM in document order, emits Typst.

```rust
pub struct Token { pub text: String, pub block: usize }

pub struct Converted {
    pub typst: String,                  // ready to drop into a flow
    pub tokens: Vec<Token>,             // ordered; index i <-> region tok-<i>
    pub images: Vec<(String, String)>,  // (key, url), deduped by key, first-seen
}
```

- `tokens` is the decode map: token index `i` corresponds to region `tok-<i>`.
- `block` increments at every **block-level** element so highlight coalescing never
  merges across blocks; inline elements keep the current block id.
- `highlights` (matched by token string, same semantics as
  `HighlightableText::with_highlights`) decides which tokens render pre-marked.

### Sanitization — whitelist by construction

No blocklist pre-pass. Because output is Typst (not passed-through HTML), a
whitelist translator cannot leak anything by construction. The walker:

- **Drops entire subtrees** of `script, style, iframe, noscript, object, embed,
  form`, including their text (a `<script>` body never surfaces as visible text).
- **Never reads** `on*`, `style`, `class`, or legacy presentational attributes
  (`align`, `valign`, `bgcolor`, `color`, `face`, `width`, `height`).
- Treats **unknown tags as transparent**: recurse into children and emit their
  text (browser-like).

This covers the same threat set as old `rmreader` Pass-2 (no script execution, no
`font-family` override blanking text under the offline renderer, no navigation, no
remote/`data:` loads) with no blocklist-completeness gap.

## Token recipe — generalized and shared in `inkapp-core`

The load-bearing per-token region recipe (proven in Typst 0.14.2; see the
`span-level-regions-work-in-typst` memory) gets one home, reused by both
`HighlightableText` and the converter:

- `inkapp_core::components::esc_typst_str` becomes `pub`.
- Add `token_region(index: usize, t_let_expr: &str, highlighted: bool) -> String`
  — emits the recipe
  `#box[#let t = <expr>; #context [#metadata((name: "tok-<i>", page: …, x: …, y: …, w: measure(t).width / 1pt, h: measure(t).height / 1pt)) <region>]<#t | #highlight[#t]>]`.
- Add `highlighted_token_indices(n: usize, ink: &[RegionInk], manifest: &Manifest)
  -> Vec<usize>` — the highlighter-overlap detection (a `tok-<i>` region overlapped
  by a highlighter stroke's bbox).
- `HighlightableText::render`/`read` are refactored onto these helpers and remain
  **byte-identical** for the plain case (its `t_let_expr` is the quoted string
  `"esc"`), guarded by existing golden/byte tests.

Inline styling rides in `t_let_expr` so both `measure(t)` and display reflect it:

| Style        | `t_let_expr`          |
|--------------|-----------------------|
| plain        | `"esc"`               |
| bold         | `strong("esc")`       |
| italic       | `emph("esc")`         |
| bold+italic  | `strong(emph("esc"))` |
| inline code  | `raw("esc")`          |
| link text    | `underline("esc")`    |

Every prose token stays an individually highlightable region regardless of styling.
`esc` is `esc_typst_str(token)` (escape `\` and `"` only — string-literal context).

## Structural element mapping

| HTML                       | Typst emitted                                          | Highlightable    |
|----------------------------|--------------------------------------------------------|------------------|
| `<h1>`–`<h6>`              | `#heading(level: n)[ …token boxes… ]` (n clamped 1–6) | yes (tokens)     |
| `<p>`                      | token boxes; blocks separated by `#parbreak()`         | yes              |
| `<strong>/<b>`, `<em>/<i>` | inline style folded into each token's `t`              | yes              |
| `<a>`                      | `underline("…")` tokens — **no href, no nav**          | yes              |
| `<ul>` / `<ol>`            | `#list(…)` / `#enum(…)`, each item = token boxes       | yes              |
| `<blockquote>`            | `#quote(block: true)[ …tokens… ]`                      | yes              |
| `<code>` (inline)         | one `raw("…")` token                                   | yes (one token)  |
| `<pre>` / `<pre><code>`   | `#raw(block: true, "…")` — literal                     | no (code)        |
| `<img src=URL>`           | `#image("/assets/{key}.png", width: 100%)`             | n/a              |
| `<figure>`+`<figcaption>` | `#figure(image(…), caption: [#"…"])`                   | caption plain    |

Block-level elements bump the `block` id; only `http(s)` `<img>` srcs produce an
`#image` + `(key, url)` pair (non-http srcs are dropped as unfetchable). Bare
`<img>` (not inside `<figure>`) emits a standalone `#image`.

## The `Article` component — `article.rs`

```rust
pub struct Article<M> {
    converted: Converted,
    on_highlight: Box<dyn Fn(&str) -> M>,
}

impl<M> Article<M> {
    pub fn new(
        html: &str,
        highlights: &[String],
        on_highlight: impl Fn(&str) -> M + 'static,
    ) -> Self;

    /// Coalesced highlighted span strings (document order).
    pub fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<String>;

    /// The image contract seam: (key, url) for every referenced image.
    pub fn images(&self) -> &[(String, String)];
}

impl<M> Component for Article<M> {
    type Msg = M;
    fn render(&self, _cx: &mut RenderCx) -> String; // returns stored converted.typst
    fn decode(&self, ink, manifest) -> Vec<M>;      // read().map(on_highlight)
}
```

`Article::new` calls `convert(html, highlights)` once and stores the result. Render
returns the stored Typst (token indices are fixed at convert time, consistent with
`HighlightableText`'s enumerate-based indexing). `images()` exposes the seam for the
framework/image worktree.

### Coalescing in `read`

1. `highlighted_token_indices(tokens.len(), ink, manifest)`.
2. Group into runs of **index-adjacent** tokens that **share a `block` id**.
3. Join each run's token strings with a single space → one span string.
4. Return `Vec<String>` in document order.

A swipe over bold-inside-prose ("important note") yields `["important note"]`;
non-contiguous picks stay separate; runs never cross block boundaries. This is the
reusable-content-component escape hatch (`Box<dyn Fn>`) that appdx explicitly
sanctions for content-derived messages.

## reading-queue rewire

`ArticleBody` renders via `inkapp_content::Article` when `a.html_content` is `Some`
(real structured render), else falls back to whitespace-split `HighlightableText`
(existing behavior preserved). `Msg::Highlighted` is now emitted once per
**coalesced span** instead of per token; reading-queue tests are updated to match.

Existing cassettes carry no `html_content`, so they keep the plaintext path and
`cargo test --workspace` stays green without the image worktree. One new test
exercises an **image-free** structured `html_content` through `Article`. The
`inkapp_content::Article` vs `inkapp_readwise_reader::Article` name clash is
resolved by an import alias.

## Tests (TDD, no network)

- **Converter unit tests**, one per construct: heading, paragraph, bold, italic,
  link (text only, no href/nav), `<ul>`, `<ol>`, blockquote, inline code, `<pre>`,
  figure/img → key + pair. Assert emitted Typst + token list + image pairs.
- **Sanitizer tests**: `script/style/iframe/object/embed/form/noscript` bodies
  dropped; `on*` and inline `style` never appear in output; surrounding text
  survives.
- **Coalescing unit tests**: contiguous → one span; gap → split; block boundary →
  no merge; bold-in-prose → merged.
- **Harness render→recover→decode test** (image-free; mixes heading + bold + list +
  paragraph): compile → `recover_regions` → synthetic highlighter swipe over chosen
  token rects → `attribute_page` → `decode`/`read` returns the expected coalesced
  spans; plus a "prior highlights render pre-marked" assertion.
- **inkapp-core**: existing `HighlightableText` golden/byte tests guard the
  byte-identical refactor.
- Final gate: `cargo test --workspace` green.

## Repo conventions

- Add `crates/inkapp-content` to workspace `members`.
- Track progress in a co-located `.tasks.json`; delete native tasks so the
  `pre-commit-check-tasks` hook gate stays clear (see
  `subagent-dev-hook-and-tasklist-gotchas` memory).
- Commit via the `.githooks` form
  (`git -c core.hooksPath=.githooks commit -m "…"`).
- **Do not stage `Cargo.lock`.**
- Final step: mark the HTML→Typst content capability **built** in `docs/appdx.md`.
