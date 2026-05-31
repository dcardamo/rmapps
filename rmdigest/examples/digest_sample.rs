//! Build a sample Digest PDF from the real `stamped-labels.rmdoc` fixture and
//! write it to `/tmp/digest_sample.pdf`.  Then rasterise each page to
//! `/tmp/digest_sample-N.png` via `pdftoppm` for visual inspection.
//!
//! Usage (from ~/git/wt/rmdigest):
//!   nix develop -c cargo run --example digest_sample
use rmdigest::digest_doc::{build_digest, DigestMeta};
use rmdigest::extract::extract;
use rmdigest::render::compile;
use rmfiles::Bundle;
use std::path::PathBuf;
use std::process::Command;

fn main() -> anyhow::Result<()> {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let fixture = PathBuf::from(manifest).join("tests/fixtures/stamped-labels.rmdoc");

    let bundle = Bundle::open(&fixture)?;
    let pages = bundle.pages();
    let all_indices: Vec<usize> = pages.iter().map(|p| p.index).collect();

    let marks = extract(&bundle, &all_indices)?;

    let n_highlights = marks
        .iter()
        .filter(|m| matches!(m, rmdigest::extract::Mark::Highlight { .. }))
        .count();
    let n_notes = marks
        .iter()
        .filter(|m| {
            matches!(
                m,
                rmdigest::extract::Mark::Note { .. } | rmdigest::extract::Mark::InsertedPage { .. }
            )
        })
        .count();

    let meta = DigestMeta {
        title: "The Stamped Labels Test".into(),
        author: "reMarkable Device".into(),
        n_highlights,
        n_notes,
        date_range: "May 2026".into(),
    };

    eprintln!("marks: {n_highlights} highlights, {n_notes} notes");

    let (src, assets) = build_digest(&meta, &marks);

    // Write the typst source for debugging.
    std::fs::write("/tmp/digest_sample.typ", &src)?;
    eprintln!("Typst source written to /tmp/digest_sample.typ");

    let pdf = compile(&src, &assets)?;
    std::fs::write("/tmp/digest_sample.pdf", &pdf)?;
    eprintln!(
        "PDF written to /tmp/digest_sample.pdf ({} bytes)",
        pdf.len()
    );

    // Rasterise pages with pdftoppm.
    let status = Command::new("pdftoppm")
        .args([
            "-r",
            "150",
            "-png",
            "/tmp/digest_sample.pdf",
            "/tmp/digest_sample",
        ])
        .status();

    match status {
        Ok(s) if s.success() => {
            // List generated PNGs.
            let mut pngs: Vec<_> = std::fs::read_dir("/tmp")?
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let n = e.file_name();
                    let name = n.to_string_lossy();
                    name.starts_with("digest_sample-") && name.ends_with(".png")
                })
                .map(|e| e.path())
                .collect();
            pngs.sort();
            eprintln!("Pages rasterised:");
            for p in &pngs {
                eprintln!("  {}", p.display());
            }
        }
        Ok(s) => eprintln!("pdftoppm exited with {s}"),
        Err(e) => eprintln!("pdftoppm not available: {e}"),
    }

    Ok(())
}
