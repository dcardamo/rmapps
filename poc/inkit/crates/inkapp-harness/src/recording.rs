//! On-device recording: the gesture catalog, self-instructing template
//! generation, and (later task) extraction of captures into fixtures.

use inkapp_core::embed::embed_manifest;
use inkapp_core::error::Result;
use inkapp_core::geometry::PdfPoint;
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::{compile_to_document, document_to_pdf};
use inkapp_core::widget::region_metadata;

use crate::fixtures::{Fit, Tool};

/// Template page width in points. 0.75 aspect approximates the reMarkable canvas;
/// the device fits to width. Shared by generation and (later) extraction.
pub const PAGE_W: f64 = 420.0;
/// Template page height in points.
pub const PAGE_H: f64 = 560.0;

/// Top inset so the pen toolbar never covers a guide cell (mechanics doc §7).
const TOP_INSET: f64 = 48.0;

/// Guide-box shape for a gesture, matched to how it is naturally drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxShape {
    /// ~square (checkmark, circle, scribble-out, arrow).
    Square,
    /// Wide and short (swipe, strike, handwritten-word).
    Wide,
}

impl BoxShape {
    fn dims(self) -> (f64, f64) {
        match self {
            BoxShape::Square => (120.0, 120.0),
            BoxShape::Wide => (340.0, 80.0),
        }
    }
}

/// One entry in the gesture catalog — the single source of truth for a gesture.
#[derive(Debug, Clone, Copy)]
pub struct CatalogEntry {
    pub name: &'static str,
    pub tool: Tool,
    pub fit: Fit,
    pub instruction: &'static str,
    pub box_shape: BoxShape,
    /// Optional sample text to display inside each guide box.
    pub sample_text: Option<&'static str>,
}

/// The gesture vocabulary. Editing this list is how the vocabulary grows.
pub fn catalog() -> &'static [CatalogEntry] {
    &[
        CatalogEntry {
            name: "checkmark",
            tool: Tool::Pen,
            fit: Fit::AspectFit,
            instruction: "draw a check in each box",
            box_shape: BoxShape::Square,
            sample_text: None,
        },
        CatalogEntry {
            name: "scribble-out",
            tool: Tool::Pen,
            fit: Fit::StretchX,
            instruction: "scribble each box out",
            box_shape: BoxShape::Square,
            sample_text: None,
        },
        CatalogEntry {
            name: "highlight-swipe",
            tool: Tool::Highlighter,
            fit: Fit::StretchX,
            instruction: "swipe a highlight across the words",
            box_shape: BoxShape::Wide,
            sample_text: Some("highlight these words"),
        },
        CatalogEntry {
            name: "strike-through",
            tool: Tool::Pen,
            fit: Fit::StretchX,
            instruction: "strike the words out",
            box_shape: BoxShape::Wide,
            sample_text: Some("strike these words"),
        },
        CatalogEntry {
            name: "handwritten-word",
            tool: Tool::Pen,
            fit: Fit::AspectFit,
            instruction: "write the word: review",
            box_shape: BoxShape::Wide,
            sample_text: None,
        },
        CatalogEntry {
            name: "circle",
            tool: Tool::Pen,
            fit: Fit::AspectFit,
            instruction: "circle inside each box",
            box_shape: BoxShape::Square,
            sample_text: None,
        },
        CatalogEntry {
            name: "arrow",
            tool: Tool::Pen,
            fit: Fit::AspectFit,
            instruction: "draw an arrow left to right",
            box_shape: BoxShape::Square,
            sample_text: None,
        },
    ]
}

/// Number of guide boxes (samples) per gesture template.
pub const BOXES_PER_GESTURE: usize = 3;

/// Emit the standard page setup Typst preamble.
fn page_header() -> String {
    format!(
        "#set page(width: {PAGE_W}pt, height: {PAGE_H}pt, margin: 0pt)\n#set text(size: 11pt)\n"
    )
}

/// Place text at Typst top-left coords.
fn place_text(x: f64, y: f64, text: &str) -> String {
    // Escape characters that would break Typst's content block syntax.
    let esc = text
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]");
    format!("#place(top + left, dx: {x}pt, dy: {y}pt)[{esc}]\n")
}

