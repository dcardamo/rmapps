//! The theme threads through `document_source_in` into the `#set page` fill and
//! is available on the `RenderCx` (only emits a page fill when paper is set).

use inkapp_core::document::Document;
use inkapp_core::geometry::PageGeom;
use inkapp_core::runtime::{document_source_in, RenderEnv};
use inkapp_core::theme::Theme;

#[test]
fn grayscale_emits_no_page_fill() {
    let doc: Document<()> = Document::keyed("d", vec![]);
    let src = document_source_in(&doc, RenderEnv::default());
    assert!(src.contains("#set page("), "page set is present: {src}");
    assert!(
        !src.contains("fill:"),
        "no page fill under grayscale: {src}"
    );
}

#[test]
fn color_theme_sets_warm_paper_fill() {
    let doc: Document<()> = Document::keyed("d", vec![]);
    let env = RenderEnv {
        geom: PageGeom::default(),
        theme: Theme::indigo_tomato(),
    };
    let src = document_source_in(&doc, env);
    assert!(
        src.contains("fill: rgb(\"#F3F1EA\")"),
        "warm paper fill applied: {src}"
    );
}

#[test]
fn pagegeom_still_accepted_via_from() {
    let doc: Document<()> = Document::keyed("d", vec![]);
    let _ = document_source_in(&doc, PageGeom::default());
}
