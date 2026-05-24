//! The `Component` trait: a nestable unit of view with two halves — a Typst
//! `render` and an ink `decode` that turns the ink on it into app messages.
//! Mirrors `Widget`, but `decode` emits `Msg` values (not a typed read), so a
//! component is what a `view` flow is built from.

use crate::ink::RegionInk;
use crate::manifest::Manifest;
use crate::widget::RenderCx;

/// A view component. `render` emits Typst (declaring `<region>` metadata);
/// `decode` interprets the ink attributed to this component's region(s) into
/// zero or more messages.
pub trait Component {
    /// The application message this component emits.
    type Msg;
    /// Emit Typst markup, including `<region>` metadata for each region.
    fn render(&self, cx: &mut RenderCx) -> String;
    /// Interpret the attributed ink into messages. `ink` is pre-attributed: the
    /// framework has already assigned strokes to regions (via `attribute`) before
    /// calling `decode`. The component filters to its own region name(s).
    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<Self::Msg>;

    /// The Typst source file(s) this component's `render` output `#import`s, as
    /// `(root-absolute virtual path, source text)`. Default: none (the component
    /// builds its Typst inline). Authored components override this to register
    /// their `.typ` render half; the render driver imports each one into `main.typ`.
    fn typst_sources(&self) -> Vec<(String, String)> {
        Vec::new()
    }
}
