//! `Section<M>` — wraps a body in an authored `#section("<id>", ...)` call that
//! sets the `inkapp.section` Typst state to `id` and forces a weak page break.
//! A per-page header (see ActionBand) reads that state to know which section it
//! belongs to on any given page.

use crate::component::{Component, RenderCx};
use crate::components::esc_typst_str;
use crate::ink::RegionInk;
use crate::manifest::Manifest;

const SECTION_TYPST: (&str, &str) = (
    "/inkapp/section.typ",
    include_str!("../../typst/section.typ"),
);

pub struct Section<M> {
    id: String,
    body: Vec<Box<dyn Component<Msg = M>>>,
}

impl<M> Section<M> {
    pub fn new(id: impl Into<String>, body: Vec<Box<dyn Component<Msg = M>>>) -> Self {
        Self {
            id: id.into(),
            body,
        }
    }
}

impl<M> Component for Section<M> {
    type Msg = M;

    fn render(&self, cx: &mut RenderCx) -> String {
        let id = esc_typst_str(&self.id);
        let mut body_src = String::new();
        for c in &self.body {
            body_src.push_str(&c.render(cx));
        }
        // Emit a zero-size labelable anchor at the start of the body so an
        // Index entry's `#link(<art-{id}>, ...)` can jump here. Typst label
        // syntax requires the label name in markup source, so we substitute
        // via Rust: `<art-{id}>` attaches to the preceding `#metadata` element
        // (renders nothing and takes no layout space). The label name shape
        // (`art-{id}`) matches what `Index` consumes via `link_id` so the two
        // sides stay in sync. The `art-` prefix is load-bearing: Typst labels
        // forbid leading digits and reader article ids are ULIDs starting
        // with a digit.
        format!("#section(\"{id}\", [#metadata(\"section-anchor\")<art-{id}>\n{body_src}])\n")
    }

    fn typst_sources(&self) -> Vec<(String, String)> {
        let mut out = vec![(SECTION_TYPST.0.into(), SECTION_TYPST.1.into())];
        for c in &self.body {
            out.extend(c.typst_sources());
        }
        out
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<M> {
        let mut out = Vec::new();
        for c in &self.body {
            out.extend(c.decode(ink, manifest));
        }
        out
    }

    fn image_urls(&self) -> Vec<String> {
        let mut out = Vec::new();
        for c in &self.body {
            out.extend(c.image_urls());
        }
        out
    }
}
