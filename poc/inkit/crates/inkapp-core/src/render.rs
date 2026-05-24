use typst::layout::PagedDocument;

use crate::error::{Error, Result};
use crate::world::InkWorld;

/// Compile Typst source to a laid-out document. The single-arg form authors no
/// component `.typ` files (used by the harness and most tests).
pub fn compile_to_document(src: &str) -> Result<PagedDocument> {
    compile_to_document_with_sources(src, &[])
}

/// Compile with additional registered Typst sources the main source may `#import`
/// (component render halves + the framework prelude).
pub fn compile_to_document_with_sources(
    src: &str,
    sources: &[(String, String)],
) -> Result<PagedDocument> {
    let world = InkWorld::with_sources(src, sources);
    typst::compile::<PagedDocument>(&world)
        .output
        .map_err(|d| Error::Compile(format!("{d:?}")))
}

/// Export a laid-out document to PDF bytes.
pub fn document_to_pdf(doc: &PagedDocument) -> Result<Vec<u8>> {
    typst_pdf::pdf(doc, &typst_pdf::PdfOptions::default()).map_err(|d| Error::Pdf(format!("{d:?}")))
}
