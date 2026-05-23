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

#[cfg(test)]
mod tests {
    use super::*;

    const R: PdfRect = PdfRect {
        x0: 10.0,
        y0: 10.0,
        x1: 30.0,
        y1: 30.0,
    };

    #[test]
    fn contains_interior_and_boundary_inclusive() {
        assert!(R.contains(20.0, 20.0), "interior point");
        assert!(R.contains(10.0, 10.0), "bottom-left corner is inclusive");
        assert!(R.contains(30.0, 30.0), "top-right corner is inclusive");
        assert!(R.contains(10.0, 20.0), "left edge is inclusive");
    }

    #[test]
    fn contains_rejects_outside() {
        assert!(!R.contains(9.999, 20.0), "just left of x0");
        assert!(!R.contains(30.001, 20.0), "just right of x1");
        assert!(!R.contains(20.0, 9.999), "just below y0");
        assert!(!R.contains(20.0, 30.001), "just above y1");
    }

    #[test]
    fn overlaps_intersecting_touching_and_disjoint() {
        // Clearly intersecting.
        assert!(R.overlaps(&PdfRect {
            x0: 20.0,
            y0: 20.0,
            x1: 40.0,
            y1: 40.0
        }));
        // Sharing only a corner counts as overlap (inclusive bounds).
        assert!(R.overlaps(&PdfRect {
            x0: 30.0,
            y0: 30.0,
            x1: 50.0,
            y1: 50.0
        }));
        // Fully separate on x.
        assert!(!R.overlaps(&PdfRect {
            x0: 31.0,
            y0: 10.0,
            x1: 40.0,
            y1: 30.0
        }));
        // Fully separate on y.
        assert!(!R.overlaps(&PdfRect {
            x0: 10.0,
            y0: 31.0,
            x1: 30.0,
            y1: 40.0
        }));
    }
}
