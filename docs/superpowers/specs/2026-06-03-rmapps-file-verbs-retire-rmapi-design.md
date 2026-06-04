# rmapps file CLI verbs + retire rmapi from kobo — design

Date: 2026-06-03

## Goal

Make `rmapps` a complete replacement for `rmapi` file operations
(put / list / get / delete) over the native `rm-cloud` client, then rewrite the
dotfiles `kobo_getbooks` book-push pipeline to use `rmapps` and remove `rmapi`
from `~/git/dotfiles` entirely.

## Background

`bin/kobo_getbooks` (in `~/git/dotfiles`) is the only remaining `rmapi`
consumer. It uses exactly two operations:

- `rmapi -ni mkdir <path>` — nested folder creation; "already exists" is treated
  as soft-success.
- `rmapi -ni put <file> <dir>` — upload **EPUB and PDF**, with the hard
  invariant **never overwrite** an existing cloud document ("entry already
  exists" is soft-success, so cloud-side annotations are never destroyed). The
  PDF is staged with a `<Title> (PDF)` visible name to avoid colliding with the
  EPUB in the same folder.

`rmapps` today already provides:

- `ls` (list) — read-only, prints `d`/`f` per entry.
- `rm` (delete) — refuses the cloud root, requires `--recursive` for non-empty
  folders, removes deepest-first so nothing is orphaned. No "delete all" verb.
- `push` (put) — but **PDF-only**, and offers only `replace` (destructive
  create-or-replace) and `--content-only` (ink-preserving refresh). There is no
  create-if-missing mode.
- `Cloud::ensure_folder` / `mkdir_p` — covers the nested-mkdir need; `push`
  already creates its destination folder.

Gaps that block retiring `rmapi`:

1. **EPUB upload** — `rm-cloud` only has `DocFiles::new_pdf`; `push` always
   builds a PDF document.
2. **create-if-missing push mode** — the kobo "never overwrite annotations"
   invariant. `Cloud::create_if_missing_in` exists internally but is not exposed
   on the CLI, and is PDF-only.
3. **`get` (download)** — no equivalent exists.

On saturn, the `rmapps` binary is **not** on the system PATH: it is built from
the monorepo to `~/git/rmapps/target/release/rmapps` (see
`nixos/saturn/remarkable.nix`). The dotfiles swap must invoke that path.
"Paired" is determined by the presence of `~/.config/rmapps/auth.json` (the
device-token file written by `rmapps auth`), replacing the old
`~/.config/rmapi/rmapi.conf` token check.

## Components

### 1. `rm-cloud`: EPUB document constructor

Add `DocFiles::new_epub(visible_name, parent, epub: Vec<u8>)` to
`crates/rm-cloud/src/porcelain/docfiles.rs`, a sibling of `new_pdf`:

- Metadata identical to the PDF path (`DocumentType`, given `visible_name` and
  `parent`, `lastModified` = now).
- Content is **minimal**: `{"fileType":"epub","formatVersion":1,
  "sizeInBytes":"<len>"}`. Unlike `new_pdf` we deliberately do **not** synthesize
  a `pages`/`redirectionPageMap` list — the device paginates EPUB at render time,
  and a fabricated page map would be incorrect.
- Source blob stored as `<id>.epub`.
- Factor the shared metadata construction so `new_pdf` and `new_epub` do not
  drift.

### 2. `cloud.rs`: thread a document kind through the upload helpers

Introduce `pub enum DocKind { Pdf, Epub }` and a private
`build_doc(kind, name, folder_id, bytes) -> DocFiles` that dispatches to
`new_pdf` / `new_epub`. The create/replace helpers take a `DocKind`:

- `replace_in`, `create_if_missing_in`, and their path-resolving wrappers gain a
  `DocKind` parameter.
- `upsert` / `upsert_in` / `put_content_only` (the ink-preserving swap) stay
  **PDF-only**; content-only on an EPUB returns a clear error.
- All existing PDF callers (bujo, reader, digest, sync, the existing `push`
  replace path) pass `DocKind::Pdf` — a mechanical change.

### 3. `push` gains create-if-missing + EPUB

- New mode flag `--if-missing`: create the doc only when absent; if a same-named
  doc already exists, **no-op success** — never overwrites, preserving cloud
  annotations. This is the kobo invariant and maps `rmapi`'s "entry already
  exists" soft-success.
- The document kind is inferred from the **local file extension** (`.epub` →
  `Epub`, else `Pdf`).
- Three mutually-exclusive modes: default `replace` (destructive),
  `--content-only` (PDF-only, ink-preserving), `--if-missing` (safe create).
  Clap enforces mutual exclusion; `--content-only` combined with a `.epub` input
  errors.
- `--name` continues to override the visible name (file stem otherwise). The
  existing `doc_name` already strips the final extension generically, so it
  needs no change.

### 4. New `get` command

`rmapps get PATH [DEST]` (new module `apps/rmapps/src/get.rs`, wired in
`main.rs`). It is read-only with respect to the cloud (it only writes to the
local filesystem), so — like `ls` — it takes **no** cloud-mutation lock:

- Resolve PATH read-only into parent folder + leaf name; find the document by
  leaf name among the parent's children (same pattern as `rm`).
- `client.get(id)` → `DocFiles`; extract the **original source blob**, preferring
  `<id>.pdf`, else `<id>.epub`.
- Write to DEST (default `./<name>.<ext>` where `<ext>` matches the found blob).
- A document with no source blob (e.g. a pure-ink notebook) errors with a
  message naming the limitation.
- **Safety:** refuse to overwrite an existing DEST unless `--force`.
- A missing PATH is reported (not an error), mirroring `rm`.

### 5. Delete safety (audit only)

`rm` is already safe — refuses the root, requires `--recursive` for non-empty
folders, deletes deepest-first. No "delete all" / wildcard verb exists and none
is added. No code change; recorded here for the safety requirement.

### 6. dotfiles swap (retire rmapi)

In `~/git/dotfiles`:

- `bin/kobo_getbooks`:
  - Replace `rmapi_mkdir` / `rmapi_put` (and the entire rmapi token-clobber
    snapshot/guard machinery: `RMAPI_CONF`, `_rmapi_snapshot_conf`,
    `_rmapi_guard`, `_rmapi_paired`) with `rmapps push --if-missing`.
  - The mkdir chain collapses — `push` creates the destination folder.
  - EPUB: `rmapps push --if-missing <epub> /Books/Purchased/kobo/<Author>`.
  - PDF: `rmapps push --if-missing --name "<Title> (PDF)" <pdf>
    /Books/Purchased/kobo/<Author>`.
  - "Paired" check becomes: `~/.config/rmapps/auth.json` exists.
  - Binary resolved via `RMAPPS_BIN` env var, defaulting to
    `~/git/rmapps/target/release/rmapps`, falling back to `rmapps` on PATH.
  - `--if-missing` exits 0 whether it created or found an existing doc, so the
    existing best-effort push/retry semantics (book stays safe locally + on
    Filen on failure) are preserved.
- `nixos/saturn/kobo-books.nix`: drop `rmapi` from `systemPackages`; update the
  comment block.
- `overlays/rmapi.nix` and its reference in `flake.nix`: removed.
- `secrets/manifest.nix`: remove rmapi entries if unused after the swap.
- `tests/test_kobo_getbooks.py`: update mocks from the `rmapi` invocation to the
  `rmapps push` invocation.

## Testing

- `rm-cloud`: `new_epub` writes an `<id>.epub` blob and `fileType:epub` content
  (mirrors the existing `new_pdf` tests).
- `cloud.rs`: `create_if_missing` is a no-op when the doc exists (annotation
  bytes preserved) and creates when absent, for both `Pdf` and `Epub`; EPUB
  content-only errors. Against `FakeCloud`.
- `get`: round-trip a pushed EPUB and PDF and assert byte-identical source
  extraction; refuses to overwrite without `--force`; a pure-ink doc errors.
  Against `FakeCloud`.
- dotfiles: updated `tests/test_kobo_getbooks.py` passes under pytest.

## Out of scope

- EPUB content-only / ink-preserving updates.
- Bulk / recursive upload.
- Wildcard / glob path expansion.
- Any "delete all" verb.
