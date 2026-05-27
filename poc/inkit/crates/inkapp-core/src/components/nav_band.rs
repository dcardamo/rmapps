//! `NavBand<M>` — a three-cell page-header strip: `< Prev | Home | Next >`.
//! Renders as PDF link annotations that jump between Sections in document
//! order. Reusable on any multi-section reading app.
//!
//! Pair with `Section<M>` (whose start emits `<art-{id}>` labels) and
//! `Index<M>` (which emits an `<index-home>` label at its first row). Attach
//! to `Document::page_header` either alone or stacked above an `ActionBand`
//! via `Stack<M>`. Decode is a no-op — navigation never produces messages;
//! the device's PDF viewer handles taps on link annotations.

use std::marker::PhantomData;

use crate::component::{Component, RenderCx};
use crate::components::esc_typst_str;
use crate::ink::RegionInk;
use crate::manifest::Manifest;

const NAV_BAND_TYPST: (&str, &str) = (
    "/inkapp/nav_band.typ",
    include_str!("../../typst/nav_band.typ"),
);

pub struct NavBand<M = ()> {
    order: Vec<String>,
    _msg: PhantomData<fn() -> M>,
}

impl<M> NavBand<M> {
    /// Build a NavBand over `order` — the ordered list of section ids the
    /// host document's `Section`s use. The current article's id (from the
    /// `inkapp.section` Typst state) is resolved to a position in this list
    /// at render time so the Prev/Next cells link to the neighbors.
    pub fn new(order: Vec<String>) -> Self {
        Self {
            order,
            _msg: PhantomData,
        }
    }
}

impl<M> Component for NavBand<M> {
    type Msg = M;

    fn render(&self, _cx: &mut RenderCx) -> String {
        let order = self
            .order
            .iter()
            .map(|s| format!("\"{}\"", esc_typst_str(s)))
            .collect::<Vec<_>>()
            .join(", ");
        // Trailing comma keeps single-element arrays from being parsed as
        // grouping parens — same guard the ActionBand uses.
        format!("#nav-band(({order}, ))\n")
    }

    fn typst_sources(&self) -> Vec<(String, String)> {
        // NavBand imports section.typ for the section-state handle.
        vec![
            (NAV_BAND_TYPST.0.into(), NAV_BAND_TYPST.1.into()),
            (
                "/inkapp/section.typ".into(),
                include_str!("../../typst/section.typ").into(),
            ),
        ]
    }

    fn decode(&self, _ink: &[RegionInk], _manifest: &Manifest) -> Vec<M> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_bakes_ordered_ids() {
        let n: NavBand<()> = NavBand::new(vec!["a".into(), "b".into(), "c".into()]);
        let out = n.render(&mut RenderCx::new(0));
        assert!(out.contains("\"a\""));
        assert!(out.contains("\"b\""));
        assert!(out.contains("\"c\""));
        assert!(out.contains("#nav-band("));
    }

    #[test]
    fn typst_sources_include_section_dep() {
        let n: NavBand<()> = NavBand::new(vec![]);
        let paths: Vec<String> = n.typst_sources().into_iter().map(|(p, _)| p).collect();
        assert!(paths.contains(&"/inkapp/nav_band.typ".to_string()));
        assert!(paths.contains(&"/inkapp/section.typ".to_string()));
    }

    #[test]
    fn decode_is_empty() {
        let n: NavBand<()> = NavBand::new(vec!["a".into()]);
        assert!(n.decode(&[], &Manifest::default()).is_empty());
    }

    #[test]
    fn ids_are_escaped() {
        let n: NavBand<()> = NavBand::new(vec![r#"weird"id"#.into()]);
        let out = n.render(&mut RenderCx::new(0));
        assert!(
            out.contains(r#"weird\"id"#),
            "quote in id escaped: {out}"
        );
    }
}
