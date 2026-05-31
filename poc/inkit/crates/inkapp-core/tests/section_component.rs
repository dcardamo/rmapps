//! Section component: pagebreak between sections, body composition, decode delegation,
//! and section-state observability via a recoverable probe region.

use inkapp_core::component::{Component, RenderCx};
use inkapp_core::components::gesture::GestureAction;
use inkapp_core::components::heading::Heading;
use inkapp_core::components::notice::Notice;
use inkapp_core::components::section::Section;
use inkapp_core::document::Document;
use inkapp_core::flow;
use inkapp_core::geometry::{PageGeom, PdfPoint};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::{recover_regions, Manifest, Region};
use inkapp_core::readback::attribute_page;
use inkapp_core::render::compile_to_document_with_sources;
use inkapp_core::runtime::compile_document_in;
use inkapp_core::theme::Theme;

fn sources() -> Vec<(String, String)> {
    vec![
        (
            "/inkapp/region.typ".into(),
            include_str!("../typst/region.typ").into(),
        ),
        (
            "/inkapp/section.typ".into(),
            include_str!("../typst/section.typ").into(),
        ),
    ]
}

#[test]
fn render_emits_section_call_with_id() {
    let s: Section<()> = Section::new("art-1", vec![Box::new(Notice::line("hello"))]);
    let mut cx = RenderCx::new(0).with_theme(Theme::reader());
    let out = s.render(&mut cx);
    assert!(out.contains("section(\"art-1\""), "id in call: {out}");
    assert!(out.contains("hello"), "body composed: {out}");
}

#[test]
fn two_sections_produce_multiple_pages() {
    let theme = Theme::reader();
    let s1: Section<()> = Section::new("a", vec![Box::new(Notice::line("first"))]);
    let s2: Section<()> = Section::new("b", vec![Box::new(Notice::line("second"))]);
    let mut cx = RenderCx::new(0).with_theme(theme.clone());
    let src = format!(
        "#import \"/inkapp/section.typ\": *\n#set page(width: 200pt, height: 100pt, margin: 6pt)\n{}\n{}{}",
        theme.prelude(),
        s1.render(&mut cx),
        s2.render(&mut cx),
    );
    let doc = compile_to_document_with_sources(&src, &sources()).unwrap();
    assert!(
        doc.pages.len() >= 2,
        "two sections should paginate to ≥2 pages; got {}",
        doc.pages.len()
    );
}

#[test]
fn decode_delegates_to_body() {
    // A body Notice decodes empty; this just exercises that Section's decode loops
    // over children without panic. Real per-child decode tested in ActionBand integration.
    let s: Section<()> = Section::new("x", vec![Box::new(Notice::line("noop"))]);
    let manifest = inkapp_core::manifest::Manifest::default();
    let msgs = <Section<()> as Component>::decode(&s, &[], &manifest);
    assert!(msgs.is_empty());
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SecMsg {
    Hit,
}

#[test]
fn decode_forwards_child_msg() {
    let s: Section<SecMsg> = Section::new(
        "art-1",
        vec![Box::new(GestureAction::with_msg(
            "title",
            "Read me",
            SecMsg::Hit,
        ))],
    );
    let doc: Document<SecMsg> = Document::keyed("d", flow![s]);
    let compiled = compile_document_in(&doc, PageGeom::default(), &Theme::reader()).unwrap();
    let manifest = recover_regions(&compiled).unwrap();
    let r: &Region = manifest
        .regions
        .iter()
        .find(|r| r.name == "title")
        .expect("title region recovered through Section");
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
    assert_eq!(decoded, vec![SecMsg::Hit]);
}

#[test]
fn typst_sources_aggregates_section_and_body() {
    let s: Section<()> = Section::new("x", vec![Box::new(Heading::<()>::new("hi"))]);
    let paths: Vec<String> = s.typst_sources().into_iter().map(|(p, _)| p).collect();
    assert!(
        paths.iter().any(|p| p == "/inkapp/section.typ"),
        "section.typ missing: {paths:?}"
    );
    assert!(
        paths.iter().any(|p| p == "/inkapp/heading.typ"),
        "heading.typ not forwarded: {paths:?}"
    );
}

#[test]
fn image_urls_forwards_from_body() {
    struct Img;
    impl Component for Img {
        type Msg = ();
        fn render(&self, _cx: &mut RenderCx) -> String {
            String::new()
        }
        fn decode(&self, _ink: &[RegionInk], _m: &Manifest) -> Vec<()> {
            Vec::new()
        }
        fn image_urls(&self) -> Vec<String> {
            vec!["https://example.com/picture.png".into()]
        }
    }

    let s: Section<()> = Section::new("x", vec![Box::new(Img)]);
    let urls = s.image_urls();
    assert_eq!(urls, vec!["https://example.com/picture.png".to_string()]);
}

#[test]
fn render_emits_art_anchor_label() {
    let s: Section<()> = Section::new("art-1", vec![Box::new(Notice::line("body"))]);
    let mut cx = RenderCx::new(0).with_theme(Theme::reader());
    let out = s.render(&mut cx);
    assert!(out.contains("<art-art-1>"), "art anchor missing: {out}");
}
