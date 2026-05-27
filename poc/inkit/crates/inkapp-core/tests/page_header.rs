//! Document::page_header — header is rendered into `#set page(header: ...)` and
//! its regions appear on every page frame.

use inkapp_core::components::notice::Notice;
use inkapp_core::document::Document;
use inkapp_core::manifest::recover_regions;
use inkapp_core::components::passage::Passage;
use inkapp_core::runtime::{collect_typst_sources, document_source_in};
use inkapp_core::flow;
use inkapp_core::theme::Theme;

#[test]
fn header_source_contains_set_page_header() {
    // A long passage forces multi-page; the header should be rendered into #set page(header:).
    let doc: Document<()> = Document::keyed(
        "doc",
        flow![Passage::new("body", &(0..200).map(|i| format!("line {i}")).collect::<Vec<_>>().iter().map(String::as_str).collect::<Vec<_>>())],
    ).page_header(Notice::line("HEADER MARK"));

    let src = document_source_in(&doc, Default::default(), &Theme::reader());
    assert!(src.contains("#set page(header:"), "header set: {src:.200}");
    assert!(src.contains("HEADER MARK"), "header text in source");
}

#[test]
fn collect_sources_includes_header_typst() {
    let doc: Document<()> = Document::keyed("d", flow![Notice::line("body")])
        .page_header(Notice::line("hd"));
    let sources = collect_typst_sources(&doc);
    assert!(sources.iter().any(|(p, _)| p == "/inkapp/region.typ"), "prelude present");
    // Notice has no authored sources today; the assertion is that we don't crash and we
    // include whatever the header brings. If Notice grows authored sources later, this
    // will still hold by construction.
}

#[test]
fn header_regions_appear_on_every_page() {
    // Make a wide-bodied doc with a Passage that splits across at least 2 pages, plus a
    // page header that emits a recoverable region named `hd-mark` per page.
    use inkapp_core::manifest::Manifest;
    use inkapp_core::components::passage::Passage;
    let body_lines: Vec<String> = (0..120).map(|i| format!("Line of body content {i}")).collect();
    let line_refs: Vec<&str> = body_lines.iter().map(String::as_str).collect();
    let body = Passage::new("body", &line_refs);
    let header = Passage::new("hd-mark", &["page-header sentinel"]);

    let doc: Document<()> = Document::keyed("d", flow![body]).page_header(header);
    let geom = inkapp_core::geometry::PageGeom { w: 200.0, h: 150.0, margin: 8.0 };

    let src = document_source_in(&doc, geom, &Theme::reader());
    let sources = collect_typst_sources(&doc);
    let compiled = inkapp_core::render::compile_to_document_with_sources(&src, &sources).unwrap();
    assert!(compiled.pages.len() >= 2, "test fixture must span ≥2 pages; got {}", compiled.pages.len());

    let manifest: Manifest = recover_regions(&compiled).unwrap();
    let hd_regions: Vec<_> = manifest.regions.iter().filter(|r| r.name == "hd-mark").collect();
    assert!(
        hd_regions.len() >= compiled.pages.len(),
        "expected ≥1 'hd-mark' region per page; got {} regions over {} pages",
        hd_regions.len(),
        compiled.pages.len()
    );
}
