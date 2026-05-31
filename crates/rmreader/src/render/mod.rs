//! Render a reader collection (index + articles) to PDF via Typst.
//!
//! Typst replaced fulgur/krilla here because fulgur's text layer is broken for
//! snap-to-text read-back: it tags every wrapped line with the *whole* paragraph
//! as `/ActualText` + ToUnicode, so the reMarkable's snapped highlights read back
//! shifted/duplicated. Typst emits a clean per-glyph text layer, so highlights
//! round-trip exactly. The chrome (paper fill, nav bar, action band, links,
//! bookmarks) is drawn in-flow by Typst; only the manifest embed stays in lopdf.
use crate::device::Device;
use crate::theme::Palette;

pub mod html2typst;
pub mod typst_doc;
pub mod world;

pub use world::RmWorld;

/// Compile Typst `src` (with `assets` served via `file()`) to PDF bytes.
pub fn compile_pdf(src: &str, assets: &[(String, Vec<u8>)]) -> anyhow::Result<Vec<u8>> {
    Ok(render_collection_src(src, assets)?.0)
}

/// A `<region>`-labelled metadata record recovered from a compiled document.
/// `art-{id}` regions carry only `name`+`page`; `action-{LABEL}` regions also
/// carry the cell rect (`x`/`y`/`w`/`h`, Typst top-left origin, pt).
#[derive(Debug, serde::Deserialize)]
struct RawRegion {
    name: String,
    page: usize,
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
    #[serde(default)]
    w: f64,
    #[serde(default)]
    h: f64,
}

/// Compile a Typst source once and recover the PDF bytes, the page height (pt),
/// and the `<region>` metadata. Shared by `compile_pdf` and `render_collection`.
fn render_collection_src(
    src: &str,
    assets: &[(String, Vec<u8>)],
) -> anyhow::Result<(Vec<u8>, f64, Vec<RawRegion>)> {
    use typst::foundations::{Label, Selector};
    use typst::introspection::MetadataElem;
    use typst::utils::PicoStr;

    let world = RmWorld::new(src, assets);
    let document = typst::compile::<typst::layout::PagedDocument>(&world)
        .output
        .map_err(|d| anyhow::anyhow!("typst compile failed: {d:?}"))?;
    let pdf = typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default())
        .map_err(|d| anyhow::anyhow!("typst pdf export failed: {d:?}"))?;

    let page_h = document
        .pages
        .first()
        .map(|p| p.frame.height().to_pt())
        .unwrap_or(0.0);

    let label = Label::new(PicoStr::intern("region")).ok_or_else(|| anyhow::anyhow!("label"))?;
    let elems = document.introspector.query(&Selector::Label(label));
    let mut regions = Vec::with_capacity(elems.len());
    for elem in &elems {
        if let Some(packed) = elem.to_packed::<MetadataElem>() {
            let json = serde_json::to_value(&packed.value)?;
            if let Ok(r) = serde_json::from_value::<RawRegion>(json) {
                regions.push(r);
            }
        }
    }
    Ok((pdf, page_h, regions))
}

/// A fully rendered collection: PDF bytes plus the read-back geometry recovered
/// from the Typst document.
pub struct Rendered {
    pub pdf: Vec<u8>,
    /// article id -> 0-based inclusive page range.
    pub page_ranges: std::collections::HashMap<String, crate::manifest::PageRange>,
    /// action-band cell rects (inbox/archive/later/delete), PDF bottom-left origin.
    pub label_rects: Vec<crate::manifest::LabelRect>,
}

/// Render a whole collection (index + articles) to PDF via Typst and recover the
/// read-back geometry. `articles` must be in document order.
pub fn render_collection(
    device: &Device,
    theme: &Palette,
    collection: &str,
    rows: &[typst_doc::Row],
    articles: &[typst_doc::Article],
    assets: &[(String, Vec<u8>)],
) -> anyhow::Result<Rendered> {
    let src = typst_doc::build(device, theme, collection, rows, articles);
    if std::env::var("RMREADER_DUMP_TYPST").is_ok() {
        let _ = std::fs::write(format!("/tmp/rmreader_{collection}.typ"), &src);
    }
    let (pdf, page_h, regions) = render_collection_src(&src, assets)?;

    // Total page count: read it back from the PDF we just produced.
    let doc = lopdf::Document::load_mem(&pdf)?;
    let total_pages = doc.get_pages().len();

    // Article first pages, in document order, from the art-{id} regions.
    let order: Vec<&str> = articles.iter().map(|a| a.anchor.as_str()).collect();
    let mut first_page: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for r in &regions {
        if let Some(id) = r.name.strip_prefix("art-") {
            // First occurrence wins (the article's opening page).
            first_page.entry(id).or_insert(r.page);
        }
    }
    let mut page_ranges = std::collections::HashMap::new();
    for (i, id) in order.iter().enumerate() {
        let Some(&first) = first_page.get(*id) else {
            continue;
        };
        // last = (next article's first page) - 1, else the final page.
        let last = order
            .iter()
            .skip(i + 1)
            .find_map(|nid| first_page.get(*nid).copied())
            .map(|np| np.saturating_sub(1))
            .unwrap_or(total_pages.saturating_sub(1));
        page_ranges.insert(
            (*id).to_string(),
            crate::manifest::PageRange { first, last },
        );
    }

    // Action label rects: first occurrence of each action-{LABEL} region,
    // converted from Typst top-left to PDF bottom-left coords.
    let mut label_rects = Vec::new();
    for lbl in typst_doc::ACTION_LABELS {
        let name = format!("action-{lbl}");
        if let Some(r) = regions.iter().find(|r| r.name == name) {
            label_rects.push(crate::manifest::LabelRect {
                kind: lbl.to_ascii_lowercase(),
                rect: crate::manifest::ManifestRect {
                    x0: r.x,
                    y0: page_h - (r.y + r.h),
                    x1: r.x + r.w,
                    y1: page_h - r.y,
                },
            });
        }
    }

    Ok(Rendered {
        pdf,
        page_ranges,
        label_rects,
    })
}
