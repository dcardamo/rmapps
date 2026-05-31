//! Integration tests for snap-to-text highlights (`GlyphRange`) against a real
//! reMarkable Paper Pro capture.
//!
//! The fixture `rmtest-glyph.rmdoc` is a 3-page test doc whose page index 1
//! carries a `GlyphRange text="ARCHIVE"` and page index 2 carries
//! `GlyphRange text="Sphinx of black quartz, judge my vow."` — both color
//! HIGHLIGHT (PenColor 9). Verified against the rmscene reference parser.

use std::collections::BTreeSet;
use std::path::Path;

use rmfiles::{Bundle, PenColor};

fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rmtest-glyph.rmdoc")
}

#[test]
fn parses_text_highlights_from_real_fixture() {
    let bundle = Bundle::open(&fixture()).expect("open rmtest-glyph.rmdoc");

    // Collect every text highlight across all pages.
    let mut texts = BTreeSet::new();
    let mut total = 0usize;
    for page in bundle.pages() {
        let Some(scene) = page.scene().expect("parse page scene") else {
            continue;
        };
        for hl in scene.text_highlights() {
            total += 1;
            texts.insert(hl.text.clone());

            // Every text highlight in this fixture is a HIGHLIGHT color with
            // at least one bounding rectangle.
            assert_eq!(
                hl.color,
                PenColor::Highlight,
                "highlight {:?} should be HIGHLIGHT color",
                hl.text
            );
            assert!(
                !hl.rectangles.is_empty(),
                "highlight {:?} should have >=1 rectangle",
                hl.text
            );
        }
    }

    let expected: BTreeSet<String> = ["ARCHIVE", "Sphinx of black quartz, judge my vow."]
        .into_iter()
        .map(String::from)
        .collect();

    assert_eq!(
        texts, expected,
        "recovered highlight texts should match the ground truth"
    );
    assert_eq!(total, 2, "fixture has exactly two text highlights");
}

#[test]
fn rectangles_have_finite_nonzero_extent() {
    let bundle = Bundle::open(&fixture()).expect("open rmtest-glyph.rmdoc");

    for page in bundle.pages() {
        let Some(scene) = page.scene().expect("parse page scene") else {
            continue;
        };
        for hl in scene.text_highlights() {
            for r in &hl.rectangles {
                assert!(r.x.is_finite() && r.y.is_finite(), "rect origin finite");
                assert!(r.w > 0.0, "rect width positive for {:?}", hl.text);
                assert!(r.h > 0.0, "rect height positive for {:?}", hl.text);
            }
        }
    }
}
