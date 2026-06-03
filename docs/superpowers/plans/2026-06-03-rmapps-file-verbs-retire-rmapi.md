# rmapps file CLI verbs + retire rmapi — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `rmapps` a complete rmapi replacement for file ops (put/list/get/delete) by adding EPUB upload, a safe create-if-missing push mode, and a `get` download verb — then rewrite the dotfiles `kobo_getbooks` pipeline onto `rmapps` and remove rmapi from `~/git/dotfiles`.

**Architecture:** EPUB support lands in `rm-cloud` (`DocFiles::new_epub`), is threaded through `cloud.rs` via a `DocKind` enum on new kind-aware create/replace helpers (existing PDF callers untouched via thin wrappers), exposed on `push` (`--if-missing`, EPUB by extension), and complemented by a read-only `get` command. The dotfiles swap replaces the rmapi `mkdir`+`put` machinery with `rmapps push --if-missing`.

**Tech Stack:** Rust (clap, tokio, anyhow, serde_json, zip, uuid, lopdf), `rm-cloud` native client with `FakeCloud` test seam; Python 3 + pytest for the dotfiles `kobo_getbooks` script and Nix for saturn packaging.

**User Verification:** NO — no user verification required (the spec asks for automated tests; no human-in-the-loop sign-off was requested).

**Cross-repo note:** Tasks 1–4 are in this repo (`~/.paseo/worktrees/36mv6dc6/mad-elk`, the rmapps monorepo). Tasks 5–6 are in `~/git/dotfiles`.

---

### Task 1: `rm-cloud` EPUB document constructor

**Goal:** Add `DocFiles::new_epub` (and factor shared metadata) so the native client can build EPUB documents.

**Files:**
- Modify: `crates/rm-cloud/src/porcelain/docfiles.rs`

**Acceptance Criteria:**
- [ ] `DocFiles::new_epub(name, parent, bytes)` produces a `DocumentType` doc with a `<id>.epub` blob and `{"fileType":"epub","formatVersion":1,"sizeInBytes":"<len>"}` content.
- [ ] `new_pdf` and `new_epub` share one metadata builder.
- [ ] Existing `new_pdf` tests still pass.

**Verify:** `cargo test -p rm-cloud docfiles` → all pass.

**Steps:**

- [ ] **Step 1: Write the failing tests** — add to the `tests` module in `crates/rm-cloud/src/porcelain/docfiles.rs`:

```rust
    #[test]
    fn new_epub_writes_epub_blob_and_content() {
        let docs = DocFiles::new_epub("My Book", "parent-id", b"epub-bytes".to_vec());
        // The source blob is stored as <id>.epub with the exact bytes.
        let blob = docs.get(&format!("{}.epub", docs.id)).expect("has .epub");
        assert_eq!(blob, b"epub-bytes");
        // Content declares the epub fileType (no fabricated page list).
        let content_raw = docs.get(&format!("{}.content", docs.id)).expect("has .content");
        let content: serde_json::Value = serde_json::from_slice(content_raw).unwrap();
        assert_eq!(content["fileType"], "epub");
        assert_eq!(content["formatVersion"].as_u64(), Some(1));
        assert_eq!(content["sizeInBytes"], "epub-bytes".len().to_string());
        assert!(content.get("pages").is_none(), "epub must not synthesize a page list");
        // Metadata names the doc and sets DocumentType under the given parent.
        let meta = docs.metadata().unwrap();
        assert_eq!(meta.visible_name, "My Book");
        assert_eq!(meta.doc_type, "DocumentType");
        assert_eq!(meta.parent, "parent-id");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p rm-cloud new_epub_writes_epub_blob_and_content`
Expected: FAIL — `no function or associated item named 'new_epub'`.

- [ ] **Step 3: Factor the shared metadata builder** — in `impl DocFiles`, add above `new_pdf`:

```rust
    /// Shared `DocumentType` metadata for a freshly-created document.
    fn base_metadata(visible_name: &str, parent: &str) -> Metadata {
        Metadata {
            visible_name: visible_name.to_string(),
            doc_type: "DocumentType".to_string(),
            parent: parent.to_string(),
            last_modified: super::document::now_millis(),
            deleted: false,
            extra: Default::default(),
        }
    }
```

Then replace the inline `let meta = Metadata { ... };` block in `new_pdf` with:

```rust
        let meta = Self::base_metadata(visible_name, parent);
```

- [ ] **Step 4: Add `new_epub`** — directly after `new_pdf`:

```rust
    /// Build a brand-new EPUB document file-set: a fresh UUID, a `DocumentType`
    /// `.metadata`, a minimal `.content` declaring `fileType:"epub"`, and the
    /// `.epub` blob. Unlike [`new_pdf`](Self::new_pdf) we do NOT synthesize a
    /// `pages`/`redirectionPageMap` list — the device paginates EPUB at render
    /// time, so a fabricated page map would be wrong.
    pub fn new_epub(visible_name: &str, parent: &str, epub: Vec<u8>) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let meta = Self::base_metadata(visible_name, parent);
        let content = serde_json::to_vec(&serde_json::json!({
            "fileType": "epub",
            "formatVersion": 1,
            "sizeInBytes": epub.len().to_string(),
        }))
        .expect("serialize content");
        let files = vec![
            (
                format!("{id}.metadata"),
                serde_json::to_vec(&meta).expect("serialize metadata"),
            ),
            (format!("{id}.content"), content),
            (format!("{id}.epub"), epub),
        ];
        Self { id, files }
    }
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test -p rm-cloud docfiles`
Expected: PASS (new test + existing `new_pdf_*` tests).

- [ ] **Step 6: Commit**

```bash
git add crates/rm-cloud/src/porcelain/docfiles.rs
git commit -m "feat(rm-cloud): add DocFiles::new_epub for native EPUB upload"
```

