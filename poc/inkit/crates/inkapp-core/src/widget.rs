use crate::component::RenderCx;
use crate::ink::RegionInk;
use crate::manifest::Manifest;

/// A widget renders Typst markup that declares named regions, and interprets the
/// ink attributed to those regions. Render and readback co-located.
///
/// NOTE: obsolete — being removed in favor of `Component`. Do not add new impls.
pub trait Widget {
    type Output;
    /// Emit Typst markup (including `<region>` metadata for each region).
    fn render(&self, cx: &mut RenderCx) -> String;
    /// Interpret the strokes attributed to this widget's region(s).
    fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> Self::Output;
}
