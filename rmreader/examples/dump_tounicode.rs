//! Debug: dump the first ToUnicode CMap stream(s) (containing beginbfchar) from a PDF.
//! Used to diagnose the fulgur/krilla ToUnicode bug where each bfchar maps one glyph
//! code to a whole text run instead of per-character, breaking on-device text
//! extraction (snap-to-text) and pdftotext.
//! Usage: cargo run --example dump_tounicode -- <file.pdf>
use lopdf::{Document, Object};

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("usage: dump_tounicode <pdf>");
    let doc = Document::load(&path)?;
    let mut shown = 0;
    for (_id, obj) in &doc.objects {
        if let Object::Stream(s) = obj {
            let bytes = s
                .decompressed_content()
                .unwrap_or_else(|_| s.content.clone());
            let text = String::from_utf8_lossy(&bytes);
            if text.contains("beginbfchar") || text.contains("beginbfrange") {
                println!("===== CMap stream ({} bytes) =====", bytes.len());
                println!("{text}");
                shown += 1;
                if shown >= 2 {
                    break;
                }
            }
        }
    }
    if shown == 0 {
        println!("no CMap streams found");
    }
    Ok(())
}
