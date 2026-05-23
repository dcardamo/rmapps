use crate::ink::RegionInk;
use crate::manifest::Manifest;
use crate::widget::{region_metadata, RenderCx, Widget};

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
}

impl Widget for Checkbox {
    type Output = bool;

    fn render(&self, cx: &mut RenderCx) -> String {
        // Default placement; apps that need control call render_at directly.
        self.render_at(cx.page, 20.0, 40.0, 16.0, 16.0)
    }

    fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> bool {
        let Some(region) = manifest.regions.iter().find(|r| r.name == self.name) else {
            return false;
        };
        ink.iter()
            .filter(|ri| ri.region == self.name)
            .flat_map(|ri| &ri.strokes)
            .flat_map(|s| &s.points)
            .any(|p| region.rect.contains(p.x, p.y))
    }
}
