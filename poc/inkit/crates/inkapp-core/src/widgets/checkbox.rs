use crate::component::Component;
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

/// A single tappable checkbox bound to a named region, carrying the message to
/// emit when marked (Elm's value-message; no stored closure). `M` defaults to
/// `()` so a presence-only `Checkbox::new(name)` keeps working.
pub struct Checkbox<M = ()> {
    name: String,
    label: String,
    on_check: M,
}

impl Checkbox<()> {
    /// A presence-only checkbox (no message). Back-compatible constructor.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            label: String::new(),
            on_check: (),
        }
    }
}

impl<M> Checkbox<M> {
    /// A checkbox that carries `on_check` to emit when marked.
    pub fn with_msg(name: &str, on_check: M) -> Self {
        Self {
            name: name.to_string(),
            label: String::new(),
            on_check,
        }
    }

    /// Set the visible label (builder).
    #[must_use]
    pub fn label(mut self, label: &str) -> Self {
        self.label = label.to_string();
        self
    }

    /// Render the checkbox glyph and its region at an explicit position
    /// (Typst-space points). Used by tests/apps that lay out absolutely.
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
        // Two stages: first the strokes bucketed to this region by name, then only
        // those with a point actually inside the rect.
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

impl<M> Widget for Checkbox<M> {
    type Output = bool;

    fn render(&self, cx: &mut RenderCx) -> String {
        self.render_at(cx.page, 20.0, 40.0, 16.0, 16.0)
    }

    fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> bool {
        self.read_state(ink, manifest) != CheckState::Empty
    }
}

impl<M: Clone> Component for Checkbox<M> {
    type Msg = M;

    /// Inline render: an in-flow box whose region rect is recovered from layout
    /// (via `here().position()`), so it composes after flowing content in a
    /// document. The page index comes from Typst introspection.
    fn render(&self, _cx: &mut RenderCx) -> String {
        let name = &self.name;
        let label = self.label.replace('\\', "\\\\").replace('"', "\\\"");
        format!(
            "#box[#context [#metadata((name: \"{name}\", \
               page: here().position().page - 1, x: here().position().x / 1pt, \
               y: here().position().y / 1pt, w: 14, h: 14)) <region>]\
             #rect(width: 14pt, height: 14pt, stroke: 0.5pt)] #text[{label}]\n"
        )
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<M> {
        if self.read_state(ink, manifest) != CheckState::Empty {
            vec![self.on_check.clone()]
        } else {
            vec![]
        }
    }
}
