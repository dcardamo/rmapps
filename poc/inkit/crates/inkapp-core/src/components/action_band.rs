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
use crate::components::gesture::{strike_spans_region, STRIKE_WIDTH_RATIO};
use crate::ink::RegionInk;
use crate::manifest::Manifest;

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
        let cells: Vec<_> = cells.into_iter().collect();
        for (label, _) in &cells {
            assert!(
                !label.is_empty(),
                "ActionBand cell label must not be empty"
            );
            assert!(
                !label.contains('-'),
                "ActionBand cell label must not contain '-' (label: {label:?})"
            );
        }
        Self { cells }
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
            // Labels are guaranteed (by new()) to contain no '-', so the first
            // '-' after the prefix is unambiguously the label/section_id split.
            let Some(rest) = ri.region.strip_prefix("action-") else {
                continue;
            };
            let Some((label, section_id)) = rest.split_once('-') else {
                continue;
            };

            // Only handle labels this band owns.
            let Some((_, handler)) = self.cells.iter().find(|(l, _)| l == label) else {
                continue;
            };

            // Find this region's rect in the manifest to know the cell's width.
            let Some(region) = manifest.regions.iter().find(|r| r.name == ri.region) else {
                continue;
            };

            // Classify using the shared bbox-union helper — mirrors GestureAction.
            if strike_spans_region(&ri.strokes, region, STRIKE_WIDTH_RATIO) {
                out.push(handler(section_id));
            }
        }
        out
    }
}
