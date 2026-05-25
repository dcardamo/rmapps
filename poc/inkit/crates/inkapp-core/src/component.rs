//! The `Component` trait: a nestable unit of view with two halves — a Typst
//! `render` and an ink `decode` that turns the ink on it into app messages.
//! Render and decode are co-located; `decode` emits `Msg` values, so a
//! component is what a `view` flow is built from.

use crate::ink::RegionInk;
use crate::manifest::Manifest;

/// Render-time context: supplies the current page index and a monotonically
/// increasing id so components can mint unique region names if needed.
#[derive(Debug, Default)]
pub struct RenderCx {
    pub page: usize,
    next_id: u64,
}

impl RenderCx {
    pub fn new(page: usize) -> Self {
        Self { page, next_id: 0 }
    }

    /// Mint a fresh per-render id (used by components that subdivide into
    /// programmatically-named regions).
    #[must_use]
    pub fn fresh_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

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

    /// Stable, props-derived key under which this component's state is carried in
    /// the sealed manifest. `None` (default) = stateless. Derive from identity
    /// props (e.g. a name/id), never from volatile content, so the key is
    /// identical at render time and at the next cycle's pre-fold decode.
    fn state_key(&self) -> Option<String> {
        None
    }

    /// The state to seal at render time — the base the document is rendered with.
    /// `None` (default) = nothing carried.
    fn render_state(&self) -> Option<serde_json::Value> {
        None
    }

    /// URLs whose images this component's `render` references via
    /// `#image("/assets/{asset_key(url)}.png")`. The framework collects these,
    /// resolves them through the image pipeline (fetch + normalize + cache, with
    /// a placeholder on failure), and registers the bytes before compiling — so
    /// the emitted `#image` always resolves. Default: none.
    fn image_urls(&self) -> Vec<String> {
        Vec::new()
    }
}
