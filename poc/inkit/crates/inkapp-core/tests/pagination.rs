use inkapp_core::component::Component;
use inkapp_core::components::notice::Notice;
use inkapp_core::document::Document;
use inkapp_core::geometry::PageGeom;
use inkapp_core::runtime::compile_document_in;

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
