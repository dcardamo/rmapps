# rmreader

Turn your [Readwise Reader](https://readwise.io/read) library and feed into two
beautiful, hyperlinked, reader-optimized PDFs — and sync them to your reMarkable
Paper Pro, where what you do on the page flows back to Readwise.

rmreader makes a reMarkable a first-class reading device for your Readwise queue.
It pulls your saved articles, renders them as clean editorial PDFs (no ads, no
banners, no tracking junk), uploads them to the reMarkable cloud, and — on the
next sync — reads your on-device highlights and triage decisions back out and
applies them to Readwise.

> **This is a library crate.** `rmreader` is the rendering/read-back engine inside
> the [`rmapps`](../../README.md) workspace; the user-facing command is `rmapps
> reader`. Configuration lives in the unified `~/.config/rmapps/config.toml` under
> a `[reader]` section. Everything below describes the engine and that command.

<p align="center">
  <img src="docs/screenshots/index.png" alt="Feed index: a typographic table of contents, newest first" width="30%">
  &nbsp;
  <img src="docs/screenshots/article.png" alt="An article page with the nav bar and the highlightable action band" width="30%">
  &nbsp;
  <img src="docs/screenshots/article-links.png" alt="An article page showing inline links and byline" width="30%">
</p>

<p align="center"><em>Left: the index hub. Center & right: article pages with the
tappable nav bar (Home / Prev / Next) and the highlightable <code>INBOX · ARCHIVE ·
LATER · DELETE</code> action band.</em></p>

## Why

reMarkable is a wonderful surface for reading and marking up long text, but
getting your reading queue *onto* it — and getting your reactions back *off* it —
is tedious. rmreader closes that loop:

- **Read on the device, not in a browser.** Your whole Library and Feed become two
  self-contained PDFs with full article text, so there's nothing to tap through to
  and no network required once they're on the device.
- **Triage with a highlighter.** Highlight the `ARCHIVE` label at the top of an
  article and it gets archived in Readwise on the next sync. Same for `INBOX`,
  `LATER`, and `DELETE`.
- **Highlights go home.** Anything you highlight in the body is pushed back to
  Readwise as a highlight on that document.
- **It's idempotent.** Every sync replaces the on-device document with a fresh,
  un-annotated copy, so each run only ever sees *new* marks.

## What it produces

Two PDFs — `Library.pdf` and `Feed.pdf` — each a three-tier hyperlinked document:

1. **Index** — a typographic table of contents, one row per item (title, author,
   reading time), newest first. Tap a row to jump to that article.
2. **Articles** — the full, de-cluttered reader text of every item, each starting
   on a fresh page, with a tappable nav bar (Home / Prev / Next) and the
   highlightable action band repeated on every page so you can triage from
   anywhere.
3. **Native bookmarks** — the PDF outline populates the reMarkable's navigation
   panel, giving a device-wide table of contents from any page.

Typography is tuned for e-ink reading: the **Newsreader** optical serif for body
and display, **Hanken Grotesk** for navigation and metadata, generous measure and
line-height, and editorial touches (uppercase kickers, hairline rules, ink-red
links). Content images are kept in color for the Paper Pro's color e-ink display.

## How it works

```
Readwise Reader API  ──fetch──▶  clean HTML  ──fulgur (HTML→PDF)──▶  Library.pdf / Feed.pdf
                                                                          │
                                                              cloud upload │ (reMarkable cloud)
                                                                          ▼
                                                                   reMarkable Paper Pro
                                                                  (read · highlight · triage)
                                                                          │
                                                            cloud fetch │ next sync
                                                                          ▼
        Readwise  ◀──archive / later / delete / highlights──  read back highlighter strokes
```

- **HTML→PDF with no browser.** Rendering uses [fulgur](https://crates.io/crates/fulgur)
  (Blitz + krilla), so there's no headless Chromium — just a fast, deterministic
  Rust pipeline with embedded fonts.
- **The PDF is the single source of truth.** rmreader embeds a manifest (page →
  Readwise document map, plus the action-label positions) inside the generated PDF.
  reMarkable returns the source PDF unchanged on download, so the bundle is
  self-describing — there's no local state to keep in sync.
- **Read-back is geometric.** The Paper Pro stores highlights as highlighter *ink
  strokes*, not selected text. rmreader maps those strokes from device space into
  PDF points, checks which land on an action label, and reconstructs highlighted
  body text from the PDF's own text layer (`pdftotext -bbox`). reMarkable file
  parsing lives in the sibling [`rmfiles`](../rmfiles) crate.

## Install

Build the `rmapps` binary from the workspace root — one build gives you every
subcommand, `reader` included:

```sh
cargo build --release        # binary at ./target/release/rmapps
```

On Nix, a dev shell for this crate is available for working on the engine itself
(`nix develop ./crates/rmreader`); a plain `cargo build` from the repo root builds
the whole workspace. The dev shell also brings `poppler-utils` for the PDF text
layer (`pdftotext`). reMarkable cloud sync is native — a pure-Rust client built
into the `rmapps` binary, with no external `rmapi` tool to install.

## Usage

Pair the machine once (native — no rmapi), then add a `[reader]` section to
`~/.config/rmapps/config.toml` (see below) and sync:

```sh
rmapps auth login            # paste the 8-char code from my.remarkable.com
rmapps reader                # read back, regenerate, and re-upload
```

`rmapps reader` reads back your on-device highlights and triage, applies them to
Readwise, regenerates fresh PDFs from the post-action state, and re-uploads them.

Run it on a schedule (cron, a systemd timer, or via `rmapps sync`) and your
reMarkable stays in step with your Readwise queue.

### Configuration

Add a `[reader]` section to `~/.config/rmapps/config.toml`. Because it holds your
Readwise token, keep that file private (mode 0600).

```toml
[reader]
device = "paper-pro-move"            # or "paper-pro"
theme = "reader"

  [reader.readwise]
  token = "..."                      # from readwise.io/access_token

  [reader.library]
  locations = ["new", "later", "shortlist"]
  max_items = 100

  [reader.feed]
  enabled = true
  max_items = 100

  [reader.images]
  enabled = true                     # fetch + embed content images (color)

  [reader.deploy]
  backend = "rmapi"                  # any value uploads; "none" writes PDFs locally only
  library_folder = "/Readwise"       # reMarkable cloud folder for Library.pdf
  feed_folder = "/Readwise"          # reMarkable cloud folder for Feed.pdf
```

Set `backend = "none"` to generate the PDFs on disk without touching the
reMarkable cloud — handy for trying it out. (The `"rmapi"` value is a legacy name
kept for compatibility; transport is always the native cloud client now.)

## Triage from the device

Every article page carries a band of real text labels just below the nav bar:

```
INBOX     ARCHIVE     LATER     DELETE
```

Highlight one with the reMarkable highlighter and, on the next sync, rmreader:

- moves the document to that location in Readwise (`INBOX` → new, `ARCHIVE`,
  `LATER`, or deletes it for `DELETE`);
- pushes any text you highlighted in the article body to Readwise as a highlight
  on that document.

Highlight two different action labels on the same article and rmreader skips the
action and warns rather than guessing — your body highlights still go through.

## Supported devices

| Device                    | Resolution (portrait) | PPI |
|---------------------------|-----------------------|-----|
| reMarkable Paper Pro Move | 954 × 1696            | 264 |
| reMarkable Paper Pro      | 1620 × 2160           | 229 |

Both are color e-ink, so images are rendered in color.

## Development

```sh
make test           # cargo test in the nix dev shell
make clippy         # cargo clippy -D warnings
make fmt-check      # rustfmt check
make build          # nix build
make hooks          # enable the pre-commit fmt hook (once per clone)
```

The codebase has no manual test steps — the Readwise client, content pipeline, PDF
assembly, manifest round-trip, read-back classification, and deploy command
sequences are all unit-tested (the Readwise and cloud boundaries use injectable
transports so they're tested against fakes). Layout is guarded by golden-image
visual-regression tests; regenerate them with `make update-goldens` after an
intentional design change.

## Project layout

```
src/
  cli.rs            init wizard | regenerate-and-sync from a config
  config.rs         TOML config + validate()
  readwise/         Reader API client (list, location change, delete, highlights)
  content.rs        sanitize HTML, fetch/transcode/rewrite images
  assemble.rs       build the three-tier HTML document
  render.rs         fulgur render: reader CSS, embedded fonts, bookmarks
  postprocess.rs    stamp nav bar + action band, embed the manifest
  embed.rs          write/read the PDF-embedded manifest
  readback/         strokes → coords → text layer → classify → Readwise plan
  deploy/           bundle-fetch seam for read-back (native cloud impl lives in rmapps)
  generate.rs       orchestration
themes/             reader.toml — the "Newsprint" palette
assets/fonts/       embedded TTFs (Newsreader, Hanken Grotesk, …)
docs/superpowers/   design specs, plans, and spike notes
```

reMarkable file-format parsing (`.rmdoc` bundles, v6 `.rm` scene blocks) lives in
the standalone, reusable [`rmfiles`](../rmfiles) crate.

## License

MIT — see [LICENSE](LICENSE).
