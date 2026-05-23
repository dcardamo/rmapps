use crate::geometry::PdfPoint;

/// A device-agnostic ink stroke in PDF-point coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub points: Vec<PdfPoint>,
    /// True if drawn with a highlighter tool (the only distinction widgets need).
    pub highlighter: bool,
}

impl Stroke {
    /// Axis-aligned bounding box `(x0, y0, x1, y1)` of the stroke, or `None` if empty.
    pub fn bbox(&self) -> Option<(f64, f64, f64, f64)> {
        let mut it = self.points.iter();
        let first = it.next()?;
        let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x, first.y);
        for p in it {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        Some((x0, y0, x1, y1))
    }
}

/// Strokes attributed to one named region.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionInk {
    pub region: String,
    pub strokes: Vec<Stroke>,
}
