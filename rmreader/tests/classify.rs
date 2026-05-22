//! Geometric classification table tests.
use rmreader::manifest::{EmbeddedDoc, EmbeddedManifest, LabelRect, ManifestRect, PageRange};
use rmreader::readback::{classify, PdfRect, StrokeHit};
use rmreader::readwise::ActionKind;

/// Page geometry: 200 pt wide × 462 pt tall.
/// Action-label band: y=[380,400] (near the top, PDF bottom-left coords).
/// Four columns (left→right): Inbox[0,50], Later[50,100], Archive[100,150], Delete[150,200].
fn manifest() -> EmbeddedManifest {
    EmbeddedManifest {
        schema_version: 1,
        collection: "Library".into(),
        docs: vec![
            EmbeddedDoc {
                id: "d1".into(),
                title: "One".into(),
                url: "https://a".into(),
                author: "A".into(),
                category: "articles".into(),
                page_range: PageRange { first: 0, last: 1 },
            },
            EmbeddedDoc {
                id: "d2".into(),
                title: "Two".into(),
                url: "https://b".into(),
                author: "B".into(),
                category: "articles".into(),
                page_range: PageRange { first: 2, last: 2 },
            },
        ],
        label_rects: vec![
            LabelRect {
                kind: "inbox".into(),
                rect: ManifestRect {
                    x0: 0.0,
                    y0: 380.0,
                    x1: 50.0,
                    y1: 400.0,
                },
            },
            LabelRect {
                kind: "later".into(),
                rect: ManifestRect {
                    x0: 50.0,
                    y0: 380.0,
                    x1: 100.0,
                    y1: 400.0,
                },
            },
            LabelRect {
                kind: "archive".into(),
                rect: ManifestRect {
                    x0: 100.0,
                    y0: 380.0,
                    x1: 150.0,
                    y1: 400.0,
                },
            },
            LabelRect {
                kind: "delete".into(),
                rect: ManifestRect {
                    x0: 150.0,
                    y0: 380.0,
                    x1: 200.0,
                    y1: 400.0,
                },
            },
        ],
    }
}

fn hit(page: usize, x0: f64, y0: f64, x1: f64, y1: f64) -> StrokeHit {
    StrokeHit {
        page,
        bbox: PdfRect { x0, y0, x1, y1 },
    }
}

/// A stroke centred squarely in the Archive column → Archive action.
#[test]
fn stroke_over_archive_column_is_archive_action() {
    let p = classify(
        &manifest(),
        &[hit(1, 105.0, 382.0, 145.0, 398.0)],
        |_, _| String::new(),
    );
    assert_eq!(p.actions, vec![("d1".into(), ActionKind::Archive)]);
    assert!(p.highlights.is_empty());
    assert!(p.warnings.is_empty());
}

/// A stroke spanning Archive and Later columns (more area in Archive) → Archive.
/// bbox x=[60,140] y=[382,398]: Later overlap = 40×16=640, Archive overlap = 40×16=640.
/// Make it unambiguously more in Archive: x=[75,140] → Later=25×16=400, Archive=40×16=640.
#[test]
fn stroke_spanning_two_columns_picks_max_overlap() {
    // x=[75,140]: Later gets [75,100]=25 wide, Archive gets [100,140]=40 wide → Archive wins.
    let p = classify(&manifest(), &[hit(0, 75.0, 382.0, 140.0, 398.0)], |_, _| {
        String::new()
    });
    assert_eq!(p.actions, vec![("d1".into(), ActionKind::Archive)]);
    assert!(p.highlights.is_empty());
}

/// A body stroke (below the label band) → content highlight via words_under.
#[test]
fn body_stroke_becomes_content_highlight() {
    let words = |_page: usize, _bbox: &PdfRect| "the highlighted sentence".to_string();
    let p = classify(&manifest(), &[hit(2, 20.0, 100.0, 180.0, 115.0)], words);
    assert!(p.actions.is_empty());
    assert_eq!(p.highlights.len(), 1);
    assert_eq!(p.highlights[0].text, "the highlighted sentence");
    assert_eq!(p.highlights[0].source_url, "https://b");
    assert!(p.warnings.is_empty());
}

/// Body stroke where words_under returns "" → no highlight, one warning.
#[test]
fn content_with_no_text_warns() {
    let p = classify(&manifest(), &[hit(2, 20.0, 100.0, 180.0, 115.0)], |_, _| {
        String::new()
    });
    assert!(p.highlights.is_empty());
    assert_eq!(p.warnings.len(), 1);
}

/// Two distinct action-column hits on the same doc → no action, one warning.
#[test]
fn two_distinct_actions_skip_with_warning() {
    let hits = [
        hit(0, 105.0, 382.0, 145.0, 398.0), // Archive
        hit(1, 155.0, 382.0, 195.0, 398.0), // Delete
    ];
    let p = classify(&manifest(), &hits, |_, _| String::new());
    assert!(p.actions.is_empty());
    assert_eq!(p.warnings.len(), 1);
}

/// Two Archive-column hits on the same doc → deduplicated to one Archive action.
#[test]
fn duplicate_same_action_applies_once() {
    let hits = [
        hit(0, 105.0, 382.0, 145.0, 398.0),
        hit(1, 110.0, 383.0, 140.0, 397.0),
    ];
    let p = classify(&manifest(), &hits, |_, _| String::new());
    assert_eq!(p.actions, vec![("d1".into(), ActionKind::Archive)]);
}

/// Stroke on a page not covered by the manifest → warning, no action or highlight.
#[test]
fn stroke_off_manifest_warns() {
    let p = classify(
        &manifest(),
        &[hit(99, 50.0, 100.0, 150.0, 200.0)],
        |_, _| String::new(),
    );
    assert!(p.actions.is_empty());
    assert!(p.highlights.is_empty());
    assert_eq!(p.warnings.len(), 1);
}
