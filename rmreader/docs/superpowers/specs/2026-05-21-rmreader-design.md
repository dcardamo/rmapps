# rmreader — design

**Date:** 2026-05-21
**Status:** approved (Phase 1)

A CLI that turns your [Readwise Reader](https://readwise.io/reader_api) library and feed
into two beautiful, hyperlinked, reader-optimized PDFs and uploads them to the reMarkable
cloud. Built to make heavier use of a reMarkable Paper Pro / Paper Pro Move as a reading
device. Reuses the patterns proven in the sibling project `../rmbujo`: Rust + fulgur
(Blitz + krilla) HTML→PDF with no headless browser, `rmapi` for cloud sync, TOML config,
an interactive wizard.

## Goals

- Read the Readwise Reader **Library** (locations `new` + `later` + `shortlist`) and
  **Feed** (location `feed`).
- Produce two PDFs — `Library.pdf` and `Feed.pdf` — each a three-tier hyperlinked
  document: an index, one summary card per item, then the full article text.
- Newest item first, oldest last (sorted by `saved_at` descending), capped at 100 items
  per PDF.
- Content is reader-optimized like Readwise's reader view: clean typography, content
  images in color, **no ads, banners, or tracking junk**.
- Upload both PDFs to the reMarkable cloud via `rmapi`, non-destructively on regenerate
  so on-device handwriting survives.
- Do not foreclose a future phase that reads on-device annotations to drive actions
  (e.g. archive). Phase 1 lays the seams; it does not build that phase.

## Non-goals (Phase 1)

- Reading or acting on device annotations (designed-for, not built).
- Highlights/notes sync back to Readwise.
- Arbitrary themes/layout configurability beyond a single reader theme.
- Incremental/`updatedAfter` syncing (full sweep within the capped scope is fine at
  Phase-1 volumes).

## Target devices

reMarkable Paper Pro and Paper Pro Move are **color** e-ink devices. We keep images in
color (no desaturation). Geometry is copied from rmbujo's `device.rs`:

| key               | name                        | px (portrait) | ppi |
|-------------------|-----------------------------|---------------|-----|
| `paper-pro-move`  | reMarkable Paper Pro Move    | 954 × 1696    | 264 |
| `paper-pro`       | reMarkable Paper Pro         | 1620 × 2160   | 229 |

Page size in PDF points is `px / ppi * 72`.

## Readwise Reader API (verified 2026-05-21)

- **Auth:** simple access token (not OAuth). User obtains it from
  `https://readwise.io/access_token`. Sent as header `Authorization: Token <token>`.
  Validate with `GET https://readwise.io/api/v2/auth/` → `204` means valid.
- **List:** `GET https://readwise.io/api/v3/list/`.
  Params used: `location` (one of `new`, `later`, `shortlist`, `archive`, `feed`),
  `withHtmlContent=true` (adds the `html_content` field — the parsed reader content,
  inline in the list response, so no per-article request is needed), `limit` (1–100,
  default 100), `pageCursor`. Pagination: response carries `nextPageCursor`; loop until
  it is null.
  Document fields used: `id`, `url`, `source_url`, `title`, `author`, `site_name`,
  `category`, `location`, `summary`, `image_url`, `word_count`, `reading_time`,
  `published_date`, `saved_at`, `created_at`, `updated_at`, `html_content`.
- **Rate limit:** the LIST endpoint is 20 requests/minute. On `429`, read the
  `Retry-After` header (seconds) and sleep before retrying.
- **Future archive phase:** `PATCH https://readwise.io/api/v3/update/<id>/` with
  `{"location": "archive"}` (or `new`/`later`/`feed`). Out of scope for Phase 1.

## Architecture (chosen approach)

**Single-pass, one HTML document per PDF.** For each PDF we build one large HTML string
containing the index, every card, and every article as sections, and render it once with
fulgur. This is required for cross-section links: fulgur builds its `DestinationRegistry`
per render pass, so an `<a href="#item-42">` only resolves to `<section id="item-42">`
when both live in the same render. Bookmarks, page numbers, and the page→doc-id manifest
all derive from that single pass.

Rejected alternatives:
- *Render sections separately and merge with lopdf* — fulgur resolves anchors per render,
  so every cross-document link would break and we'd have to inject link annotations and
  named destinations by hand. More code, more fragile, no benefit.
- *Cards-only with external source links* — no full text on-device, and external links
  do not open on reMarkable (no browser). Fails the "great for reading" goal.

### fulgur capabilities confirmed (from fulgur 0.6.0 source)

- **Internal links work.** A pre-pass records every block-level element with a (trimmed,
  non-empty) `id` attribute into a `DestinationRegistry`; `<a href="#id">` resolves to a
  real PDF `XyzDestination` (jumps to that page/position). Destination ids must be on
  **block-level** elements (`section`, `div`, `h1`–`h6`). Unresolved anchors log a
  warning and are skipped — a content error, not a render error. (Implication: anchors
  go on block elements.)
- **External links work.** `<a href="https://…">` becomes a real PDF link action.
- **No opt-in needed for links** — `emit_link_annotations` runs unconditionally in the
  render path.
- **Native PDF outline/bookmarks** available via GCPM CSS (`bookmark-level`,
  `bookmark-label`), opt-in with `Engine::builder().bookmarks(true)`. reMarkable shows
  this as a navigation panel, giving a device-wide TOC from any page.
- **Images:** krilla embeds PNG/JPEG/GIF and SVG, in color. **WebP/AVIF are not
  supported** and must be transcoded.
- **Networking is offline-first:** fulgur's `NetProvider` only serves `file://` URLs
  inside the configured base dir; `http(s)://` and `data:` are silently dropped. So
  remote `<img>` URLs are never fetched by fulgur — we must pre-fetch images ourselves
  and supply the bytes via `AssetBundle`.

## Module layout

Mirrors rmbujo's conventions.

```
src/
  main.rs            # thin: calls cli::main()
  lib.rs             # module declarations
  cli.rs             # `rmreader init` (wizard) | `rmreader <config.toml>` (regen+sync)
  config.rs          # Config struct, TOML load/dump, validate()
  device.rs          # MOVE / PRO geometry (copied from rmbujo)
  wizard.rs          # dialoguer prompts; pure assemble() + run_wizard(); token validate
  readwise/
    mod.rs           # Document type; ReaderClient (fetch_library / fetch_feed)
    http.rs          # HttpTransport trait + ureq impl; pagination; 429/Retry-After
  content.rs         # sanitize html_content; extract/fetch/rewrite/transcode <img>
  assemble.rs        # build 3-tier HTML doc, anchors, nav, bookmark CSS; emit manifest
  manifest.rs        # manifest types + writer (page→doc-id; future annotation seam)
  render.rs          # fulgur render: reader CSS, AssetBundle, bookmarks(true)
  generate.rs        # orchestrate: fetch → content → assemble → render → manifest → deploy
  deploy/
    mod.rs           # Deployer trait + get_deployer (copied from rmbujo)
    rmapi.rs         # rmapi backend incl. token-clobber guard (copied from rmbujo)
    local.rs         # "none" backend: no-op (copied from rmbujo)
templates/           # askama: base, index, card, article, nav
themes/              # reader.toml (reader-optimized palette)
assets/fonts/        # serif body font + sans UI/metadata font
nix/overlays/        # rmapi.nix (copied from rmbujo)
```

## Config schema

`rmreader.toml`, passed as a CLI argument, living **beside the output** (like rmbujo). It
holds the Readwise token, so it is gitignored as belt-and-suspenders. Each invocation
targets its own config + output dir + token, which makes future multi-user support a
matter of running once per user's config — no global state.

```toml
device = "paper-pro-move"          # or "paper-pro"
output_dir = "."                   # PDFs + manifests written here (relative to config)

[readwise]
token = "..."                      # from readwise.io/access_token

[library]
locations = ["new", "later", "shortlist"]
max_items = 100

[feed]
enabled = true
max_items = 100

[images]
enabled = true

[deploy]
backend = "rmapi"                  # or "none"
library_folder = "/Reader"         # reMarkable cloud folder for Library.pdf
feed_folder = "/Reader"            # reMarkable cloud folder for Feed.pdf
```

`validate()` runs before any network or render: device known; `library.locations` ⊆
{new, later, shortlist, archive}; `deploy.backend` ∈ {none, rmapi}; token non-empty;
rmapi folders non-empty when backend is rmapi. Fail fast with clear messages.

## CLI

- `rmreader init` — interactive wizard. Prompts for device, output dir, Readwise token
  (with a link to `readwise.io/access_token`), validates the token via `GET /api/v2/auth/`,
  prompts for library/feed caps and deploy backend/folders. Writes `rmreader.toml` into
  the output dir, then runs a full generate + deploy.
- `rmreader <path/to/rmreader.toml>` — load config, validate, regenerate both PDFs, and
  re-sync with `rmapi put --content-only` (preserves on-device handwriting).
- `rmreader` (no args) — print help.

## Readwise client

Same testability seam as rmbujo's `RmapiRunner`: a low-level `HttpTransport` trait
(`get(url, token) -> Response { status, retry_after, body }`) so pagination, sorting, and
rate-limit logic are unit-tested against a fake transport; the real impl uses **ureq**
(rustls backend — no OpenSSL system dependency).

- `fetch_library(cfg)`: one sweep per configured location, each paginating via
  `nextPageCursor` with `withHtmlContent=true&limit=100`; merge results, dedupe by `id`,
  sort by `saved_at` descending, take `library.max_items`.
- `fetch_feed(cfg)`: one sweep of `location=feed`, same pagination/sort/cap.
- On `429`: sleep `Retry-After` seconds, retry. At Phase-1 volumes (cap 100, page size
  100) this is 1–2 requests per location, rarely hitting the limit.

## Content pipeline (`content.rs`)

Readwise's `html_content` is already the de-cluttered reader view (ads/banners removed),
so we mostly inherit clean content and add image handling plus a safety pass:

1. Parse `html_content`; collect `<img>` source URLs.
2. Fetch each image (ureq) with guards: drop 1×1 / sub-2px tracking pixels, non-image
   content types, and oversized files (configurable ceiling).
3. Transcode WebP/AVIF → PNG via the `image` crate; PNG/JPEG/GIF/SVG pass through.
4. Add bytes to the fulgur `AssetBundle` under a generated key; rewrite the `<img src>`
   to that key. Images that fail any step are dropped (warning, non-fatal).
5. Sanitize: strip `<script>`, `<iframe>`, event-handler attributes, and other
   non-content nodes (allowlist-based; e.g. via `ammonia`, with a rewrite step for the
   `src` remap).

When `images.enabled = false`, skip fetching and drop all `<img>`.

## PDF assembly (`assemble.rs`) + manifest

One HTML document per PDF, three sections:

- **§1 Index** — compact list, one row per item (title, author/site, reading time). Each
  row is `<a href="#item-<id>">` jumping to that item's card.
- **§2 Summary cards** — one page per item: `<section id="item-<id>">` with title, author,
  site, summary, reading time, and a "Read →" link `<a href="#article-<id>">`.
- **§3 Full articles** — `<section id="article-<id>">` per item, full cleaned content,
  flowing across as many pages as needed; `break-before: page` so each starts fresh.

Navigation bar at the top of each section's content: `Home` → `#index`, `‹ Prev` /
`Next ›` → adjacent item ids (item-level, not page-level), and on article pages `↑ Card`
→ that item's card. (Per-page nav across long articles depends on the running-header
spike below; the section-level nav plus the native bookmark outline is the guaranteed
fallback.) `bookmark-level` / `bookmark-label` CSS on article headings populates the
device's native outline panel.

