//! Map reMarkable device-space coordinates to PDF points.
//!
//! Device space: the v6 logical canvas, origin top-left, **X centered on 0**, y down.
//! PDF space: origin bottom-left, y up.
//!
//! Empirically (verified against a real Paper Pro Move capture), reMarkable renders
//! an imported PDF at its **native 226 dpi (1:1 points), centered horizontally and
//! top-aligned** — it does NOT scale the page to fit the canvas width. So a device
//! unit is a fixed `72/226` PDF points regardless of page or canvas size, the PDF's
//! horizontal centre sits at device x = 0, and the PDF's top sits at device y = 0.
//! (This matches the constant used by rmc.)

/// reMarkable v6 device resolution (dots per inch) for the logical canvas.
const DEVICE_DPI: f64 = 226.0;
/// PDF points per device unit.
const PT_PER_DEVICE: f64 = 72.0 / DEVICE_DPI;

/// Coordinate transform from reMarkable device space to PDF points.
///
/// Constructed from the source PDF page size; see [`Transform::new`].
#[derive(Debug, Clone, Copy)]
pub struct Transform {
    page_w: f64,
    page_h: f64,
}

/// Axis-aligned bounding rectangle in PDF points (origin bottom-left, y up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PdfRect {
    /// Left edge in PDF points.
    pub x0: f64,
    /// Bottom edge in PDF points.
    pub y0: f64,
    /// Right edge in PDF points.
    pub x1: f64,
    /// Top edge in PDF points.
    pub y1: f64,
}

impl PdfRect {
    /// True when the two rectangles overlap (touch counts as overlap).
    pub fn intersects(&self, other: &PdfRect) -> bool {
        self.x0 <= other.x1 && self.x1 >= other.x0 && self.y0 <= other.y1 && self.y1 >= other.y0
    }

    /// Shared area of the two rectangles; 0.0 if disjoint or merely touching.
    pub fn overlap_area(&self, o: &PdfRect) -> f64 {
        let w = (self.x1.min(o.x1) - self.x0.max(o.x0)).max(0.0);
        let h = (self.y1.min(o.y1) - self.y0.max(o.y0)).max(0.0);
        w * h
    }
}

impl Transform {
    /// Create a transform for a PDF page of the given size in points.
    ///
    /// `page` is the source PDF page size in points `(width, height)`. The device
    /// canvas size is not needed: the PDF is rendered at native dpi (see module
    /// docs), so the mapping depends only on the page size and the fixed device dpi.
    pub fn new(page: (f64, f64)) -> Self {
        Self {
            page_w: page.0,
            page_h: page.1,
        }
    }

    /// Device units per PDF point (e.g. for converting a stroke's ink width to
    /// points). The inverse of `PT_PER_DEVICE`.
    pub fn scale(&self) -> f64 {
        DEVICE_DPI / 72.0
    }

    /// Convert one device point to a PDF point.
    ///
    /// The PDF is centered horizontally (device x = 0 is the page's horizontal
    /// centre) and top-aligned (device y = 0 is the page top), rendered at native
    /// dpi. Returns `(pdf_x, pdf_y)`.
    pub fn device_to_pdf(&self, dx: f64, dy: f64) -> (f64, f64) {
        (
            dx * PT_PER_DEVICE + self.page_w / 2.0,
            self.page_h - dy * PT_PER_DEVICE,
        )
    }

    /// Axis-aligned PDF bounding box of a set of device points (e.g. a stroke).
    ///
    /// Returns `None` if `device_pts` is empty.
    pub fn pdf_bbox<I: IntoIterator<Item = (f64, f64)>>(&self, device_pts: I) -> Option<PdfRect> {
        let mut it = device_pts
            .into_iter()
            .map(|(x, y)| self.device_to_pdf(x, y));
        let (x, y) = it.next()?;
        let (mut x0, mut y0, mut x1, mut y1) = (x, y, x, y);
        for (x, y) in it {
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
        }
        Some(PdfRect { x0, y0, x1, y1 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_origin_maps_to_top_centre() {
        let t = Transform::new((300.0, 540.0));
        let (x, y) = t.device_to_pdf(0.0, 0.0);
        assert!((x - 150.0).abs() < 1e-9, "x={x}");
        assert!((y - 540.0).abs() < 1e-9, "y={y}");
    }

    #[test]
    fn one_device_unit_is_72_over_226_points() {
        let t = Transform::new((300.0, 540.0));
        let (x, _) = t.device_to_pdf(226.0, 0.0);
        assert!((x - (150.0 + 72.0)).abs() < 1e-6, "x={x}");
    }

    #[test]
    fn pdfrect_intersect_and_overlap() {
        let a = PdfRect {
            x0: 0.0,
            y0: 0.0,
            x1: 10.0,
            y1: 10.0,
        };
        let b = PdfRect {
            x0: 5.0,
            y0: 5.0,
            x1: 15.0,
            y1: 15.0,
        };
        assert!(a.intersects(&b));
        assert!((a.overlap_area(&b) - 25.0).abs() < 1e-9);
    }

    #[test]
    fn pdf_bbox_empty_is_none() {
        let t = Transform::new((300.0, 540.0));
        assert!(t.pdf_bbox(std::iter::empty::<(f64, f64)>()).is_none());
    }

    #[test]
    fn pdf_bbox_two_points_gives_correct_bounds() {
        let t = Transform::new((300.0, 540.0));
        // 226 device units = 72 pt. (-226,0) -> (150-72, 540) = (78, 540);
        // (226,226) -> (150+72, 540-72) = (222, 468). So bbox x 78..222, y 468..540.
        let bbox = t.pdf_bbox([(-226.0f64, 0.0), (226.0, 226.0)]).unwrap();
        assert!((bbox.x0 - 78.0).abs() < 1e-6, "x0={}", bbox.x0);
        assert!((bbox.x1 - 222.0).abs() < 1e-6, "x1={}", bbox.x1);
        assert!((bbox.y0 - 468.0).abs() < 1e-6, "y0={}", bbox.y0);
        assert!((bbox.y1 - 540.0).abs() < 1e-6, "y1={}", bbox.y1);
    }
}
