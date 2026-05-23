use crate::ink::RegionInk;
use crate::manifest::Manifest;

/// A widget renders Typst markup that declares named regions, and interprets the
/// ink attributed to those regions. Render and readback co-located.
pub trait Widget {
    type Output;
    /// Emit Typst markup (including `<region>` metadata for each region).
    fn render(&self, cx: &mut RenderCx) -> String;
    /// Interpret the strokes attributed to this widget's region(s).
    fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> Self::Output;
}

/// Render-time context: supplies the current page index and a monotonically
/// increasing id so widgets can mint unique region names if needed.
#[derive(Debug, Default)]
pub struct RenderCx {
    pub page: usize,
    next_id: u64,
}

impl RenderCx {
    pub fn new(page: usize) -> Self {
        Self { page, next_id: 0 }
    }

    // Used by widgets that mint unique region names (Task 9+).
    pub fn fresh_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

/// Emit the `#place`d metadata markup that [`crate::manifest::recover_regions`]
/// reads back. Coordinates are Typst-space (top-left origin) points.
pub fn region_metadata(name: &str, page: usize, x: f64, y: f64, w: f64, h: f64) -> String {
    format!(
        "#place(top + left, dx: {x}pt, dy: {y}pt, box(width: {w}pt, height: {h}pt)[#metadata((name: \"{name}\", page: {page}, x: {x}, y: {y}, w: {w}, h: {h})) <region>])\n"
    )
}