Alongside each PDF, write a sidecar manifest (`Library.manifest.json` /
`Feed.manifest.json`) mapping each `doc_id` → `{ title, url, card_page,
article_page_range }`. **This is the seam for the future annotation phase:** pull the
annotated PDF back with `rmapi get`, detect which pages gained ink/highlights, map page →
`doc_id` via the manifest, and `PATCH /api/v3/update/<id>/` to archive (or other action).
The `Deployer` trait keeps room for a future `fetch` method; not implemented in Phase 1.

## Render (`render.rs`)

Copied structure from rmbujo. Reader-optimized CSS: generous measure, serif body font,
comfortable line-height and margins tuned for e-ink; sans font for nav/metadata. Embed
fonts in the `AssetBundle`. `Engine::builder().bookmarks(true)`, page size from device
geometry, color images. Two renders per run: Library, Feed.

## Deploy

rmbujo's `rmapi` backend copied near-verbatim, including the token-clobber guard
(snapshot a good conf, restore if rmapi blanks it). `deploy` uploads both PDFs with
`rmapi -ni put` (after an idempotent `mkdir`); `refresh` (regenerate path) uses
`put --content-only` so the background swaps without touching handwriting. The `none`
backend is a no-op (PDFs are already on disk).

## Error handling

- `validate()` catches config errors before any network/render.
- Missing/invalid token fails fast with a clear message and the access-token URL.
- Per-image fetch/transcode failures are warnings; the article still renders.
- Unresolved internal anchors are a content bug — caught by tests, fulgur warns at
  runtime.
