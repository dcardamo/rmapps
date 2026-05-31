//! Inspect a local `.rmdoc` bundle: print the extracted marks and render the
//! Digest + Annotated PDFs to /tmp/rmd-inspect-out/ for manual QA.
//!
//! Usage: nix develop -c cargo run --example inspect -- /path/to/doc.rmdoc
use rmdigest::annotate::assemble;
use rmdigest::digest_doc::{build_digest, DigestMeta};
use rmdigest::extract::{extract, Mark};
use rmdigest::render::compile;
use rmfiles::Bundle;
use std::path::PathBuf;
use std::process::Command;

fn main() -> anyhow::Result<()> {
    let arg = std::env::args().nth(1).expect("usage: inspect <doc.rmdoc>");
    let bundle = Bundle::open(&PathBuf::from(&arg))?;
    let title = bundle.metadata().visible_name.clone();
    let n_pages = bundle.pages().len();
    let src_pages = bundle
        .source_pdf()
        .and_then(|p| lopdf::Document::load_mem(p).ok())
        .map(|d| d.get_pages().len())
        .unwrap_or(0);
    eprintln!("title: {title:?}  bundle pages: {n_pages}  source PDF pages: {src_pages}");

    let all: Vec<usize> = bundle.pages().iter().map(|p| p.index).collect();
    let marks = extract(&bundle, &all)?;

    let mut n_h = 0;
    let mut n_n = 0;
    println!("\n--- extracted marks ({}) ---", marks.len());
    for m in &marks {
        match m {
            Mark::Highlight { page, text, color } => {
                n_h += 1;
                println!("  [HL p{:>3} {:?}] {:?}", page + 1, color, text);
            }
            Mark::Note { page, png } => {
                n_n += 1;
                println!("  [NOTE p{:>3}] {} png bytes", page + 1, png.len());
            }
            Mark::InsertedPage { after_page, png } => {
                n_n += 1;
                println!(
                    "  [INSERTED after p{:>3}] {} png bytes",
                    after_page + 1,
                    png.len()
                );
            }
        }
    }
    println!("--- {n_h} highlights, {n_n} notes ---\n");

    let meta = DigestMeta {
        title: if title.is_empty() {
            "Untitled".into()
        } else {
            title
        },
        author: String::new(),
        n_highlights: n_h,
        n_notes: n_n,
        date_range: String::new(),
    };

    let out = PathBuf::from("/tmp/rmd-inspect-out");
    std::fs::create_dir_all(&out)?;
    let (src, assets) = build_digest(&meta, &marks, &rmdigest::device::MOVE);
    std::fs::write(out.join("Digest.pdf"), compile(&src, &assets)?)?;
    let mut rendered = vec![("digest", "Digest.pdf")];
    match assemble(&bundle, &meta, &marks, &rmdigest::device::MOVE) {
        Ok(pdf) => {
            std::fs::write(out.join("Annotated.pdf"), pdf)?;
            rendered.push(("annotated", "Annotated.pdf"));
        }
        Err(e) => eprintln!("annotated assembly failed (digest still produced): {e:#}"),
    }
    for (label, f) in rendered {
        let _ = Command::new("pdftoppm")
            .args([
                "-r",
                "150",
                "-png",
                out.join(f).to_str().unwrap(),
                out.join(label).to_str().unwrap(),
            ])
            .status();
    }
    eprintln!("wrote PDFs + PNGs to {out:?}");
    Ok(())
}
