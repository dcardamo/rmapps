use crate::geometry::{PdfPoint, PdfRect};

/// A device-agnostic ink stroke in PDF-point coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub points: Vec<PdfPoint>,
    /// True if drawn with a highlighter tool (the only distinction components need).
    pub highlighter: bool,
}

impl Stroke {
    /// Axis-aligned bounding box of the stroke as a [`PdfRect`], or `None` if the
    /// stroke has no points. Returning a `PdfRect` lets callers use
    /// [`PdfRect::contains`]/[`PdfRect::overlaps`] directly for hit-testing.
    pub fn bbox(&self) -> Option<PdfRect> {
        let mut it = self.points.iter();
        let first = it.next()?;
        let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x, first.y);
        for p in it {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        Some(PdfRect { x0, y0, x1, y1 })
    }
}

/// Strokes attributed to one named region.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionInk {
    pub region: String,
    pub strokes: Vec<Stroke>,
}