- rmapi failures surface with the failing command; the token-clobber guard retries once.

## Testing

rmbujo-style, no manual testing:

- **Readwise client:** fake `HttpTransport` fixtures — multi-page pagination, location
  merge + dedupe, `saved_at` sort, cap, `429`/`Retry-After` handling.
- **Content pipeline:** tracking-pixel drop, WebP→PNG transcode, sanitize (script/iframe
  removal), `img src` rewrite to asset key, images-disabled path.
- **Assembly:** correct anchors/ids, nav targets point at adjacent items, manifest page
  ranges correct.
- **PDF links:** lopdf assertion that internal anchors resolve to destinations in the
  rendered PDF.
- **Visual regression:** golden images for index, card, and article layouts
  (`make update-goldens`).
- **Deploy:** fake `RmapiRunner` verifies the `mkdir` + `put` / `put --content-only`
  command sequences for both PDFs.

## Nix / Make

Reuse rmbujo's `flake.nix`, `nix/overlays/rmapi.nix`, and `Makefile`
(`test`/`build`/`clippy`/`fmt`/`update-goldens`/`hooks`). Dev/build inputs already include
`poppler-utils`, fonts, `rmapi`. ureq's rustls backend means no extra system TLS deps.
`.gitignore`: `/target`, `/result`, `**/*.rs.bk`, `rmreader.toml`, `**/rmreader.toml`.

