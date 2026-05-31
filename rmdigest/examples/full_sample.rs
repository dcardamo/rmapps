//! Build BOTH the Digest and Annotated PDFs from the real `stamped-labels.rmdoc`
//! fixture and write them (plus per-page PNGs) under `/tmp/rmdigest-sample/` for
//! holistic visual QA (Task 13).
//!
//! Usage (from ~/git/wt/rmdigest):
//!   nix develop -c cargo run --example full_sample
use rmdigest::annotate::assemble;
use rmdigest::digest_doc::{build_digest, DigestMeta};
use rmdigest::extract::{extract, Mark};
use rmdigest::render::compile;
use rmfiles::Bundle;
use std::path::PathBuf;
use std::process::Command;

fn main() -> anyhow::Result<()> {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let fixture = PathBuf::from(manifest).join("tests/fixtures/stamped-labels.rmdoc");
    let out = PathBuf::from("/tmp/rmdigest-sample");
    std::fs::create_dir_all(&out)?;

    let bundle = Bundle::open(&fixture)?;
    let all: Vec<usize> = bundle.pages().iter().map(|p| p.index).collect();
    let marks = extract(&bundle, &all)?;

    let n_highlights = marks
        .iter()
        .filter(|m| matches!(m, Mark::Highlight { .. }))
        .count();
    let n_notes = marks
        .iter()
        .filter(|m| matches!(m, Mark::Note { .. } | Mark::InsertedPage { .. }))
        .count();
    let meta = DigestMeta {
        title: "The Stamped Labels Test".into(),
        author: "reMarkable Device".into(),
        n_highlights,
        n_notes,
        date_range: "May 2026".into(),
    };
    eprintln!("marks: {n_highlights} highlights, {n_notes} notes");

    let (src, assets) = build_digest(&meta, &marks, &rmdigest::device::MOVE);
    let digest_pdf = compile(&src, &assets)?;
    let digest_path = out.join("Digest.pdf");
    std::fs::write(&digest_path, &digest_pdf)?;

    let annotated_pdf = assemble(&bundle, &meta, &marks, &rmdigest::device::MOVE)?;
    let annotated_path = out.join("Annotated.pdf");
    std::fs::write(&annotated_path, &annotated_pdf)?;

    for (label, path) in [("digest", &digest_path), ("annotated", &annotated_path)] {
        let prefix = out.join(label);
        let status = Command::new("pdftoppm")
            .args([
                "-r",
                "150",
                "-png",
                path.to_str().unwrap(),
                prefix.to_str().unwrap(),
            ])
            .status()?;
        eprintln!("{label}: {} -> {prefix:?}-*.png ({status})", path.display());
    }
    eprintln!("Sample output in {out:?}");
    Ok(())
}