```json:metadata
{"files": ["crates/rm-cloud/src/porcelain/docfiles.rs"], "verifyCommand": "cargo test -p rm-cloud docfiles", "acceptanceCriteria": ["new_epub builds .epub blob + epub content", "shared metadata builder", "new_pdf tests still pass"], "requiresUserVerification": false}
```

---

### Task 2: `DocKind` + kind-aware create/replace in `cloud.rs`

**Goal:** Thread a document kind through new create/replace helpers so callers can upload EPUB or PDF, with existing PDF callers unchanged.

**Files:**
- Modify: `apps/rmapps/src/cloud.rs`

**Acceptance Criteria:**
- [ ] `pub enum DocKind { Pdf, Epub }` exists.
- [ ] `replace_doc` / `replace_in_kind` and `create_if_missing` / `create_if_missing_in_kind` work for both kinds; `create_if_missing*` returns `true` on create, `false` when the doc already exists (untouched).
- [ ] Existing `replace`, `replace_in`, `create_if_missing_in`, `upsert*` signatures unchanged (delegate to the new kind-aware methods with `DocKind::Pdf`).

**Verify:** `cargo test -p rmapps cloud::` → all pass.

**Steps:**

- [ ] **Step 1: Write the failing tests** — add to the `tests` module in `apps/rmapps/src/cloud.rs`:

```rust
    /// create_if_missing creates when absent (returns true) and is a no-op that
    /// preserves the existing bytes when present (returns false). Covers EPUB.
    #[test]
    fn create_if_missing_is_safe_noop_when_present() {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let fake = rt.block_on(FakeCloud::spawn());
        let client = Client::from_user_token(CloudConfig::single_host(&fake.base), "user-token");
        let cloud = cloud_from_client(client);

        // First push of an EPUB creates it.
        let created = cloud
            .create_if_missing("/Books", "Title", b"epub-v1".to_vec(), DocKind::Epub)
            .unwrap();
        assert!(created, "first create_if_missing should create the doc");

        let folder_id = cloud.resolve_folder("/Books").unwrap().unwrap();
        let id = cloud.doc_id_in(&folder_id, "Title").unwrap().unwrap();

        // Second push must NOT create or overwrite — returns false, same doc id.
        let created2 = cloud
            .create_if_missing("/Books", "Title", b"epub-v2".to_vec(), DocKind::Epub)
            .unwrap();
        assert!(!created2, "second create_if_missing must be a no-op");
        let id2 = cloud.doc_id_in(&folder_id, "Title").unwrap().unwrap();
        assert_eq!(id, id2, "the existing doc must be left untouched (no overwrite)");
    }

    /// replace_doc builds the right blob kind: an EPUB replace stores a .epub blob.
    #[test]
    fn replace_doc_epub_stores_epub_blob() {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let fake = rt.block_on(FakeCloud::spawn());
        let client = Client::from_user_token(CloudConfig::single_host(&fake.base), "user-token");
        let cloud = cloud_from_client(client);

        cloud.replace_doc("/Books", "Title", b"epub-bytes".to_vec(), DocKind::Epub).unwrap();
        let folder_id = cloud.resolve_folder("/Books").unwrap().unwrap();
        let id = cloud.doc_id_in(&folder_id, "Title").unwrap().unwrap();
        let df = cloud.block_on(cloud.client().get(&id)).unwrap();
        assert!(
            df.files.iter().any(|(n, b)| n.ends_with(".epub") && b == b"epub-bytes"),
            "replace_doc(Epub) must store the bytes as a .epub blob"
        );
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p rmapps create_if_missing_is_safe_noop_when_present`
Expected: FAIL — `cannot find type 'DocKind'` / `no method named 'create_if_missing'`.

- [ ] **Step 3: Add `DocKind` and import `new_epub`** — at the top of `apps/rmapps/src/cloud.rs`, the `use rm_cloud::{...}` line already pulls `DocFiles` (which exposes both `new_pdf`/`new_epub`); no import change needed. Add the enum near `RemoteDoc`:

```rust
/// The source format of a document being uploaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocKind {
    Pdf,
    Epub,
}
```

- [ ] **Step 4: Add the kind-aware methods** — inside `impl Cloud`, add:

```rust
    /// Build a fresh document file-set of the given kind.
    fn build_doc(&self, kind: DocKind, name: &str, folder_id: &str, bytes: Vec<u8>) -> DocFiles {
        match kind {
            DocKind::Pdf => DocFiles::new_pdf(name, folder_id, bytes),
            DocKind::Epub => DocFiles::new_epub(name, folder_id, bytes),
        }
    }

    /// Path-resolving destructive replace of any document kind: create `folder`
    /// if missing, sweep EVERY same-named doc, then create a fresh one.
    pub fn replace_doc(&self, folder: &str, name: &str, bytes: Vec<u8>, kind: DocKind) -> Result<()> {
        let folder_id = self.ensure_folder(folder)?;
        self.replace_in_kind(&folder_id, name, bytes, kind)
    }

    /// `replace_doc` against an already-resolved folder id.
    pub fn replace_in_kind(&self, folder_id: &str, name: &str, bytes: Vec<u8>, kind: DocKind) -> Result<()> {
        for id in self.doc_ids_in(folder_id, name)? {
            // Best-effort remove; individual failures surface on the create below.
            let _ = self.rt.block_on(self.client.rm(&id));
        }
        let doc = self.build_doc(kind, name, folder_id, bytes);
        self.rt
            .block_on(self.client.put(doc))
            .map_err(|e| anyhow!("replace {name}: {e}"))
    }

    /// Path-resolving create-if-missing of any document kind. Creates `folder` if
    /// missing. Returns `true` if a new doc was created, `false` if a same-named
    /// doc already existed (left completely untouched — preserves any cloud or
    /// on-device annotations). Never overwrites.
    pub fn create_if_missing(&self, folder: &str, name: &str, bytes: Vec<u8>, kind: DocKind) -> Result<bool> {
        let folder_id = self.ensure_folder(folder)?;
        self.create_if_missing_in_kind(&folder_id, name, bytes, kind)
    }

    /// `create_if_missing` against an already-resolved folder id.
    pub fn create_if_missing_in_kind(&self, folder_id: &str, name: &str, bytes: Vec<u8>, kind: DocKind) -> Result<bool> {
        if self.doc_id_in(folder_id, name)?.is_some() {
            return Ok(false);
        }
        let doc = self.build_doc(kind, name, folder_id, bytes);
        self.rt
            .block_on(self.client.put(doc))
            .map_err(|e| anyhow!("create {name}: {e}"))?;
        Ok(true)
    }
```

