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
