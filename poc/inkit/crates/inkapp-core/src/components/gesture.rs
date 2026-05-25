//! `GestureAction` — a Control component that fires a value-message when a
//! *striking pen gesture* lands on its region. It renders its target content as a
//! single region and decodes a non-highlighter stroke whose combined bounding box
//! spans most of the region's width (a horizontal strike or scribble) into one
//! message — while ignoring incidental marks, taps, and highlighter swipes. It
//! carries the message as a value (Elm's value-message, no stored closure), like
//! `Checkbox`/`Passage`. This ports the old `rmreader` `classify.rs` intent
//! (geometry → action) as a clean, reusable component.

use crate::component::{Component, RenderCx};
use crate::components::esc_typst_str;
use crate::ink::RegionInk;
use crate::manifest::Manifest;
use crate::render::is_valid_region_name;

/// A non-highlighter gesture whose combined bbox spans at least this fraction of
/// the region width reads as a deliberate strike/scribble (the action) rather than
/// an incidental mark. A strike/scribble fills the line; a tick or dot does not.
const STRIKE_WIDTH_RATIO: f64 = 0.6;

/// A Control bound to one named region that fires `on_gesture` when struck through.
/// `M` defaults to `()` for a presence-only control.
pub struct GestureAction<M = ()> {
    name: String,
    content: String,
    on_gesture: M,
}

impl GestureAction<()> {
    /// A presence-only gesture action (no message).
    pub fn new(name: &str, content: &str) -> Self {
        Self {
            name: name.to_string(),
            content: content.to_string(),
            on_gesture: (),
        }
    }
}

impl<M> GestureAction<M> {
    /// A gesture action carrying `on_gesture` to emit when struck.
    pub fn with_msg(name: &str, content: &str, on_gesture: M) -> Self {
        Self {
            name: name.to_string(),
            content: content.to_string(),
            on_gesture,
        }
    }

    /// Whether a striking pen gesture landed on this control's region: a
    /// non-highlighter stroke (or strokes) whose combined bounding box spans at
    /// least `STRIKE_WIDTH_RATIO` of the region width. Highlighter strokes are
    /// excluded, so a highlight never triggers the action.
    pub fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> bool {
        let Some(region) = manifest.regions.iter().find(|r| r.name == self.name) else {
            return false;
        };
        let region_w = region.rect.x1 - region.rect.x0;
        if region_w <= 0.0 {
            return false;
        }
        // Non-highlighter strokes attributed to this region with a point inside
        // the rect (the Checkbox two-stage filter); union their bounding boxes so
        // a multi-stroke scribble is handled as a single gesture.
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        for bbox in ink
            .iter()
            .filter(|ri| ri.region == self.name)
            .flat_map(|ri| &ri.strokes)
            .filter(|s| !s.highlighter)
            .filter(|s| s.points.iter().any(|p| region.rect.contains(p.x, p.y)))
            .filter_map(|s| s.bbox())
        {
            min_x = min_x.min(bbox.x0);
            max_x = max_x.max(bbox.x1);
        }
        if min_x > max_x {
            return false; // no qualifying pen strokes
        }
        (max_x - min_x) >= STRIKE_WIDTH_RATIO * region_w
    }
}

impl<M: Clone> Component for GestureAction<M> {
    type Msg = M;

    fn render(&self, _cx: &mut RenderCx) -> String {
        assert!(
            is_valid_region_name(&self.name),
            "gesture-action region name must be a valid region name, got: {:?}",
            self.name
        );
        let name = &self.name;
        let content = esc_typst_str(&self.content);
        // A non-breakable region: the default `#region` wraps the body in a box,
        // so recovery yields one rect whose width is the laid-out content width —
        // the span a strike must cover. The content is injected as a Typst string
        // expression (`#"..."`) so its markup chars stay literal.
        format!("#region(\"{name}\", [#\"{content}\"])\n")
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<M> {
        if self.read(ink, manifest) {
            vec![self.on_gesture.clone()]
        } else {
            vec![]
        }
    }
}
