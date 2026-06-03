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
