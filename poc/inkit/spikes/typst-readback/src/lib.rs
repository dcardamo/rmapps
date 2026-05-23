pub mod html;
pub mod regions;
pub mod rmapi;
pub mod test_docs;
pub mod world;

use anyhow::{anyhow, Result};
use typst::layout::PagedDocument;
use world::SpikeWorld;

/// Compile Typst source to PDF bytes.
///
/// Returns the raw PDF bytes on success. The caller can verify the result
/// by checking that it starts with `%PDF`.
pub fn compile_pdf(src: &str) -> Result<Vec<u8>> {
    let world = SpikeWorld::new(src);
    // typst::compile returns Warned<SourceResult<D>>
    let result = typst::compile::<PagedDocument>(&world);
    let document = result
        .output
        .map_err(|diags| anyhow!("typst compile failed: {:?}", diags))?;
    // typst_pdf::pdf returns SourceResult<Vec<u8>> directly in 0.14
    let pdf = typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default())
        .map_err(|diags| anyhow!("typst pdf export failed: {:?}", diags))?;
    Ok(pdf)
}
