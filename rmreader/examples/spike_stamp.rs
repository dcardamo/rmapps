//! Spike harness for Task 5: generate a small, self-contained PDF that carries
//! (a) the four action labels stamped as real text (the production stamping path),
//! (b) a known body sentence to highlight, and (c) an embedded manifest. Upload it
//! to the reMarkable, highlight a label + the body sentence with snap-to-text, pull
//! it back, and verify: snap-to-text recovers the label text, and the embedded
//! manifest survives the cloud round trip.
//!
//! Usage: `cargo run --example spike_stamp -- <out.pdf>`
use lopdf::{dictionary, Document, Object, Stream};
use rmreader::embed;
use rmreader::manifest::{EmbeddedDoc, EmbeddedManifest, PageRange};

/// The exact body sentence the fixtures expect to be highlighted.
const BODY: &str = "The quick brown fox jumps over the lazy dog near the riverbank.";

fn main() -> anyhow::Result<()> {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/spike.pdf".to_string());

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();

    // Standard Helvetica (Type1) — the same kind of real text the postprocess nav
    // bar stamps; this is what we are testing snap-to-text against.
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });

    // One A4 page: action labels near the top, the known body sentence below.
    let content = format!(
        "q 0 0 0 rg BT /F1 16 Tf 50 790 Td (INBOX   ARCHIVE   LATER   DELETE) Tj ET Q\n\
         q 0 0 0 rg BT /F1 13 Tf 50 740 Td ({BODY}) Tj ET Q\n"
    );
    let content_id = doc.add_object(Object::Stream(Stream::new(
        dictionary! {},
        content.into_bytes(),
    )));

    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Contents" => content_id,
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
    });

    let pages = dictionary! {
        "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);

    embed::write(
        &mut doc,
        &EmbeddedManifest {
            schema_version: 1,
            collection: "Spike".into(),
            docs: vec![EmbeddedDoc {
                id: "spike-doc".into(),
                title: "Spike".into(),
                url: "https://example.com/spike".into(),
                author: String::new(),
                category: "articles".into(),
                page_range: PageRange { first: 0, last: 0 },
            }],
        },
    )?;

    doc.save(&out)?;
    println!("wrote {out} (body sentence to highlight: {BODY:?})");
    Ok(())
}
