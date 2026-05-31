use anyhow::{anyhow, Result};
use serde::Deserialize;
use typst::foundations::{Label, Selector};
use typst::introspection::MetadataElem;
use typst::layout::PagedDocument;
use typst::utils::PicoStr;

use crate::world::SpikeWorld;

/// A labelled region recovered from Typst introspection.
/// Coordinates are in Typst's top-left origin space (y increases downward), in pt.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct TypstRegion {
    pub name: String,
    pub page: usize,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A rectangle in PDF user space (bottom-left origin, y increases upward), in pt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PdfRect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// Convert a Typst top-left-origin rect to a PDF bottom-left-origin rect.
///
/// Typst gives coordinates with y=0 at the top of the page, increasing downward.
/// PDF user space has y=0 at the bottom of the page, increasing upward.
pub fn typst_to_pdf_rect(r: &TypstRegion, page_height_pt: f64) -> PdfRect {
    PdfRect {
        x0: r.x,
        y0: page_height_pt - (r.y + r.h),
        x1: r.x + r.w,
        y1: page_height_pt - r.y,
    }
}

/// Compile Typst source, recover `<region>`-labelled metadata via the
/// introspector, and return `(pdf_bytes, regions, page_height_pt)`.
///
/// # How metadata recovery works
/// 1. Compile to `PagedDocument` (same path as `compile_pdf`).
/// 2. Build a `Selector::Label` for the label `region`.
/// 3. Call `document.introspector.query(&selector)` to obtain all `Content`
///    elements that carry that label.
/// 4. Each element is a `MetadataElem`; downcast via `to_packed::<MetadataElem>()`.
/// 5. The `value` field is a Typst `Value` (a `Dict` in this case); it
///    implements `serde::Serialize`, so we serialize it to `serde_json::Value`
///    and then deserialize into `TypstRegion`.
pub fn compile_with_regions(src: &str) -> Result<(Vec<u8>, Vec<TypstRegion>, f64)> {
    let world = SpikeWorld::new(src);

    // Compile once; we need the PagedDocument for both metadata and PDF export.
    let result = typst::compile::<PagedDocument>(&world);
    let document = result
        .output
        .map_err(|diags| anyhow!("typst compile failed: {:?}", diags))?;

    // Export PDF bytes.
    let pdf = typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default())
        .map_err(|diags| anyhow!("typst pdf export failed: {:?}", diags))?;

    // Page height in pt (used for coordinate conversion).
    let page_height_pt = document
        .pages
        .first()
        .ok_or_else(|| anyhow!("document has no pages"))?
        .frame
        .height()
        .to_pt();

    // Build a label selector for `<region>`.
    let label = Label::new(PicoStr::intern("region")).ok_or_else(|| anyhow!("empty label"))?;
    let selector = Selector::Label(label);

    // Query the introspector for all elements with that label.
    let elems = document.introspector.query(&selector);

    // For each matching Content, downcast to MetadataElem and deserialize the value.
    let mut regions = Vec::with_capacity(elems.len());
    for elem in &elems {
        let packed = elem
            .to_packed::<MetadataElem>()
            .ok_or_else(|| anyhow!("labelled element is not a MetadataElem"))?;
        // MetadataElem::value is a typst::foundations::Value which implements Serialize.
        let json_val =
            serde_json::to_value(&packed.value).map_err(|e| anyhow!("Value->JSON: {e}"))?;
        let region: TypstRegion =
            serde_json::from_value(json_val).map_err(|e| anyhow!("JSON->TypstRegion: {e}"))?;
        regions.push(region);
    }

    Ok((pdf, regions, page_height_pt))
}
