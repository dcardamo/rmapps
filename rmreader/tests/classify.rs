//! Geometric classification table tests.
use rmreader::manifest::{EmbeddedDoc, EmbeddedManifest, LabelRect, ManifestRect, PageRange};
use rmreader::readback::{classify, PdfRect, StrokeHit};
use rmreader::readwise::ActionKind;

/// Page geometry: 260 pt wide × 462 pt tall.
/// Action-label band: y=[376,400] (near the top, PDF bottom-left coords).
/// Four columns in production order (inbox, archive, later, delete), left→right:
///   Inbox[0,65], Archive[65,130], Later[130,195], Delete[195,260].
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
                    y0: 376.0,
                    x1: 65.0,
                    y1: 400.0,
                },
            },
            LabelRect {
                kind: "archive".into(),
                rect: ManifestRect {
                    x0: 65.0,
                    y0: 376.0,
                    x1: 130.0,
                    y1: 400.0,
                },
            },
            LabelRect {
                kind: "later".into(),
                rect: ManifestRect {
                    x0: 130.0,
                    y0: 376.0,
                    x1: 195.0,
                    y1: 400.0,
                },
            },
            LabelRect {
                kind: "delete".into(),
                rect: ManifestRect {
                    x0: 195.0,
                    y0: 376.0,
                    x1: 260.0,
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
    // center y = (378+398)/2 = 388, inside [376,400]; x overlaps archive [65,130]
    let p = classify(&manifest(), &[hit(1, 70.0, 378.0, 125.0, 398.0)], |_, _| {
        String::new()
    });
    assert_eq!(p.actions, vec![("d1".into(), ActionKind::Archive)]);
    assert!(p.highlights.is_empty());
    assert!(p.warnings.is_empty());
}

/// A stroke spanning Archive and Later columns (more x-overlap in Archive) → Archive.
/// x=[80,170]: Archive gets [80,130]=50 wide, Later gets [130,170]=40 wide → Archive wins.
/// center y = (378+398)/2 = 388, inside band [376,400].
#[test]
fn stroke_spanning_two_columns_picks_max_overlap() {
    let p = classify(&manifest(), &[hit(0, 80.0, 378.0, 170.0, 398.0)], |_, _| {
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
        hit(0, 70.0, 378.0, 125.0, 398.0), // Archive (center y=388 in band)
        hit(1, 200.0, 378.0, 255.0, 398.0), // Delete  (center y=388 in band)
    ];
    let p = classify(&manifest(), &hits, |_, _| String::new());
    assert!(p.actions.is_empty());
    assert_eq!(p.warnings.len(), 1);
}

/// Two Archive-column hits on the same doc → deduplicated to one Archive action.
#[test]
fn duplicate_same_action_applies_once() {
    let hits = [
        hit(0, 70.0, 378.0, 125.0, 398.0),
        hit(1, 75.0, 379.0, 120.0, 397.0),
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

/// Guard test: a first-body-line highlight whose INFLATED bbox top edge pokes into
/// the label band but whose CENTER y is BELOW the band must NOT become an action.
///
/// Production scenario: label band y=[376,400]. A body highlight at approximately
/// y=[355,388] has center y=371.5 (below 376). The inflated top edge at 388 is
/// inside the band — under the old any-overlap logic this would be misclassified
/// as an Archive action. With center-in-band logic it is correctly a content highlight.
///
/// This test FAILS under the old area-overlap logic and PASSES with center-in-band.
#[test]
fn first_body_line_inflated_bbox_not_misclassified_as_action() {
    // LABEL hit: center y = (378+398)/2 = 388, inside [376,400] → Archive action.
    let label_hit = hit(0, 70.0, 378.0, 120.0, 398.0);

    // BODY hit: inflated bbox y=[355,388] — top edge 388 is inside the band, but
    // center y = (355+388)/2 = 371.5 < 376 → NOT an action, it's a content highlight.
    let body_hit = hit(0, 70.0, 355.0, 200.0, 388.0);

    // Test the body hit in isolation — it must produce a content highlight, not an action.
    let p_body = classify(&manifest(), std::slice::from_ref(&body_hit), |_, _| {
        "body text".to_string()
    });
    assert!(
        p_body.actions.is_empty(),
        "inflated first-body-line hit should NOT be an action; got: {:?}",
        p_body.actions,
    );
    assert_eq!(
        p_body.highlights.len(),
        1,
        "should produce a content highlight"
    );
    assert_eq!(p_body.highlights[0].text, "body text");
    assert!(p_body.warnings.is_empty());

    // Together: label hit → Archive action, body hit → content highlight (no confusion).
    let p_both = classify(&manifest(), &[label_hit, body_hit], |_, _| {
        "body text".to_string()
    });
    assert_eq!(
        p_both.actions,
        vec![("d1".into(), ActionKind::Archive)],
        "label hit should still produce Archive action",
    );
    assert_eq!(
        p_both.highlights.len(),
        1,
        "body hit should still be a content highlight"
    );
    assert!(p_both.warnings.is_empty());
}