- [ ] **Step 5: Make the existing PDF methods delegate** — replace the bodies of `create_if_missing_in` and `replace_in` so the logic lives in one place:

Replace:

```rust
    pub fn create_if_missing_in(&self, folder_id: &str, name: &str, pdf: Vec<u8>) -> Result<()> {
        if self.doc_id_in(folder_id, name)?.is_some() {
            return Ok(());
        }
        self.rt
            .block_on(self.client.put(DocFiles::new_pdf(name, folder_id, pdf)))
            .map_err(|e| anyhow!("create {name}: {e}"))
    }
```

with:

```rust
    pub fn create_if_missing_in(&self, folder_id: &str, name: &str, pdf: Vec<u8>) -> Result<()> {
        self.create_if_missing_in_kind(folder_id, name, pdf, DocKind::Pdf).map(|_| ())
    }
```

And replace:

```rust
    pub fn replace_in(&self, folder_id: &str, name: &str, pdf: Vec<u8>) -> Result<()> {
        for id in self.doc_ids_in(folder_id, name)? {
            // Best-effort remove; individual failures surface on the create below.
            let _ = self.rt.block_on(self.client.rm(&id));
        }
        self.rt
            .block_on(self.client.put(DocFiles::new_pdf(name, folder_id, pdf)))
            .map_err(|e| anyhow!("replace {name}: {e}"))
    }
```

with:

```rust
    pub fn replace_in(&self, folder_id: &str, name: &str, pdf: Vec<u8>) -> Result<()> {
        self.replace_in_kind(folder_id, name, pdf, DocKind::Pdf)
    }
```

