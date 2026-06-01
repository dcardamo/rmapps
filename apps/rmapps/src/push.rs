//! `rmapps push` — generic one-shot PDF upload to a cloud folder.
//!
//! - default: `replace` (destructive create-or-replace; no ink to preserve).
//! - `--content-only`: `upsert` (create if missing, else content-only refresh that
//!   preserves on-device handwriting).

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;

use crate::cloud::{self, Cloud};

#[derive(Args)]
pub struct PushArgs {
    /// Path to the PDF file to upload.
    pdf: PathBuf,
    /// Destination cloud folder (slash path, e.g. `/Books`); created if missing.
    folder: String,
    /// Visible name on the device. Defaults to the PDF file stem.
    #[arg(long)]
    name: Option<String>,
    /// Content-only refresh (preserves on-device ink) instead of a destructive
    /// create-or-replace.
    #[arg(long = "content-only")]
    content_only: bool,
}

pub fn run(args: PushArgs) -> Result<()> {
    let bytes = std::fs::read(&args.pdf)
        .with_context(|| format!("reading PDF {}", args.pdf.display()))?;
    let name = match &args.name {
        Some(n) => n.clone(),
        None => cloud::doc_name(&args.pdf)?,
    };

    let cloud = Cloud::from_stored()?;
    let mode = if args.content_only {
        cloud.upsert(&args.folder, &name, bytes)?;
        "content-only"
    } else {
        cloud.replace(&args.folder, &name, bytes)?;
        "replace"
    };

    println!("pushed {name} -> {} ({mode})", args.folder);
    Ok(())
}
