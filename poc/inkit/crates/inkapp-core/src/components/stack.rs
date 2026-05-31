//! `Stack<M>` — a passthrough container that renders its children in order, with
//! no page break, no state, no region. Useful for composing multiple components
//! into one slot (e.g. a `Document::page_header` that wants both a `NavBand`
//! above an `ActionBand`). Section adds a state update + pagebreak; Stack adds
//! neither. Generic over `M = ()` so it slots cleanly under any app message.

use std::marker::PhantomData;

use crate::component::{Component, RenderCx};
use crate::ink::RegionInk;
use crate::manifest::Manifest;

pub struct Stack<M = ()> {
    body: Vec<Box<dyn Component<Msg = M>>>,
    _msg: PhantomData<fn() -> M>,
}

impl<M> Stack<M> {
    pub fn new(body: Vec<Box<dyn Component<Msg = M>>>) -> Self {
        Self {
            body,
            _msg: PhantomData,
        }
    }
}

impl<M> Component for Stack<M> {
    type Msg = M;

    fn render(&self, cx: &mut RenderCx) -> String {
        let mut out = String::new();
        for c in &self.body {
            out.push_str(&c.render(cx));
        }
        out
    }

    fn typst_sources(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::gesture::GestureAction;
    use crate::components::notice::Notice;
    use crate::document::Document;
    use crate::flow;
    use crate::geometry::{PageGeom, PdfPoint};
    use crate::ink::Stroke;
    use crate::manifest::{recover_regions, Region};
    use crate::readback::attribute_page;
    use crate::runtime::compile_document_in;
    use crate::Theme;

    // Mock used for aggregation tests so they don't couple to a real
    // component's current behaviour.
    struct Mock {
        sources: Vec<(String, String)>,
        images: Vec<String>,
    }
    impl Component for Mock {
        type Msg = ();
        fn render(&self, _cx: &mut RenderCx) -> String {
            String::new()
        }
        fn typst_sources(&self) -> Vec<(String, String)> {
            self.sources.clone()
        }
        fn decode(&self, _ink: &[RegionInk], _m: &Manifest) -> Vec<()> {
            Vec::new()
        }
        fn image_urls(&self) -> Vec<String> {
            self.images.clone()
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    enum M {
        Hit,
    }

    #[test]
    fn render_concatenates_children() {
        let s: Stack<()> = Stack::new(vec![
            Box::new(Notice::line("first")),
            Box::new(Notice::line("second")),
        ]);
        let out = s.render(&mut RenderCx::new(0));
        let p1 = out.find("first").expect("first present");
        let p2 = out.find("second").expect("second present");
        assert!(p1 < p2, "children render in declaration order: {out}");
    }

    #[test]
    fn decode_delegates_to_children() {
        let s: Stack<()> = Stack::new(vec![Box::new(Notice::line("x"))]);
        let manifest = Manifest::default();
        let msgs = s.decode(&[], &manifest);
        assert!(msgs.is_empty());
    }

    #[test]
    fn decode_forwards_messages_from_children() {
        let doc: Document<M> = Document::keyed(
            "d",
            flow![Stack::new(vec![Box::new(GestureAction::with_msg(
                "title",
                "How CGI changed the web",
                M::Hit
            ))])],
        );
        let compiled = compile_document_in(&doc, PageGeom::default(), &Theme::reader()).unwrap();
        let manifest = recover_regions(&compiled).unwrap();
        let r: &Region = manifest
            .regions
            .iter()
            .find(|r| r.name == "title")
            .expect("title region recovered through Stack");
        let cy = (r.rect.y0 + r.rect.y1) / 2.0;
        let stroke = Stroke {
            points: vec![
                PdfPoint {
                    x: r.rect.x0,
                    y: cy,
                },
                PdfPoint {
                    x: r.rect.x1,
                    y: cy,
                },
            ],
            highlighter: false,
        };
        let ink = attribute_page(&[stroke], &manifest);
        let decoded = doc.flow[0].decode(&ink, &manifest);
        assert_eq!(decoded, vec![M::Hit], "Stack forwards child's Msg");
    }

    #[test]
    fn typst_sources_aggregates_across_children() {
        let s: Stack<()> = Stack::new(vec![
            Box::new(Mock {
                sources: vec![("a.typ".into(), "let a = 1".into())],
                images: vec![],
            }),
            Box::new(Mock {
                sources: vec![
                    ("b.typ".into(), "let b = 2".into()),
                    ("c.typ".into(), "let c = 3".into()),
                ],
                images: vec![],
            }),
        ]);
        let out = s.typst_sources();
        assert_eq!(out.len(), 3, "all three sources collected");
        assert_eq!(out[0].0, "a.typ");
        assert_eq!(out[1].0, "b.typ");
        assert_eq!(out[2].0, "c.typ");
    }

    #[test]
    fn image_urls_aggregates_across_children() {
        let s: Stack<()> = Stack::new(vec![
            Box::new(Mock {
                sources: vec![],
                images: vec!["https://example.com/1.png".into()],
            }),
            Box::new(Mock {
                sources: vec![],
                images: vec![
                    "https://example.com/2.png".into(),
                    "https://example.com/3.png".into(),
                ],
            }),
        ]);
        let urls = s.image_urls();
        assert_eq!(urls.len(), 3);
        assert!(urls.iter().any(|u| u.ends_with("/1.png")));
        assert!(urls.iter().any(|u| u.ends_with("/3.png")));
    }

    #[test]
    fn children_regions_both_recover_under_stack() {
        let doc: Document<M> = Document::keyed(
            "d",
            flow![Stack::new(vec![
                Box::new(GestureAction::with_msg("alpha", "first action", M::Hit)),
                Box::new(GestureAction::with_msg("beta", "second action", M::Hit)),
            ])],
        );
        let compiled = compile_document_in(&doc, PageGeom::default(), &Theme::reader()).unwrap();
        let manifest = recover_regions(&compiled).unwrap();
        assert!(
            manifest.regions.iter().any(|r| r.name == "alpha"),
            "alpha region recovered"
        );
        assert!(
            manifest.regions.iter().any(|r| r.name == "beta"),
            "beta region recovered"
        );
    }
}
