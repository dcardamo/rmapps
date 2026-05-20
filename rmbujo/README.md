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

ICS calendar feeds (incl. holidays) and reMarkable cloud sync (via rmapi) are Phase 2;
see `docs/superpowers/specs/2026-05-20-rmbujo-design.md`.
