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
    #[must_use]
    pub fn fresh_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

/// Whether a region name is safe to interpolate into Typst markup.
///
/// Region names are embedded into a Typst string literal by [`region_metadata`];
/// a name containing `"`, `)`, `]`, or other markup characters would silently
/// break compilation of the whole document. We constrain names to an identifier
/// alphabet (plus `:` for namespacing, e.g. `box:checkmark:0`) so that failure
/// mode cannot occur. Widget authors mint names from this alphabet.
pub fn is_valid_region_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ':')
}

/// Emit the `#place`d metadata markup that [`crate::manifest::recover_regions`]
/// reads back. Coordinates are Typst-space (top-left origin) points.
///
/// # Panics
/// Panics if `name` is not a valid region name (see [`is_valid_region_name`]).
/// This is a programmer error: names are developer-/widget-chosen, never
/// end-user input, so a constraint violation indicates a bug at the call site.
pub fn region_metadata(name: &str, page: usize, x: f64, y: f64, w: f64, h: f64) -> String {
    assert!(
        is_valid_region_name(name),
        "region name must be non-empty ASCII alphanumeric/_/-/:, got: {name:?}"
    );
    format!(
        "#place(top + left, dx: {x}pt, dy: {y}pt, box(width: {w}pt, height: {h}pt)[#metadata((name: \"{name}\", page: {page}, x: {x}, y: {y}, w: {w}, h: {h})) <region>])\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_name_validation() {
        assert!(is_valid_region_name("done"));
        assert!(is_valid_region_name("tok-3"));
        assert!(is_valid_region_name("habit_streak"));
        assert!(
            is_valid_region_name("box:checkmark:0"),
            "colon namespace separator is allowed"
        );
        assert!(!is_valid_region_name(""), "empty is rejected");
        assert!(!is_valid_region_name("has space"), "space is rejected");
        assert!(!is_valid_region_name("quote\"inside"), "quote is rejected");
        assert!(!is_valid_region_name("paren)inside"), "paren is rejected");
    }

    #[test]
    #[should_panic(expected = "region name must be")]
    fn region_metadata_panics_on_unsafe_name() {
        // A name with a quote would close the Typst string literal and silently
        // break compilation, so it must be rejected loudly at the call site.
        let _ = region_metadata("bad\"name", 0, 0.0, 0.0, 1.0, 1.0);
    }
}