(Leave `upsert`, `upsert_in`, `replace`, `create_if_missing_in`'s callers, and `put_content_only` as-is — `upsert_in` keeps its own content-only body.)

- [ ] **Step 6: Run to verify pass**

Run: `cargo test -p rmapps cloud::`
Expected: PASS — new tests plus all existing cloud tests (`replace_*`, `resolver_*`).

- [ ] **Step 7: Commit**

```bash
git add apps/rmapps/src/cloud.rs
git commit -m "feat(rmapps): DocKind-aware create_if_missing/replace_doc helpers"
```

```json:metadata
{"files": ["apps/rmapps/src/cloud.rs"], "verifyCommand": "cargo test -p rmapps cloud::", "acceptanceCriteria": ["DocKind enum", "create_if_missing returns created/skipped and never overwrites", "replace_doc stores correct blob kind", "existing PDF methods delegate unchanged"], "requiresUserVerification": false}
```

---

### Task 3: `push` gains `--if-missing` and EPUB support

**Goal:** Expose create-if-missing and EPUB upload on the CLI, with `--content-only` restricted to PDF.

**Files:**
- Modify: `apps/rmapps/src/push.rs`

**Acceptance Criteria:**
- [ ] `--if-missing` creates only when absent and prints whether it created or skipped; never overwrites.
- [ ] A `.epub` argument uploads as an EPUB; other extensions as PDF.
- [ ] `--content-only` and `--if-missing` are mutually exclusive; `--content-only` with a `.epub` errors clearly.

**Verify:** `cargo build -p rmapps && cargo test -p rmapps push` → builds, tests pass.

**Steps:**

- [ ] **Step 1: Write the failing tests** — add a `tests` module at the end of `apps/rmapps/src/push.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_from_extension() {
        assert_eq!(doc_kind(std::path::Path::new("Book.epub")), DocKind::Epub);
        assert_eq!(doc_kind(std::path::Path::new("Book.EPUB")), DocKind::Epub);
        assert_eq!(doc_kind(std::path::Path::new("Book.pdf")), DocKind::Pdf);
        assert_eq!(doc_kind(std::path::Path::new("Book")), DocKind::Pdf);
    }

    #[test]
    fn content_only_rejects_epub() {
        let err = validate_modes(true, false, DocKind::Epub).unwrap_err();
        assert!(err.to_string().contains("PDF-only"), "got: {err}");
    }

    #[test]
    fn content_only_ok_for_pdf_and_if_missing_ok_for_epub() {
        assert!(validate_modes(true, false, DocKind::Pdf).is_ok());
        assert!(validate_modes(false, true, DocKind::Epub).is_ok());
        assert!(validate_modes(false, false, DocKind::Epub).is_ok());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p rmapps push`
Expected: FAIL — `cannot find function 'doc_kind'` / `validate_modes`.

- [ ] **Step 3: Rewrite `push.rs`** — replace the whole file with:

```rust
//! `rmapps push` — generic one-shot upload of a PDF or EPUB to a cloud folder.
//!
//! Modes (mutually exclusive):
//! - default `replace`: destructive create-or-replace (no ink to preserve).
//! - `--content-only`: PDF-only content refresh that preserves on-device ink.
//! - `--if-missing`: create only when absent; an existing same-named doc is left
//!   untouched (never overwrites — preserves cloud/device annotations).
//!
//! The document kind is inferred from the local file extension (`.epub` → EPUB,
//! else PDF). `--content-only` with an `.epub` is rejected (EPUBs have no
//! ink-preserving refresh path).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Args;

use crate::cloud::{self, Cloud, DocKind};

#[derive(Args)]
pub struct PushArgs {
    /// Path to the file to upload (`.pdf` or `.epub`).
    file: PathBuf,
    /// Destination cloud folder (slash path, e.g. `/Books`); created if missing.
    folder: String,
    /// Visible name on the device. Defaults to the file stem.
    #[arg(long)]
    name: Option<String>,
    /// Content-only refresh (preserves on-device ink). PDF-only. Mutually
    /// exclusive with `--if-missing`.
    #[arg(long = "content-only", conflicts_with = "if_missing")]
    content_only: bool,
    /// Create only if absent; never overwrite an existing same-named doc.
    /// Mutually exclusive with `--content-only`.
    #[arg(long = "if-missing")]
    if_missing: bool,
}

/// Infer the document kind from a path's extension.
fn doc_kind(path: &Path) -> DocKind {
    match path.extension().and_then(|e| e.to_str()) {
        Some(e) if e.eq_ignore_ascii_case("epub") => DocKind::Epub,
        _ => DocKind::Pdf,
    }
}

/// Reject the one invalid mode/kind combination (content-only on an EPUB).
fn validate_modes(content_only: bool, _if_missing: bool, kind: DocKind) -> Result<()> {
    if content_only && kind == DocKind::Epub {
        bail!("--content-only is PDF-only; EPUBs have no ink-preserving refresh");
    }
    Ok(())
}

pub fn run(args: PushArgs) -> Result<()> {
    let kind = doc_kind(&args.file);
    validate_modes(args.content_only, args.if_missing, kind)?;

    let bytes = std::fs::read(&args.file)
        .with_context(|| format!("reading {}", args.file.display()))?;
    let name = match &args.name {
        Some(n) => n.clone(),
        None => cloud::doc_name(&args.file)?,
    };

    let cloud = Cloud::from_stored()?;
    let mode = if args.content_only {
        cloud.upsert(&args.folder, &name, bytes)?;
        "content-only".to_string()
    } else if args.if_missing {
        let created = cloud.create_if_missing(&args.folder, &name, bytes, kind)?;
        if created { "created".to_string() } else { "already exists (skipped)".to_string() }
    } else {
        cloud.replace_doc(&args.folder, &name, bytes, kind)?;
        "replace".to_string()
    };

    println!("pushed {name} -> {} ({mode})", args.folder);
    Ok(())
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p rmapps push && cargo build -p rmapps`
Expected: PASS, clean build.

- [ ] **Step 5: Commit**

```bash
git add apps/rmapps/src/push.rs
git commit -m "feat(rmapps): push --if-missing + EPUB support (content-only stays PDF-only)"
```

```json:metadata
{"files": ["apps/rmapps/src/push.rs"], "verifyCommand": "cargo test -p rmapps push", "acceptanceCriteria": ["--if-missing create-or-skip without overwrite", "EPUB by extension", "content-only PDF-only guard", "modes mutually exclusive"], "requiresUserVerification": false}
```

---

### Task 4: New `get` (download) command

**Goal:** Add `rmapps get PATH [DEST]` that writes the original source file (PDF/EPUB), refusing to overwrite without `--force`.

**Files:**
- Create: `apps/rmapps/src/get.rs`
- Modify: `apps/rmapps/src/main.rs`

**Acceptance Criteria:**
- [ ] `get` resolves a doc by path, extracts its `.pdf` or `.epub` source blob, and writes it (default `./<name>.<ext>`).
- [ ] Refuses to overwrite an existing DEST unless `--force`.
- [ ] A doc with no source blob errors; a missing PATH is reported (not an error).

**Verify:** `cargo test -p rmapps get` → pass; `cargo run -p rmapps -- get --help` shows the command.

**Steps:**

- [ ] **Step 1: Write the failing test** — create `apps/rmapps/src/get.rs` with the source-blob picker and its test (the command body is added in Step 3):

```rust
//! `rmapps get PATH [DEST]` — download a document's original source file.
//!
//! Resolves PATH read-only into parent folder + leaf name (like `rm`), finds the
//! document, downloads it, and writes its ORIGINAL source blob (`.pdf` or
//! `.epub`) to DEST. Refuses to overwrite an existing DEST unless `--force`. A
//! pure-ink notebook (no source blob) errors. A missing PATH is reported.

use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};
use clap::Args;
use rm_cloud::DocFiles;

use crate::cloud::Cloud;

#[derive(Args)]
pub struct GetArgs {
    /// Document path to download, e.g. `/Books/Purchased/kobo/Author/Title`.
    path: String,
    /// Destination file (default: `./<name>.<ext>` from the source blob kind).
    dest: Option<PathBuf>,
    /// Overwrite DEST if it already exists.
    #[arg(long)]
    force: bool,
}

/// Pick the original source blob from a downloaded doc: prefer `.pdf`, else
/// `.epub`. Returns `(extension, bytes)`.
fn pick_source(df: &DocFiles) -> Result<(&'static str, Vec<u8>)> {
    if let Some((_, b)) = df.files.iter().find(|(n, _)| n.ends_with(".pdf")) {
        return Ok(("pdf", b.clone()));
    }
    if let Some((_, b)) = df.files.iter().find(|(n, _)| n.ends_with(".epub")) {
        return Ok(("epub", b.clone()));
    }
    Err(anyhow!(
        "document has no PDF or EPUB source blob (a pure-ink notebook cannot be exported as a source file)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn df_with(files: &[(&str, &[u8])]) -> DocFiles {
        DocFiles {
            id: "id".into(),
            files: files.iter().map(|(n, b)| (n.to_string(), b.to_vec())).collect(),
        }
    }

    #[test]
    fn prefers_pdf_then_epub() {
        let pdf = df_with(&[("id.metadata", b"{}"), ("id.pdf", b"PDF"), ("id.epub", b"EPUB")]);
        assert_eq!(pick_source(&pdf).unwrap(), ("pdf", b"PDF".to_vec()));
        let epub = df_with(&[("id.metadata", b"{}"), ("id.epub", b"EPUB")]);
        assert_eq!(pick_source(&epub).unwrap(), ("epub", b"EPUB".to_vec()));
    }

    #[test]
    fn errors_on_pure_ink_doc() {
        let ink = df_with(&[("id.metadata", b"{}"), ("id.content", b"{}"), ("id/0.rm", b"ink")]);
        assert!(pick_source(&ink).is_err());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p rmapps get`
Expected: FAIL — `get.rs` is not yet a module (`cannot find module` once referenced) OR, before wiring, the file compiles standalone only under the crate. To make it fail meaningfully, do Step 3's `main.rs` wiring first if the test runner can't see the module; the tests then run and pass after Step 3. (If `cargo test -p rmapps get` reports "no tests" because the module isn't wired, that is the failing state.)

- [ ] **Step 3: Add the command body to `get.rs`** — append the `run` function (after `pick_source`, before the `tests` module):

```rust
pub fn run(args: GetArgs) -> Result<()> {
    let cloud = Cloud::from_stored()?;

    // Split PATH into parent folder + leaf name (same shape as `rm`).
    let trimmed = args.path.trim().trim_matches('/');
    if trimmed.is_empty() {
        bail!("refusing to get the cloud root");
    }
    let (parent_path, leaf) = match trimmed.rsplit_once('/') {
        Some((p, l)) => (p.to_string(), l.to_string()),
        None => (String::new(), trimmed.to_string()),
    };

    // Resolve the parent read-only; missing parent ⇒ the target can't exist.
    let parent_id = match cloud.resolve_folder(&parent_path)? {
        Some(id) => id,
        None => {
            println!("{}: not found", args.path);
            return Ok(());
        }
    };

    // Find the target document by leaf name among the parent's children.
    let entries = cloud.block_on(cloud.client().ls(&parent_id))?;
    let Some(target) = entries.into_iter().find(|e| e.name == leaf) else {
        println!("{}: not found", args.path);
        return Ok(());
    };
    if target.is_folder {
        bail!("{} is a folder, not a document", args.path);
    }

    // Download and extract the original source blob.
    let df = cloud.block_on(cloud.client().get(&target.id))?;
    let (ext, bytes) = pick_source(&df)?;

    let dest = args
        .dest
        .unwrap_or_else(|| PathBuf::from(format!("{leaf}.{ext}")));
    if dest.exists() && !args.force {
        bail!("{} already exists (use --force to overwrite)", dest.display());
    }
    std::fs::write(&dest, bytes)
        .map_err(|e| anyhow!("writing {}: {e}", dest.display()))?;
    println!("wrote {} -> {}", args.path, dest.display());
    Ok(())
}
```

- [ ] **Step 4: Wire into `main.rs`** — add `mod get;` with the other `mod` lines (alphabetical, after `mod digest;`); add the variant to `enum Command`:

```rust
    /// Download a document's original source file (PDF/EPUB) to disk.
    Get(get::GetArgs),
```

and the match arm in `main()` (read-only, no lock — like `Ls`):

```rust
        Command::Get(args) => get::run(args),
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test -p rmapps get && cargo run -p rmapps -- get --help`
Expected: tests PASS; help shows `get` with `PATH`, `[DEST]`, `--force`.

- [ ] **Step 6: Commit**

```bash
git add apps/rmapps/src/get.rs apps/rmapps/src/main.rs
git commit -m "feat(rmapps): add get command to download original PDF/EPUB source"
```

```json:metadata
{"files": ["apps/rmapps/src/get.rs", "apps/rmapps/src/main.rs"], "verifyCommand": "cargo test -p rmapps get", "acceptanceCriteria": ["extracts pdf/epub source", "refuses overwrite without --force", "pure-ink doc errors", "missing path reported"], "requiresUserVerification": false}
```

---

### Task 5: Final verification of the rmapps side

**Goal:** Confirm the whole workspace builds and tests green before touching dotfiles.

**Files:** none (verification only).

**Acceptance Criteria:**
- [ ] `cargo build --workspace --release` succeeds.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo clippy --workspace -- -D warnings` is clean (matches repo convention).

**Verify:** the three commands below all succeed.

**Steps:**

- [ ] **Step 1: Build, test, lint**

Run:
```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --release
```
Expected: all succeed. Fix any fallout in the touched files before proceeding.

- [ ] **Step 2: No commit unless fixes were needed** — if Step 1 required edits, commit them:

```bash
git add -A && git commit -m "chore(rmapps): clippy/build fixups for file verbs"
```

```json:metadata
{"files": [], "verifyCommand": "cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings", "acceptanceCriteria": ["workspace builds release", "all tests pass", "clippy clean"], "requiresUserVerification": false}
```

---

### Task 6: Rewrite dotfiles `kobo_getbooks` onto `rmapps`

**Goal:** Replace the rmapi `mkdir`+`put` machinery in `bin/kobo_getbooks` with `rmapps push --if-missing`, and update its tests.

**Files:**
- Modify: `~/git/dotfiles/bin/kobo_getbooks`
- Modify: `~/git/dotfiles/tests/test_kobo_getbooks.py`

**Acceptance Criteria:**
- [ ] `_push_book` issues `rmapps push --if-missing` for the EPUB and `--if-missing --name "<Title> (PDF)"` for the PDF, into `/Books/Purchased/kobo/<Author>`, and issues no `rm`.
- [ ] The rmapps binary is resolved via `RMAPPS_BIN`, else `~/git/rmapps/target/release/rmapps`, else `rmapps` on PATH.
- [ ] "Paired" is determined by `~/.config/rmapps/auth.json` existing.
- [ ] `pytest tests/test_kobo_getbooks.py` passes.

**Verify:** `cd ~/git/dotfiles && python -m pytest tests/test_kobo_getbooks.py -q` → all pass.

**Steps:**

- [ ] **Step 1: Replace the rmapi block in `bin/kobo_getbooks`** — replace everything from the `# ---------- rmapi push ----------` header down through the end of `rmapi_put` (the `RMAPI_CONF`, `_RMAPI_SNAPSHOT`, `_rmapi_paired`, `_rmapi_snapshot_conf`, `_rmapi_guard`, `rmapi_mkdir`, `rmapi_put` definitions) with:

```python
# ---------- rmapps push ----------
#
# Pushes to /Books/Purchased/kobo/<Author>/ on my.remarkable.com via the native
# `rmapps push --if-missing`. `--if-missing` creates the doc only when absent and
# is a no-op (exit 0) when a same-named doc already exists — so an annotated cloud
# copy is NEVER overwritten. `push` also creates the destination folder, so there
# is no separate mkdir chain.
#
# rmapps replaces rmapi: cloud access uses the native rm-cloud client, paired via
# `rmapps auth` (credentials in ~/.config/rmapps/auth.json). There is no
# token-clobber bug to guard against.

RMAPPS_AUTH = Path.home() / ".config" / "rmapps" / "auth.json"


def _rmapps_bin() -> str:
    """Resolve the rmapps binary: $RMAPPS_BIN, else the saturn build path, else
    `rmapps` on PATH."""
    env = os.environ.get("RMAPPS_BIN")
    if env and os.access(env, os.X_OK):
        return env
    built = Path.home() / "git" / "rmapps" / "target" / "release" / "rmapps"
    if built.is_file() and os.access(built, os.X_OK):
        return str(built)
    return "rmapps"


def _rmapps_paired() -> bool:
    """True iff rmapps has stored cloud credentials."""
    return RMAPPS_AUTH.exists()


def rmapps_push_if_missing(local_file: Path, remote_dir: str, name: str | None = None) -> None:
    """Upload a local file into a remote folder, creating it only if absent.

    INVARIANT: never overwrite an existing cloud document — a book the user may
    have annotated. `--if-missing` guarantees this: if a same-named doc already
    exists, rmapps leaves it untouched and exits 0. We deliberately never issue a
    delete-then-create replace, because that would destroy cloud annotations.

    `name` overrides the visible name (used to give the PDF a " (PDF)" suffix so
    it doesn't collide with the EPUB in the same folder).
    """
    cmd = [_rmapps_bin(), "push", "--if-missing"]
    if name is not None:
        cmd += ["--name", name]
    cmd += [str(local_file), remote_dir]
    proc = _run(cmd)
    if proc.returncode == 0:
        return
    raise RuntimeError(
        f"rmapps push {local_file.name!r} → {remote_dir!r} failed "
        f"(exit {proc.returncode}): {proc.stderr.strip()}"
    )
```

- [ ] **Step 2: Rewrite `_push_book`** — replace its body so it calls the new helper (folder is created implicitly; PDF gets the `(PDF)` visible name via `--name`):

```python
def _push_book(
    epub_path: Path,
    pdf_path: Path | None,
    author: str,
    title: str,
) -> tuple[bool, bool]:
    """Push one book to /Books/Purchased/kobo/<Author>/.

    Returns (epub_pushed, pdf_pushed). On failure of either, logs and continues —
    local copies still exist + sync via Filen.
    """
    remote_dir = f"/Books/Purchased/kobo/{author}"
    epub_pushed = False
    pdf_pushed = False

    try:
        rmapps_push_if_missing(epub_path, remote_dir)
        print(f"      rmapps push → {remote_dir}/{title}")
        epub_pushed = True
    except (RuntimeError, FileNotFoundError, OSError) as e:
        _log_error(f"  rmapps push epub failed: {e}")

    if pdf_path is not None:
        try:
            rmapps_push_if_missing(pdf_path, remote_dir, name=f"{title} (PDF)")
            print(f"      rmapps push → {remote_dir}/{title} (PDF)")
            pdf_pushed = True
        except (RuntimeError, FileNotFoundError, OSError) as e:
            _log_error(f"  rmapps push pdf failed: {e}")

    return epub_pushed, pdf_pushed
```

Note: the old code staged the PDF under a temp `<Title> (PDF).pdf` filename to control the visible name; `--name` does that directly now, so no temp staging is needed.

- [ ] **Step 3: Swap remaining `_rmapi_paired()` references** — search `bin/kobo_getbooks` for `_rmapi_paired`, `_rmapi_snapshot_conf`, and `RMAPI_CONF` and replace each `_rmapi_paired()` call with `_rmapps_paired()`. Remove the `_rmapi_snapshot_conf()` call in `main()` and its surrounding snapshot comment (no clobber guard needed). Update the module docstring's "rmapi note:" paragraph to describe rmapps pairing (`~/.config/rmapps/auth.json`) and drop the rmapi.conf wording.

Run to find them:
```bash
cd ~/git/dotfiles && grep -n "rmapi" bin/kobo_getbooks
```
Expected after edits: no matches.

- [ ] **Step 4: Update the test shims** — in `tests/test_kobo_getbooks.py`, replace `_install_rmapi_shim` and `_install_rmapi_shim_existing_remote` with rmapps equivalents, and point `RMAPPS_BIN` at the stub. Replace the two helper functions with:

```python
def _install_rmapps_shim(bin_dir: Path, log_path: Path, monkeypatch) -> None:
    """Drop an `rmapps` shim that logs every invocation and always succeeds, and
    point RMAPPS_BIN + a paired auth.json at it."""
    src = textwrap.dedent(f"""\
        #!/usr/bin/env python3
        import sys, pathlib
        log = pathlib.Path({str(log_path)!r})
        with open(log, "a") as f:
            f.write(" ".join(sys.argv[1:]) + "\\n")
        sys.exit(0)
    """)
    shim = bin_dir / "rmapps"
    shim.write_text(src)
    os.chmod(shim, 0o755)
    monkeypatch.setenv("RMAPPS_BIN", str(shim))
    # Mark rmapps as paired: ~/.config/rmapps/auth.json must exist.
    auth = Path(os.environ["HOME"]) / ".config" / "rmapps" / "auth.json"
    auth.parent.mkdir(parents=True, exist_ok=True)
    auth.write_text('{"device_token": "stub"}')
```

(There is no separate "existing remote" shim: `--if-missing` always exits 0 whether it created or skipped, so the no-overwrite invariant is enforced by rmapps itself. The regression test below asserts `_push_book` uses `--if-missing` and never `rm`.)

- [ ] **Step 5: Rewrite `TestPushBook` and `TestNoClobberAnnotatedBooks`** — replace both classes with rmapps-oriented versions. `kg`, `tmp_path`, `monkeypatch`, `Path`, `shutil` are already imported/available in the file:

```python
class TestPushBook:
    """Unit tests for _push_book — the rmapps cloud-push path."""

    def test_pushes_epub_and_renamed_pdf(self, kg, tmp_path, monkeypatch):
        """_push_book pushes the epub and the PDF (PDF given a ' (PDF)' visible
        name via --name), both with --if-missing, into the author folder."""
        bin_dir = tmp_path / "bin"
        bin_dir.mkdir()
        monkeypatch.setenv("HOME", str(tmp_path / "home"))
        log = tmp_path / "rmapps.log"
        _install_rmapps_shim(bin_dir, log, monkeypatch)

        epub = tmp_path / "Children of Ruin.epub"
        epub.write_bytes(b"epub-bytes")
        pdf = tmp_path / "Children of Ruin.pdf"
        pdf.write_bytes(b"%PDF-1.4\n" + b"x" * 100)

        epub_pushed, pdf_pushed = kg._push_book(epub, pdf, "Adrian Tchaikovsky", "Children of Ruin")
        assert epub_pushed is True
        assert pdf_pushed is True

        lines = log.read_text().splitlines()
        # epub: push --if-missing <epub> /Books/Purchased/kobo/Adrian Tchaikovsky
        assert any(
            l.startswith("push --if-missing ")
            and "Children of Ruin.epub" in l
            and l.endswith("/Books/Purchased/kobo/Adrian Tchaikovsky")
            and "--name" not in l
            for l in lines
        ), lines
        # pdf: push --if-missing --name "Children of Ruin (PDF)" <pdf> <dir>
        assert any(
            l.startswith("push --if-missing --name ")
            and "Children of Ruin (PDF)" in l
            and "Children of Ruin.pdf" in l
            for l in lines
        ), lines

    def test_epub_only_when_no_pdf(self, kg, tmp_path, monkeypatch):
        """_push_book(pdf_path=None) pushes only the epub."""
        bin_dir = tmp_path / "bin"
        bin_dir.mkdir()
        monkeypatch.setenv("HOME", str(tmp_path / "home"))
        log = tmp_path / "rmapps.log"
        _install_rmapps_shim(bin_dir, log, monkeypatch)

        epub = tmp_path / "Solo.epub"
        epub.write_bytes(b"epub")
        epub_pushed, pdf_pushed = kg._push_book(epub, None, "AuthorX", "Solo")
        assert epub_pushed is True
        assert pdf_pushed is False

        push_lines = [l for l in log.read_text().splitlines() if l.startswith("push ")]
        assert len(push_lines) == 1
        assert "Solo.epub" in push_lines[0]


class TestNoClobberAnnotatedBooks:
    """Regression: the push path must use --if-missing and NEVER issue an
    `rmapps rm`, so an annotated cloud book is never overwritten."""

    def test_push_uses_if_missing_and_never_rm(self, kg, tmp_path, monkeypatch):
        bin_dir = tmp_path / "bin"
        bin_dir.mkdir()
        monkeypatch.setenv("HOME", str(tmp_path / "home"))
        log = tmp_path / "rmapps.log"
        _install_rmapps_shim(bin_dir, log, monkeypatch)

        epub = tmp_path / "Annotated Book.epub"
        epub.write_bytes(b"epub-bytes")
        pdf = tmp_path / "Annotated Book.pdf"
        pdf.write_bytes(b"%PDF-1.4\n" + b"x" * 100)

        epub_pushed, pdf_pushed = kg._push_book(epub, pdf, "AuthorY", "Annotated Book")
        assert epub_pushed is True
        assert pdf_pushed is True

        lines = log.read_text().splitlines()
        # Every push uses --if-missing (the no-overwrite guarantee).
        assert all("--if-missing" in l for l in lines if l.startswith("push ")), lines
        # No `rm` subcommand may ever be issued.
        rm_calls = [l for l in lines if l.split() and l.split()[0] == "rm"]
        assert rm_calls == [], f"rmapps 'rm' issued — would destroy annotations! {rm_calls}"
```

- [ ] **Step 6: Run the tests**

Run: `cd ~/git/dotfiles && python -m pytest tests/test_kobo_getbooks.py -q`
Expected: PASS. If integration tests in `TestIntegration*` previously assumed no push happened (no rmapi paired), they remain valid — `_rmapps_paired()` is False unless `~/.config/rmapps/auth.json` exists, which those tests don't create.

- [ ] **Step 7: Commit**

```bash
cd ~/git/dotfiles
git checkout -b kobo-rmapps-swap
git add bin/kobo_getbooks tests/test_kobo_getbooks.py
git commit -m "feat(kobo): push books via rmapps instead of rmapi"
```

```json:metadata
{"files": ["~/git/dotfiles/bin/kobo_getbooks", "~/git/dotfiles/tests/test_kobo_getbooks.py"], "verifyCommand": "cd ~/git/dotfiles && python -m pytest tests/test_kobo_getbooks.py -q", "acceptanceCriteria": ["_push_book uses rmapps push --if-missing for epub+pdf", "binary resolution via RMAPPS_BIN/built/PATH", "paired via auth.json", "no rm ever issued", "pytest passes"], "requiresUserVerification": false}
```

---

### Task 7: Remove rmapi from dotfiles Nix + overlays

**Goal:** Delete every remaining rmapi reference so the package, overlay, and secrets no longer exist.

**Files:**
- Modify: `~/git/dotfiles/nixos/saturn/kobo-books.nix`
- Delete: `~/git/dotfiles/overlays/rmapi.nix`
- Modify: `~/git/dotfiles/flake.nix`
- Modify: `~/git/dotfiles/secrets/manifest.nix` (only if rmapi entries are now unused)

**Acceptance Criteria:**
- [ ] `rmapi` is gone from `kobo-books.nix` `systemPackages`, with the comment updated.
- [ ] `overlays/rmapi.nix` is deleted and no longer referenced in `flake.nix`.
- [ ] Any rmapi-only `secrets/manifest.nix` entry is removed (kept only if still referenced elsewhere).
- [ ] `nix flake check` (or at least `nix eval` of the saturn system) evaluates without error.

**Verify:** `cd ~/git/dotfiles && grep -rn "rmapi" nixos overlays flake.nix secrets` → no matches; flake evaluates.

**Steps:**

- [ ] **Step 1: Edit `nixos/saturn/kobo-books.nix`** — remove the `rmapi` entry from `environment.systemPackages` and the `# rmapi (overlay-patched ...)` / `# bin/kobo_getbooks shells out to ...` comment lines. Leave `calibre`, `kobodl`, `kepubify`, `poppler-utils`. Update the module header comment that says it installs the CLIs the orchestrator shells out to — note that book push now goes through `rmapps` (built separately, see `remarkable.nix`), so no rmapi package is installed here.

- [ ] **Step 2: Find and remove the overlay reference** — locate where `overlays/rmapi.nix` is wired:

```bash
cd ~/git/dotfiles && grep -rn "rmapi" flake.nix overlays/
```

Remove the import/overlay-list entry for `rmapi.nix` from `flake.nix`, then delete the file:

```bash
cd ~/git/dotfiles && git rm overlays/rmapi.nix
```

- [ ] **Step 3: Clean secrets manifest** — check whether the rmapi entry in `secrets/manifest.nix` is referenced anywhere else:

```bash
cd ~/git/dotfiles && grep -rn "rmapi" . --include=*.nix
```

If the only remaining hit is the `secrets/manifest.nix` definition itself, remove that entry. If something else still references it, leave it and note why in the commit message.

- [ ] **Step 4: Verify no references remain and the flake evaluates**

Run:
```bash
cd ~/git/dotfiles
grep -rn "rmapi" nixos overlays flake.nix secrets || echo "no rmapi references"
nix flake check 2>&1 | tail -20
```
Expected: "no rmapi references"; flake check passes (or, if `nix flake check` is heavy, `nix eval .#nixosConfigurations.saturn.config.system.build.toplevel.drvPath` evaluates without error).

- [ ] **Step 5: Commit**

```bash
cd ~/git/dotfiles
git add -A
git commit -m "chore(kobo): drop rmapi package, overlay, and secret (replaced by rmapps)"
```

```json:metadata
{"files": ["~/git/dotfiles/nixos/saturn/kobo-books.nix", "~/git/dotfiles/overlays/rmapi.nix", "~/git/dotfiles/flake.nix", "~/git/dotfiles/secrets/manifest.nix"], "verifyCommand": "cd ~/git/dotfiles && grep -rn rmapi nixos overlays flake.nix secrets; nix flake check", "acceptanceCriteria": ["rmapi removed from kobo-books.nix", "overlays/rmapi.nix deleted + unreferenced", "secrets entry removed if unused", "flake evaluates"], "requiresUserVerification": false}
```

---

## Self-Review

**Spec coverage:** Component 1 → Task 1; Component 2 → Task 2; Component 3 → Task 3; Component 4 → Task 4; Component 5 (delete safety audit) → no code change, verified by existing `rm` behavior and not regressed (noted in spec); Components 6 (dotfiles swap) → Tasks 6 & 7; workspace-green gate → Task 5. All spec sections covered.

**Type consistency:** `DocKind` defined in Task 2 and used identically in Tasks 2–3; `create_if_missing`/`replace_doc`/`create_if_missing_in_kind`/`replace_in_kind` names consistent across cloud.rs and push.rs; `pick_source` returns `(&'static str, Vec<u8>)` used consistently in get.rs; `rmapps_push_if_missing`/`_rmapps_paired`/`_rmapps_bin` names consistent across kobo_getbooks and its tests.

**Verification requirement scan:** The prompt asked for safe CLI verbs and automated tests, with no human-in-the-loop sign-off. Answer: **NO** — no `requiresUserVerification: true` task required.

**Placeholder scan:** No TBD/TODO/"handle edge cases"; every code step shows full code.
