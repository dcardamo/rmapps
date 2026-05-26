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
        // The body is wrapped in `[...]` so Typst treats it as content.
        format!("#section(\"{id}\", [\n{body_src}])\n")
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
