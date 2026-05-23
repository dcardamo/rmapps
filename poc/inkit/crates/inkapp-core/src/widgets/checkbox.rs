use crate::geometry::PdfPoint;
use crate::ink::RegionInk;
use crate::manifest::Manifest;
use crate::widget::{region_metadata, RenderCx, Widget};

/// How a checkbox region was marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    /// No ink in the region.
    Empty,
    /// A check/tick (short mark).
    Marked,
    /// A dense scribble-out (cancel / un-check).
    ScribbledOut,
}

/// Total ink path length above this multiple of the region diagonal reads as a
/// scribble-out rather than a mark. A tick is ~1–2 diagonals; a scribble many.
const SCRIBBLE_RATIO: f64 = 3.0;

/// A single tappable checkbox bound to a named region.
pub struct Checkbox {
    name: String,
}

impl Checkbox {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }

    /// Render the checkbox glyph and its region at an explicit position
    /// (Typst-space points). Used directly by tests and apps that lay out
    /// absolutely; `render` wraps this with a default box.
    pub fn render_at(&self, page: usize, x: f64, y: f64, w: f64, h: f64) -> String {
        let mut s = region_metadata(&self.name, page, x, y, w, h);
        s.push_str(&format!(
            "#place(top + left, dx: {x}pt, dy: {y}pt, rect(width: {w}pt, height: {h}pt, stroke: 0.5pt))\n"
        ));
        s
    }

    /// Classify the ink attributed to this checkbox's region.
    pub fn read_state(&self, ink: &[RegionInk], manifest: &Manifest) -> CheckState {
        let Some(region) = manifest.regions.iter().find(|r| r.name == self.name) else {
            return CheckState::Empty;
        };
        let strokes: Vec<&crate::ink::Stroke> = ink
            .iter()
            .filter(|ri| ri.region == self.name)
            .flat_map(|ri| &ri.strokes)
            .filter(|s| s.points.iter().any(|p| region.rect.contains(p.x, p.y)))
            .collect();
        if strokes.is_empty() {
            return CheckState::Empty;
        }
        let dx = region.rect.x1 - region.rect.x0;
        let dy = region.rect.y1 - region.rect.y0;
        let diagonal = (dx * dx + dy * dy).sqrt().max(f64::EPSILON);
        let total: f64 = strokes.iter().map(|s| polyline_len(&s.points)).sum();
        if total > SCRIBBLE_RATIO * diagonal {
            CheckState::ScribbledOut
        } else {
            CheckState::Marked
        }
    }
}

/// Sum of segment lengths of a polyline.
fn polyline_len(points: &[PdfPoint]) -> f64 {
    points
        .windows(2)
        .map(|w| {
            let dx = w[1].x - w[0].x;
            let dy = w[1].y - w[0].y;
            (dx * dx + dy * dy).sqrt()
        })
        .sum()
}

impl Widget for Checkbox {
    type Output = bool;

    fn render(&self, cx: &mut RenderCx) -> String {
        self.render_at(cx.page, 20.0, 40.0, 16.0, 16.0)
    }

    fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> bool {
        self.read_state(ink, manifest) != CheckState::Empty
    }
}
