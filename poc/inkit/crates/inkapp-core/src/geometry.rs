use serde::{Deserialize, Serialize};

/// A rectangle in PDF user space (bottom-left origin, y up), in points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PdfRect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl PdfRect {
    /// Whether a point (PDF space) lies within this rect (inclusive).
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }

    /// Whether this rect overlaps `other`.
    pub fn overlaps(&self, other: &PdfRect) -> bool {
        self.x0 <= other.x1 && self.x1 >= other.x0 && self.y0 <= other.y1 && self.y1 >= other.y0
    }
}

/// A point in PDF user space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PdfPoint {
    pub x: f64,
    pub y: f64,
}

/// A point in a device's native ink space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DevicePoint {
    pub x: f64,
    pub y: f64,
}

/// Convert a Typst top-left-origin rect to a PDF bottom-left-origin rect using
/// the height of the rect's own page.
pub fn typst_to_pdf_rect(x: f64, y: f64, w: f64, h: f64, page_height_pt: f64) -> PdfRect {
    PdfRect {
        x0: x,
        y0: page_height_pt - (y + h),
        x1: x + w,
        y1: page_height_pt - y,
    }
}
