//! Heading component: render shape, byline/meta optionality, decode-empty.

use inkapp_core::component::{Component, RenderCx};
use inkapp_core::components::heading::Heading;
use inkapp_core::render::compile_to_document_with_sources;
use inkapp_core::theme::Theme;

fn render(h: &Heading) -> String {
    let theme = Theme::reader();
    let mut cx = RenderCx::new(0).with_theme(theme);
    h.render(&mut cx)
}

#[test]
fn title_only_renders() {
    let out = render(&Heading::new("Hello world"));
    assert!(out.contains("Hello world"), "title in output: {out}");
}

#[test]
fn all_fields_render() {
    let h = Heading::new("Title")
        .byline("Jane")
        .reading_time("5 min")
        .subtitle("a summary");
    let out = render(&h);
    for fragment in ["Title", "Jane", "5 min", "a summary"] {
        assert!(out.contains(fragment), "missing {fragment}: {out}");
    }
}

#[test]
fn heading_typst_compiles() {
    let h = Heading::<()>::new("Compilable")
        .byline("Author")
        .reading_time("3 min");
    let theme = Theme::reader();
    let mut cx = RenderCx::new(0).with_theme(theme.clone());
    let body = h.render(&mut cx);
    let src = format!(
        "#import \"/inkapp/heading.typ\": *\n#set page(width: 200pt, height: 200pt, margin: 8pt)\n{}\n{body}",
        theme.prelude()
    );
    let sources = vec![
        (
            "/inkapp/region.typ".into(),
            include_str!("../typst/region.typ").into(),
        ),
        (
            "/inkapp/heading.typ".into(),
            include_str!("../typst/heading.typ").into(),
        ),
    ];
    compile_to_document_with_sources(&src, &sources).expect("Heading typst compiles");
}

#[test]
fn decode_is_empty() {
    let h = Heading::new("x");
    let manifest = inkapp_core::manifest::Manifest::default();
    // Default Heading<()> — decode returns Vec<()>
    let msgs = <Heading<()> as Component>::decode(&h, &[], &manifest);
    let _: Vec<()> = msgs;
}

#[test]
fn heading_generic_msg_decode_is_empty() {
    // Heading<u8> — still emits nothing; just verifies the generic impl works.
    let h = Heading::<u8>::new("generic");
    let manifest = inkapp_core::manifest::Manifest::default();
    let msgs = <Heading<u8> as Component>::decode(&h, &[], &manifest);
    assert!(msgs.is_empty());
}

#[test]
fn title_with_special_chars_escapes_and_compiles() {
    let title = r#"Say "hello" \ world"#;
    let h = Heading::<()>::new(title);
    let theme = Theme::reader();
    let mut cx = RenderCx::new(0).with_theme(theme.clone());
    let body = h.render(&mut cx);
    assert!(
        body.contains(r#"\""#),
        "quote not escaped in heading call: {body}"
    );
    let src = format!(
        "#import \"/inkapp/heading.typ\": *\n#set page(width: 200pt, height: 200pt, margin: 8pt)\n{}\n{body}",
        theme.prelude()
    );
    let sources = vec![
        (
            "/inkapp/region.typ".into(),
            include_str!("../typst/region.typ").into(),
        ),
        (
            "/inkapp/heading.typ".into(),
            include_str!("../typst/heading.typ").into(),
        ),
    ];
    compile_to_document_with_sources(&src, &sources)
        .expect("Heading with escaped specials compiles");
}

#[test]
fn absent_optionals_do_not_pollute_output() {
    let out = render(&Heading::new("just title"));
    assert!(!out.contains("byline:"), "stray byline: {out}");
    assert!(!out.contains("meta:"), "stray meta: {out}");
    assert!(!out.contains("subtitle:"), "stray subtitle: {out}");
}

#[test]
fn typst_sources_contract() {
    let h = Heading::<()>::new("x");
    let srcs = <Heading<()> as Component>::typst_sources(&h);
    assert_eq!(srcs.len(), 1, "exactly one source registered");
    assert_eq!(srcs[0].0, "/inkapp/heading.typ");
    assert!(!srcs[0].1.is_empty(), "source text must be non-empty");
}
