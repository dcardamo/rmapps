use inkapp_core::component::Component;
use inkapp_core::components::passage::Passage;
use inkapp_core::geometry::PdfPoint;
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::Manifest;

#[derive(Debug, Clone, PartialEq)]
enum M {
    Captured,
}

#[test]
fn decode_fires_once_on_any_ink() {
    let p = Passage::with_msg("notes", &["hello world", "second line"], M::Captured);
    let ink = vec![RegionInk {
        region: "notes".into(),
        strokes: vec![Stroke {
            points: vec![PdfPoint { x: 1.0, y: 1.0 }],
            highlighter: true,
        }],
    }];
    assert_eq!(p.decode(&ink, &Manifest::default()), vec![M::Captured]);
    assert!(p.decode(&[], &Manifest::default()).is_empty());
}

#[test]
fn render_emits_breakable_region() {
    let p = Passage::with_msg("notes", &["a", "b"], M::Captured);
    let src = p.render(&mut inkapp_core::component::RenderCx::new(0));
    assert!(
        src.contains("#region(\"notes\""),
        "calls the region prelude: {src}"
    );
    assert!(
        src.contains("breakable: true"),
        "as a breakable region: {src}"
    );
    assert!(src.contains("#\"a\""), "line 'a' present in body: {src}");
    assert!(src.contains("#\"b\""), "line 'b' present in body: {src}");
}

// Integration test: render → recover → attribute → decode, single page.
// Cross-page split-stitch is exercised in crates/inkapp-harness/tests/pagination_device_blind.rs (Task 7).
use inkapp_core::document::Document;
use inkapp_core::flow;
use inkapp_core::geometry::PageGeom;
use inkapp_core::manifest::recover_regions;
use inkapp_core::readback::attribute_page;
use inkapp_core::runtime::compile_document_in;
use inkapp_core::Theme;

#[test]
fn passage_decodes_ink_end_to_end() {
    let doc: Document<M> = Document::keyed(
        "d",
        flow![Passage::with_msg(
            "notes",
            &["the quick brown fox", "jumps over the lazy dog"],
            M::Captured
        )],
    );
    let compiled = compile_document_in(&doc, PageGeom::default(), &Theme::reader()).unwrap();
    let manifest = recover_regions(&compiled).unwrap();

    // Find the passage region and drop a stroke at its center.
    let region = manifest
        .regions
        .iter()
        .find(|r| r.name == "notes")
        .expect("notes region recovered");
    let cx = (region.rect.x0 + region.rect.x1) / 2.0;
    let cy = (region.rect.y0 + region.rect.y1) / 2.0;
    let stroke = Stroke {
        points: vec![PdfPoint { x: cx, y: cy }],
        highlighter: true,
    };
    let ink = attribute_page(&[stroke], &manifest);

    // Decode through the document's own component — no duplicate construction.
    let decoded = doc.flow[0].decode(&ink, &manifest);
    assert_eq!(
        decoded,
        vec![M::Captured],
        "ink in the passage decodes to one Captured"
    );
}
