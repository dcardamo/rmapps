# rmbujo

Dot-grid bullet-journal PDF generator for reMarkable devices (Paper Pro Move / Paper Pro),
written in Rust. Renders askama HTML/CSS via fulgur (Blitz + krilla) — no headless browser.

## Setup

Dependencies are managed with Nix. With direnv:

    direnv allow        # loads the flake dev shell automatically

Or manually: `nix develop`.

## Usage

Create a new year (interactive wizard — creates `<base>/<year>/` and its `rmbujo.toml`,
then generates the PDFs):

    rmbujo new

Regenerate an existing year from its config:

    rmbujo path/to/2026/rmbujo.toml

Re-fetch ICS feeds and regenerate (otherwise the cached snapshot is reused):

    rmbujo path/to/2026/rmbujo.toml --refresh-feeds

## Output

A flat folder per year, one PDF per notebook: `2026 Future Log.pdf`,
`2026.01 January.pdf` … `2026.12 December.pdf`, `2026 Collection Template.pdf`,
`2026 Reference.pdf`.

## Development

    make test             # full suite in the Nix shell
    make update-goldens   # regenerate visual-regression golden images
    make clippy           # lints
    make build            # nix build the rmbujo package

## ICS calendar feeds

Add calendar feeds to `rmbujo.toml` and they will be overlaid on monthly spreads:

```toml
timezone = "America/Toronto"   # IANA timezone — used for all event rendering

[[ics]]
name    = "Holidays"
url     = "https://example.com/holidays.ics"
color   = "brick"              # any theme color name

[[ics]]
name    = "Work"
url     = "https://example.com/work.ics"
color   = "navy"
```

Fetched feeds are cached under `<year>/.ics-cache/`. On a plain `rmbujo
path/to/rmbujo.toml` run the cache is reused — regeneration is fast, reproducible,
and works offline. Pass `--refresh-feeds` to force a re-fetch:

    rmbujo path/to/2026/rmbujo.toml --refresh-feeds

## reMarkable cloud sync (rmapi)

Set `deploy.backend = "rmapi"` and `deploy.base_folder = "/rmbujo"` in `rmbujo.toml`
(the `new` wizard prompts for both). Pair once: run `rmapi` and paste a code from
<https://my.remarkable.com/device/desktop/connect>. Then:

- `rmbujo new` uploads the year's PDFs to `<base_folder>/<year>` (e.g. `/rmbujo/2026`).
- `rmbujo path/to/rmbujo.toml` regenerates and re-syncs with `rmapi put --content-only`,
  which replaces each PDF's background **without touching your handwriting**.

**Device sync rule:** always sync the device *before* running rmbujo, then sync again
after. This ensures any handwriting you added on the device reaches the cloud before
the content-only push, so nothing is lost.

## Adding pages on the device

To insert an extra page directly on the reMarkable, tap **+** and choose the built-in
**Dots Small** template — its dot grid matches rmbujo's spacing exactly. No sideloaded
template is needed.
