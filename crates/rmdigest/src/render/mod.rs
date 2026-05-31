//! Compile typst source to PDF using a vendored-font World (see `world`).
pub mod world;
pub use world::RmWorld;

/// Compile typst `src` (with `assets` images served via `file()`) to PDF bytes.
pub fn compile(src: &str, assets: &[(String, Vec<u8>)]) -> anyhow::Result<Vec<u8>> {
    let world = RmWorld::new(src, assets);
    let doc = typst::compile::<typst::layout::PagedDocument>(&world)
        .output
        .map_err(|d| anyhow::anyhow!("typst compile failed: {d:?}"))?;
    let pdf = typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default())
        .map_err(|d| anyhow::anyhow!("typst pdf export failed: {d:?}"))?;
    Ok(pdf)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compiles_minimal_doc_to_valid_pdf() {
        let src = "#set page(width: 100pt, height: 100pt)\nHello";
        let pdf = compile(src, &[]).unwrap();
        let doc = lopdf::Document::load_mem(&pdf).unwrap();
        assert!(!doc.get_pages().is_empty());
    }
    #[test]
    fn compiles_with_vendored_fonts() {
        // Should compile cleanly using the vendored Newsreader + Hanken faces.
        let src = "#set text(font: \"Newsreader\")\n#text(font: \"Hanken Grotesk\")[Hi] World";
        let pdf = compile(src, &[]).unwrap();
        assert!(!lopdf::Document::load_mem(&pdf)
            .unwrap()
            .get_pages()
            .is_empty());
    }
}
