# rmapps

Monorepo for my reMarkable / pen-device tooling. Each subdirectory is an
independent Rust project with its own `flake.nix` and `Cargo.toml` — there is
no top-level workspace, so build each one inside its own Nix dev shell
(`nix develop --command cargo build --release`).

## Apps

| Path         | What it does                                                                 |
| ------------ | --------------------------------------------------------------------------- |
| `rmfiles/`   | Pure-Rust reader for reMarkable `.rm` (v6) scene files; extracts ink strokes. Shared library used by `rmreader` and `rmdigest`. |
| `rmbujo/`    | Dot-grid bullet-journal PDF generator for reMarkable.                        |
| `rmreader/`  | Syncs Readwise Reader articles to reMarkable as reader PDFs.                 |
| `rmdigest/`  | Builds annotation digests from highlighted/annotated reMarkable PDFs.        |
| `poc/inkit/` | Proof-of-concept framework for interactive apps on pen-based document devices (formerly `inkapp`). Device-agnostic by design; reMarkable first. |

## Layout notes

- `rmreader` and `rmdigest` depend on `rmfiles` via a path dependency
  (`rmfiles = { path = "../rmfiles" }`), so they must stay siblings.
- `poc/` holds experimental work not yet promoted to a first-class app.

## History

Each project was migrated here from its own repository with full git history
preserved (relocated under its subdirectory via `git filter-repo`). The former
standalone repos — `rmfiles`, `rmbujo`, `rmreader`, `rmdigest`, `inkapp` — are
archived and read-only.