/// Place a stroked rectangle at Typst top-left coords.
fn place_box(x: f64, y: f64, w: f64, h: f64) -> String {
    format!("#place(top + left, dx: {x}pt, dy: {y}pt, rect(width: {w}pt, height: {h}pt, stroke: 0.5pt))\n")
}

/// Render the self-instructing template PDF for one gesture.
///
/// The returned PDF has a manifest embedded; its regions are named
/// `box:<name>:0`, `box:<name>:1`, `box:<name>:2`.
pub fn render_template(entry: &CatalogEntry) -> Result<Vec<u8>> {
    let (bw, bh) = entry.box_shape.dims();
    let mut src = page_header();

    // Header line: gesture name + instruction.
    src.push_str(&place_text(
        24.0,
        16.0,
        &format!("{} -- {}", entry.name, entry.instruction),
    ));

    // One cell per sample box.
    let cell_h = bh + 30.0;
    for i in 0..BOXES_PER_GESTURE {
        let box_x = 24.0;
        let box_y = TOP_INSET + (i as f64) * cell_h + 14.0;

        // Region metadata (must precede any visual, so recovery finds it).
        src.push_str(&region_metadata(
            &format!("box:{}:{i}", entry.name),
            0,
            box_x,
            box_y,
            bw,
            bh,
        ));
        // Visual guide box.
        src.push_str(&place_box(box_x, box_y, bw, bh));
        // Optional sample text centered inside the box.
        if let Some(t) = entry.sample_text {
            src.push_str(&place_text(box_x + 8.0, box_y + bh / 2.0 - 6.0, t));
        }
    }

    let doc = compile_to_document(&src)?;
    let manifest = recover_regions(&doc)?.with_version(1);
    let pdf = document_to_pdf(&doc)?;
    embed_manifest(&pdf, &manifest)
}

/// Known calibration-cross centers in PDF space (bottom-left origin, y-up).
pub fn calibration_points() -> Vec<PdfPoint> {
    let m = 60.0;
    vec![
        PdfPoint { x: m, y: m }, // bottom-left
        PdfPoint {
            x: PAGE_W - m,
            y: m,
        }, // bottom-right
        PdfPoint {
            x: m,
            y: PAGE_H - m,
        }, // top-left
        PdfPoint {
            x: PAGE_W - m,
            y: PAGE_H - m,
        }, // top-right
        PdfPoint {
            x: PAGE_W / 2.0,
            y: PAGE_H / 2.0,
        }, // centre
    ]
}

/// Render the calibration sheet: crosshairs at known PDF points, each wrapped in
/// a `cross:<i>` region whose center equals `calibration_points()[i]`.
pub fn render_calibration() -> Result<Vec<u8>> {
    const HALF: f64 = 12.0;
    let mut src = page_header();
    src.push_str(&place_text(
        24.0,
        16.0,
        "Calibration -- tap the centre of each cross",
    ));

    for (i, p) in calibration_points().iter().enumerate() {
        // Convert PDF y-up center to Typst top-left origin for the region box.
        let tx = p.x - HALF;
        let ty = (PAGE_H - p.y) - HALF;

        // Region wraps the cross so extraction can locate it by name.
        src.push_str(&region_metadata(
            &format!("cross:{i}"),
            0,
            tx,
            ty,
            HALF * 2.0,
            HALF * 2.0,
        ));

        // Horizontal bar of the cross.
        // -0.3 centers the 0.6pt-thick bar on the known point.
        src.push_str(&format!(
            "#place(top + left, dx: {}pt, dy: {}pt, rect(width: {}pt, height: 0.6pt, fill: black))\n",
            p.x - HALF,
            (PAGE_H - p.y) - 0.3,
            HALF * 2.0
        ));
        // Vertical bar of the cross.
        // -0.3 centers the 0.6pt-thick bar on the known point.
        src.push_str(&format!(
            "#place(top + left, dx: {}pt, dy: {}pt, rect(width: 0.6pt, height: {}pt, fill: black))\n",
            p.x - 0.3,
            (PAGE_H - p.y) - HALF,
            HALF * 2.0
        ));
    }

    let doc = compile_to_document(&src)?;
    let manifest = recover_regions(&doc)?.with_version(1);
    let pdf = document_to_pdf(&doc)?;
    embed_manifest(&pdf, &manifest)
}
