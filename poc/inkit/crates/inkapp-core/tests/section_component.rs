//! Section component: pagebreak between sections, body composition, decode delegation,
//! and section-state observability via a recoverable probe region.

use inkapp_core::component::{Component, RenderCx};
use inkapp_core::components::notice::Notice;
use inkapp_core::components::section::Section;
use inkapp_core::render::compile_to_document_with_sources;
use inkapp_core::theme::Theme;

fn sources() -> Vec<(String, String)> {
    vec![
        ("/inkapp/region.typ".into(), include_str!("../typst/region.typ").into()),
        ("/inkapp/section.typ".into(), include_str!("../typst/section.typ").into()),
    ]
}

#[test]
fn render_emits_section_call_with_id() {
    let s: Section<()> = Section::new(
        "art-1",
        vec![Box::new(Notice::line("hello"))],
    );
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
    assert!(doc.pages.len() >= 2, "two sections should paginate to ≥2 pages; got {}", doc.pages.len());
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