## Dependencies (added beyond rmbujo's set)

- `ureq` (rustls) — HTTP client for the Readwise API and image fetches.
- `serde_json` — parse API responses.
- `url` — image URL resolution/validation.
- `ammonia` — HTML sanitize (allowlist).
- `scraper` — HTML parse/walk for `<img>` extraction + rewrite.
- `image` — transcode WebP/AVIF → PNG (also already a rmbujo dev-dep).

Reused from rmbujo: `fulgur`, `askama`, `serde`, `toml`, `clap`, `dialoguer`, `anyhow`,
`chrono`, and `lopdf` + `image` as dev-deps.

## Spike (resolve during planning)

**Do `<a>` links inside fulgur GCPM running headers emit clickable annotations on every
page?** If yes, the nav bar is tappable on every page of a long article. If no, fall back
to per-section nav plus the native bookmark outline (device-wide TOC from any page). The
design works either way. Record the result in
`docs/superpowers/spikes/2026-05-21-fulgur-running-header-links.md`.

## Future phase (designed-for, not built)

Annotation-driven actions: `rmapi get` the annotated PDFs → detect per-page ink/highlights
→ map page → `doc_id` via the sidecar manifest → `PATCH /api/v3/update/<id>/` (e.g.
`location: archive`). Requires only additions: a `fetch` method on `Deployer`, an
annotation-detection module, and a Readwise `update` client call. No Phase-1 structure
needs to change.
