mod common;
use common::assert_golden;

use std::collections::HashSet;

use inkapp_core::components::index::{Index, IndexEntry};
use inkapp_core::document::Document;
use inkapp_core::flow;
use inkapp_core::geometry::PageGeom;
use inkapp_core::manifest::recover_regions;
use inkapp_core::runtime::compile_document_in;
use inkapp_core::theme::Theme;
use inkapp_harness::inspector::render_page;

fn sample_entries(n: usize) -> Vec<IndexEntry> {
    (0..n)
        .map(|i| IndexEntry {
            title: format!("Article number {i}: a reasonably long headline"),
            byline: Some(format!("Author {i}")),
            reading_time: Some(format!("{} min", i + 2)),
            summary: Some(
                "A concise standfirst describing the piece in a sentence or two so the \
                 reader can decide whether to open it."
                    .into(),
            ),
        })
        .collect()
}

#[test]
fn index_renders_and_paginates() {
    // Enough entries to overflow one 420×560 page and force a second, under the
    // default reader theme. Calibrated to land on exactly 2 pages at default geometry.
    let n = 9;
    let doc: Document<()> = Document::keyed("contents", flow![Index::<()>::new(sample_entries(n))]);
    let compiled = compile_document_in(&doc, PageGeom::default(), &Theme::reader()).unwrap();
    assert_eq!(compiled.pages.len(), 2, "{n} entries paginate to two pages");

    let manifest = recover_regions(&compiled).unwrap();
    let pages: HashSet<usize> = manifest
        .regions
        .iter()
        .filter(|r| r.name.starts_with("idx-"))
        .map(|r| r.page)
        .collect();
    assert!(
        pages.contains(&0) && pages.contains(&1),
        "entries land on both pages: {pages:?}"
    );

    let idx_regions = manifest
        .regions
        .iter()
        .filter(|r| r.name.starts_with("idx-"))
        .count();
    assert_eq!(idx_regions, n, "every entry rendered as an idx-* region");

    assert_golden("index_page0", &render_page(&compiled, 0).unwrap());
    assert_golden("index_page1", &render_page(&compiled, 1).unwrap());
}
