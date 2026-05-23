use serde::{Deserialize, Serialize};
use typst::foundations::{Label, Selector};
use typst::introspection::MetadataElem;
use typst::layout::PagedDocument;
use typst::utils::PicoStr;

use crate::error::{Error, Result};
use crate::geometry::{typst_to_pdf_rect, PdfRect};

/// The raw metadata an author/widget emits next to a `<region>` label.
/// Coordinates are Typst-space (top-left origin), in points.
#[derive(Debug, Clone, Deserialize)]
struct RawRegion {
    name: String,
    page: usize,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// A labelled rectangle on a page, in PDF-point coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub name: String,
    pub page: usize,
    pub rect: PdfRect,
}

/// The document's self-describing layout: regions plus a version marker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u64,
    pub regions: Vec<Region>,
}

/// Recover all `<region>`-labelled metadata from a compiled document and convert
/// each rect to PDF coordinates using its own page's height. `version` defaults
/// to 0; callers can set it via [`Manifest::with_version`].
pub fn recover_regions(doc: &PagedDocument) -> Result<Manifest> {
    let page_heights: Vec<f64> = doc.pages.iter().map(|p| p.frame.height().to_pt()).collect();

    let label = Label::new(PicoStr::intern("region"))
        .ok_or_else(|| Error::Region("empty region label".into()))?;
    let elems = doc.introspector.query(&Selector::Label(label));

    let mut regions = Vec::with_capacity(elems.len());
    for elem in &elems {
        let packed = elem
            .to_packed::<MetadataElem>()
            .ok_or_else(|| Error::Region("labelled element is not metadata".into()))?;
        // MetadataElem::value is a typst::foundations::Value which implements Serialize.
        let json = serde_json::to_value(&packed.value).map_err(|e| Error::Region(e.to_string()))?;
        let raw: RawRegion =
            serde_json::from_value(json).map_err(|e| Error::Region(e.to_string()))?;
        let page_h = *page_heights.get(raw.page).ok_or_else(|| {
            Error::Region(format!(
                "region '{}' references missing page {}",
                raw.name, raw.page
            ))
        })?;
        regions.push(Region {
            name: raw.name,
            page: raw.page,
            rect: typst_to_pdf_rect(raw.x, raw.y, raw.w, raw.h, page_h),
        });
    }
    Ok(Manifest {
        version: 0,
        regions,
    })
}

impl Manifest {
    /// Set the version marker (builder style).
    pub fn with_version(mut self, version: u64) -> Self {
        self.version = version;
        self
    }
}
