//! `ActionBand<M>` — a per-page header of N labelled cells. Each cell carries
//! a `Fn(section_id: &str) -> M` closure; on decode, a non-highlighter pen
//! stroke that spans most of a cell's width fires that cell's closure with
//! the section id parsed from the region name (`action-{label}-{section_id}`).
//!
//! The closure is the appdx-documented escape hatch: a reusable content
//! component whose message depends on both *which cell was struck* (label) and
//! *which section the page belonged to* (section id) — both content-derived.

use crate::component::{Component, RenderCx};
use crate::components::esc_typst_str;
use crate::ink::RegionInk;
use crate::manifest::Manifest;

/// Strike width as a fraction of the cell's width. A non-highlighter stroke
/// must span at least this much of the cell's width in X to count.
const STRIKE_WIDTH_RATIO: f64 = 0.5;

type Handler<M> = Box<dyn Fn(&str) -> M + Send + Sync>;

const ACTION_BAND_TYPST: (&str, &str) = (
    "/inkapp/action_band.typ",
    include_str!("../../typst/action_band.typ"),
);

pub struct ActionBand<M> {
    cells: Vec<(String, Handler<M>)>,
}

impl<M> ActionBand<M> {
    pub fn new(cells: impl IntoIterator<Item = (String, Handler<M>)>) -> Self {
        Self {
            cells: cells.into_iter().collect(),
        }
    }

    /// The labels, in order — used for the Typst call.
    fn labels(&self) -> Vec<&str> {
        self.cells.iter().map(|(l, _)| l.as_str()).collect()
    }
}

impl<M> Component for ActionBand<M> {
    type Msg = M;

    fn render(&self, _cx: &mut RenderCx) -> String {
        let labels = self
            .labels()
            .iter()
            .map(|l| format!("\"{}\"", esc_typst_str(l)))
            .collect::<Vec<_>>()
            .join(", ");
        // Trailing comma inside the parens ensures a single-element array is
        // parsed as an array (not a parenthesised expression) by Typst.
        format!("#action-band(({labels}, ))\n")
    }

    fn typst_sources(&self) -> Vec<(String, String)> {
        // The action_band module imports section.typ; both must be registered so
        // the Typst world can resolve the import.
        vec![
            (ACTION_BAND_TYPST.0.into(), ACTION_BAND_TYPST.1.into()),
            (
                "/inkapp/section.typ".into(),
                include_str!("../../typst/section.typ").into(),
            ),
        ]
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<M> {
        let mut out = Vec::new();
        for ri in ink {
            // Region name shape: "action-{label}-{section_id}".
            let Some(rest) = ri.region.strip_prefix("action-") else {
                continue;
            };
            // Split into (label, section_id) by matching against known labels.
            // Labels can contain any chars except we use a trailing '-' as the
            // separator, so we try `rest.strip_prefix("{label}-")` for each.
            let Some((label, section_id)) = self.cells.iter().find_map(|(lbl, _)| {
                rest.strip_prefix(lbl.as_str())
                    .and_then(|s| s.strip_prefix('-'))
                    .map(|sid| (lbl.as_str(), sid))
            }) else {
                continue;
            };

            // Find this region's rect in the manifest to know the cell's width.
            let Some(region) = manifest.regions.iter().find(|r| r.name == ri.region) else {
                continue;
            };
            let cell_w = region.rect.x1 - region.rect.x0;

            // Classify: any non-highlighter stroke spanning ≥ STRIKE_WIDTH_RATIO
            // of cell width fires the action.
            let fires = ri.strokes.iter().any(|s| {
                if s.highlighter {
                    return false;
                }
                let mut min_x = f64::INFINITY;
                let mut max_x = f64::NEG_INFINITY;
                for p in &s.points {
                    if p.x < min_x {
                        min_x = p.x;
                    }
                    if p.x > max_x {
                        max_x = p.x;
                    }
                }
                if min_x > max_x {
                    return false; // zero-point stroke
                }
                (max_x - min_x) >= STRIKE_WIDTH_RATIO * cell_w
            });

            if fires {
                if let Some((_, handler)) = self.cells.iter().find(|(l, _)| l == label) {
                    out.push(handler(section_id));
                }
            }
        }
        out
    }
}
