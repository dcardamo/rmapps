use anyhow::{bail, Result};
use std::path::Path;
use std::process::{Command, Stdio};

/// Push a PDF preserving on-device ink (content-only). Spike-grade: assumes a paired,
/// v4-patched rmapi on PATH (provided by the flake). Mirrors rmreader's arg order.
pub fn push_content_only(pdf: &Path, folder: &str) -> Result<()> {
    // mkdir is best-effort/idempotent (rmapi errors on an existing dir).
    let _ = Command::new("rmapi")
        .args(["-ni", "mkdir", folder])
        .stdin(Stdio::null())
        .status();
    let pdf_str = pdf
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF-8 pdf path"))?;
    let ok = Command::new("rmapi")
        .args(["-ni", "put", "--content-only", pdf_str, folder])
        .stdin(Stdio::null())
        .status()?
        .success();
    if !ok {
        bail!("rmapi put --content-only failed for {}", pdf.display());
    }
    Ok(())
}
