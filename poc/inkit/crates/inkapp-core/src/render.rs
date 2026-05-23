use typst::layout::PagedDocument;

use crate::error::{Error, Result};
use crate::world::InkWorld;

/// Compile Typst source to a laid-out document (shared by PDF export and region
/// recovery — the single compile path for the whole framework).
pub fn compile_to_document(src: &str) -> Result<PagedDocument> {
    let world = InkWorld::new(src);
    typst::compile::<PagedDocument>(&world)
        .output
        .map_err(|d| Error::Compile(format!("{d:?}")))
}

/// Export a laid-out document to PDF bytes.
pub fn document_to_pdf(doc: &PagedDocument) -> Result<Vec<u8>> {
    typst_pdf::pdf(doc, &typst_pdf::PdfOptions::default()).map_err(|d| Error::Pdf(format!("{d:?}")))
}
