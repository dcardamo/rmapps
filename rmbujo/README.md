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

## Output

A flat folder per year, one PDF per notebook: `2026 Future Log.pdf`,
`2026.01 January.pdf` … `2026.12 December.pdf`, `2026 Collection Template.pdf`,
`2026 Reference.pdf`.

## Development

    make test             # full suite in the Nix shell
    make update-goldens   # regenerate visual-regression golden images
    make clippy           # lints
    make build            # nix build the rmbujo package

## reMarkable cloud sync (rmapi)

Set `deploy.backend = "rmapi"` and `deploy.base_folder = "/rmbujo"` in `rmbujo.toml`
(the `new` wizard prompts for both). Pair once: run `rmapi` and paste a code from
<https://my.remarkable.com/device/desktop/connect>. Then:

- `rmbujo new` uploads the year's PDFs to `<base_folder>/<year>` (e.g. `/rmbujo/2026`).
- `rmbujo path/to/rmbujo.toml` regenerates and re-syncs with `rmapi put --content-only`,
  which replaces each PDF's background **without touching your handwriting**.

ICS calendar feeds (incl. holidays) are the next phase; see
`docs/superpowers/specs/2026-05-20-rmbujo-design.md`.
