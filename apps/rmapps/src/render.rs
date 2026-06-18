//! `rmapps render PATH [DEST]` — render a notebook's ink to a PDF on disk.
//!
//! Resolves PATH read-only into parent folder + leaf name (like `get`), finds
//! the document, downloads its full bundle (including `.rm` ink blobs), writes a
//! temporary `.rmdoc`, and flattens every page's ink onto white into a PDF at
//! DEST. Unlike `get` (which only extracts an existing PDF/EPUB source), this
//! works on pure-ink notebooks. Refuses to overwrite DEST unless `--force`.

use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};
use clap::Args;
use rmfiles::Bundle;

use crate::cloud::Cloud;

#[derive(Args)]
pub struct RenderArgs {
    /// Document path to render, e.g. `/Ideas/Adventure Self`.
    path: String,
    /// Destination PDF (default: `./<name>.pdf`).
    dest: Option<PathBuf>,
    /// Overwrite DEST if it already exists.
    #[arg(long)]
    force: bool,
}

pub fn run(args: RenderArgs) -> Result<()> {
    let cloud = Cloud::from_stored()?;

    // Split PATH into parent folder + leaf name (same shape as `get`).
    let trimmed = args.path.trim().trim_matches('/');
    if trimmed.is_empty() {
        bail!("refusing to render the cloud root");
    }
    let (parent_path, leaf) = match trimmed.rsplit_once('/') {
        Some((p, l)) => (p.to_string(), l.to_string()),
        None => (String::new(), trimmed.to_string()),
    };

    let parent_id = match cloud.resolve_folder(&parent_path)? {
        Some(id) => id,
        None => {
            println!("{}: not found", args.path);
            return Ok(());
        }
    };

    let entries = cloud.block_on(cloud.client().ls(&parent_id))?;
    let Some(target) = entries.into_iter().find(|e| e.name == leaf) else {
        println!("{}: not found", args.path);
        return Ok(());
    };
    if target.is_folder {
        bail!("{} is a folder, not a document", args.path);
    }

    // Download the full doc (all blobs incl. ink), write a temp .rmdoc, open it.
    let df = cloud.block_on(cloud.client().get(&target.id))?;
    let tmp = tempfile::Builder::new().suffix(".rmdoc").tempfile()?;
    df.write_rmdoc(tmp.path())
        .map_err(|e| anyhow!("writing temp rmdoc: {e}"))?;
    let bundle = Bundle::open(tmp.path()).map_err(|e| anyhow!("opening bundle: {e}"))?;
    let n_pages = bundle.pages().len();
    let pdf = rmdigest::notebook::render_bundle_pdf(&bundle)?;

    let dest = args
        .dest
        .unwrap_or_else(|| PathBuf::from(format!("{leaf}.pdf")));
    if dest.exists() && !args.force {
        bail!("{} already exists (use --force to overwrite)", dest.display());
    }
    std::fs::write(&dest, &pdf).map_err(|e| anyhow!("writing {}: {e}", dest.display()))?;
    println!(
        "rendered {} ({n_pages} pages) -> {}",
        args.path,
        dest.display()
    );
    Ok(())
}
