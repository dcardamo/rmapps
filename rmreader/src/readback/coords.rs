//! Map reMarkable device-space coordinates to PDF points.
//! Device: canvas_w x canvas_h px, origin top-left, X centered on 0, y down.
//! PDF: origin bottom-left, y up. reMarkable fits an imported portrait PDF to the
//! canvas WIDTH; the inverse transform below recovers PDF points from stroke coords.

#[derive(Debug, Clone, Copy)]
pub struct Transform {
    canvas_w: f64,
    /// Stored for future use (e.g. vertical clipping); not used by current transforms.
    #[allow(dead_code)]
    canvas_h: f64,
    page_w: f64,
    page_h: f64,
}

/// Axis-aligned bounding rectangle in PDF points (origin bottom-left, y up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PdfRect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
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
    pub fn new(canvas: (f64, f64), page: (f64, f64)) -> Self {
        Self {
            canvas_w: canvas.0,
            canvas_h: canvas.1,
            page_w: page.0,
            page_h: page.1,
        }
    }

    /// The device-to-PDF scale factor: device pixels per PDF point.
    pub fn scale(&self) -> f64 {
        self.canvas_w / self.page_w
    }

    /// Convert one device point to a PDF point.
    pub fn device_to_pdf(&self, dx: f64, dy: f64) -> (f64, f64) {
        let s = self.scale();
        ((dx + self.canvas_w / 2.0) / s, self.page_h - dy / s)
    }

    /// Axis-aligned PDF bounding box of a set of device points (e.g. a stroke).
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
