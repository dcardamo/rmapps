//! `Passage` — a Capture-mode component: a breakable block of read-only text that
//! captures any ink on it as a single region, regardless of how it paginates. It
//! carries the value-message to emit when inked (Elm's value-message, no stored
//! closure), so it drops into any `view` flow. It is the component that exercises a
//! region split across a page break (the framework stitches per-page ink into one
//! RegionInk before `decode`).

use crate::component::{Component, RenderCx};
use crate::components::esc_typst_str;
use crate::ink::RegionInk;
use crate::manifest::Manifest;
use crate::render::is_valid_region_name;

/// A breakable text passage bound to one named region, carrying `on_capture` to
/// emit when any ink lands on it. `M` defaults to `()` for a presence-only passage.
pub struct Passage<M = ()> {
    name: String,
    lines: Vec<String>,
    on_capture: M,
}

impl Passage<()> {
    /// A presence-only passage (no message).
    pub fn new(name: &str, lines: &[&str]) -> Self {
        Self {
            name: name.to_string(),
            lines: lines.iter().map(|s| s.to_string()).collect(),
            on_capture: (),
        }
    }
}

impl<M> Passage<M> {
    /// A passage carrying `on_capture` to emit when inked.
    pub fn with_msg(name: &str, lines: &[&str], on_capture: M) -> Self {
        Self {
            name: name.to_string(),
            lines: lines.iter().map(|s| s.to_string()).collect(),
            on_capture,
        }
    }

    /// Whether any ink landed in this passage's (stitched) region.
    ///
    /// Unlike `Checkbox`, this does not re-check point-in-rect against the manifest:
    /// `attribute` already does per-frame containment when building each `RegionInk`
    /// and stitches a split region's frames into one `RegionInk`. A Checkbox-style
    /// `find(name)` would inspect only the first frame's rect, which is wrong for a
    /// breakable region — so presence of any attributed, stitched ink is the correct,
    /// split-safe signal. `_manifest` is therefore unused.
    pub fn read(&self, ink: &[RegionInk], _manifest: &Manifest) -> bool {
        ink.iter()
            .filter(|ri| ri.region == self.name)
            .any(|ri| !ri.strokes.is_empty())
    }
}

impl<M: Clone> Component for Passage<M> {
    type Msg = M;

    fn render(&self, _cx: &mut RenderCx) -> String {
        assert!(
            is_valid_region_name(&self.name),
            "passage region name must be a valid region name, got: {:?}",
            self.name
        );
        let name = &self.name;
        // Each line is injected as a Typst *string expression* (`#"..."`) so its
        // markup chars stay literal; lines are separated by linebreaks so the body
        // is a single flowing (breakable) block.
        let body: String = self
            .lines
            .iter()
            .map(|l| format!("#\"{}\"", esc_typst_str(l)))
            .collect::<Vec<_>>()
            .join(" #linebreak() ");
        format!("#region(\"{name}\", [{body}], breakable: true)\n")
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<M> {
        if self.read(ink, manifest) {
            vec![self.on_capture.clone()]
        } else {
            vec![]
        }
    }
}
