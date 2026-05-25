use inkapp_core::component::Component;
use inkapp_core::components::notice::Notice;
use inkapp_core::document::Document;
use inkapp_core::geometry::PageGeom;
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::{compile_to_document, compile_to_document_with_sources};
use inkapp_core::runtime::{compile_document_in, REGION_PRELUDE};

/// A tall flow: 40 notice lines. Fits in few pages at the default geometry and
/// more pages on a short page — pagination is purely a function of PageGeom.
fn tall_doc() -> Document<()> {
    let mut f: Vec<Box<dyn Component<Msg = ()>>> = Vec::new();
    for i in 0..40 {
        f.push(Box::new(Notice::line(&format!("line number {i}"))));
    }
    Document::keyed("tall", f)
}

#[test]
fn short_page_paginates_to_more_pages() {
    let doc = tall_doc();
    let default_pages = compile_document_in(&doc, PageGeom::default())
        .unwrap()
        .pages
        .len();
    let short_pages = compile_document_in(
        &doc,
        PageGeom {
            w: 420.0,
            h: 180.0,
            margin: 16.0,
        },
    )
    .unwrap()
    .pages
    .len();
    assert!(
        short_pages > default_pages,
        "short page must paginate to more pages: short={short_pages} default={default_pages}"
    );
}

#[test]
fn flow_region_recovers_one_rect_per_frame() {
    // Emit flow-start/flow-end markers directly (no prelude needed) around a tall
    // block on a short page, so the body spans several frames.
    let src = r#"
#set page(width: 200pt, height: 100pt, margin: 8pt)
#context [#metadata((name: "p", role: "flow-start", page: here().position().page - 1, x: here().position().x / 1pt, y: here().position().y / 1pt, w: 120.0)) <region>]
#block(height: 300pt, fill: luma(230))[]
#context [#metadata((name: "p", role: "flow-end", page: here().position().page - 1, x: here().position().x / 1pt, y: here().position().y / 1pt)) <region>]
"#;
    let doc = compile_to_document(src).unwrap();
    let m = recover_regions(&doc).unwrap();
    let p: Vec<_> = m.regions.iter().filter(|r| r.name == "p").collect();
    assert!(
        p.len() >= 2,
        "a 300pt body on an ~84pt page must split into ≥2 frames, got {}",
        p.len()
    );
    let pages: Vec<usize> = p.iter().map(|r| r.page).collect();
    assert!(
        pages.windows(2).all(|w| w[0] < w[1]),
        "frames in page order: {pages:?}"
    );
}

#[test]
fn orphaned_flow_end_errors() {
    let src = r#"
#set page(width: 200pt, height: 100pt, margin: 8pt)
#context [#metadata((name: "p", role: "flow-end", page: here().position().page - 1, x: here().position().x / 1pt, y: here().position().y / 1pt)) <region>]
"#;
    let doc = inkapp_core::render::compile_to_document(src).unwrap();
    assert!(
        inkapp_core::manifest::recover_regions(&doc).is_err(),
        "orphaned flow-end must error"
    );
}

#[test]
fn prelude_breakable_splits_atomic_does_not() {
    let src = r#"#import "/inkapp/region.typ": *
#set page(width: 200pt, height: 100pt, margin: 8pt)
#region("p", [#block(height: 300pt, fill: luma(230))[]], breakable: true)
#region("c", box(width: 14pt, height: 14pt, stroke: 0.5pt))
"#;
    let sources = vec![(REGION_PRELUDE.0.to_string(), REGION_PRELUDE.1.to_string())];
    let doc = compile_to_document_with_sources(src, &sources).unwrap();
    let m = recover_regions(&doc).unwrap();
    assert!(
        m.regions.iter().filter(|r| r.name == "p").count() >= 2,
        "breakable region splits across frames"
    );
    // Each recovered "p" rect must span the full content column width.
    // Page is 200pt wide with 8pt margins → content column ≈ 184pt.
    // Before the fix, measure(body) in an unconstrained context returned ~0 for
    // a zero-width block, so this assertion catches regressions to zero-width rects.
    for r in m.regions.iter().filter(|r| r.name == "p") {
        let w = r.rect.x1 - r.rect.x0;
        assert!(
            w > 150.0,
            "breakable region frame should be ~column-width (~184pt), got {w}"
        );
    }
    assert_eq!(
        m.regions.iter().filter(|r| r.name == "c").count(),
        1,
        "atomic region stays a single rect"
    );
}
