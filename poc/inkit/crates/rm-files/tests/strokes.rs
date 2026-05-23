//! Integration tests against a real reMarkable Paper Pro capture.
//!
//! The fixture `stamped-labels.rmdoc` is a zip; the page scene lives at
//! `<uuid>/<page>.rm`. These tests parse that real `.rm` and assert on the
//! highlighter strokes, cross-checked against the rmscene reference parser.

use std::io::Read;

use rm_files::{Pen, PenColor, Scene};

/// Read the single `.rm` page file out of the bundled `.rmdoc` zip.
fn load_rm_bytes() -> Vec<u8> {
    let rmdoc_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/stamped-labels.rmdoc"
    );
    let file = std::fs::File::open(rmdoc_path).expect("open rmdoc fixture");
    let mut archive = zip::ZipArchive::new(file).expect("read zip");

    let rm_name = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .find(|name| name.ends_with(".rm"))
        .expect("a .rm entry in the rmdoc");

    let mut entry = archive.by_name(&rm_name).expect("open .rm entry");
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).expect("read .rm bytes");
    bytes
}

#[test]
fn parses_highlighter_strokes_from_real_fixture() {
    let bytes = load_rm_bytes();
    let scene = Scene::parse(&bytes).expect("parse v6 scene");

    assert_eq!(scene.version(), 6, "fixture is a v6 file");

    let strokes = scene.strokes();
    assert!(
        strokes.len() >= 2,
        "expected >=2 strokes, got {}",
        strokes.len()
    );

    for stroke in &strokes {
        assert!(
            stroke.is_highlighter(),
            "every stroke should be a highlighter, got {:?}",
            stroke.tool
        );
        assert_eq!(stroke.tool, Pen::Highlighter2, "tool is HIGHLIGHTER_2");
        assert_eq!(stroke.color, PenColor::Highlight, "color is HIGHLIGHT");
        assert!(!stroke.points.is_empty(), "stroke has points");
    }

    // The fixture has a top-band label (ARCHIVE, y<200) and a body sentence
    // (y>250). Confirm we recover strokes in both regions.
    let min_y = |s: &&rm_files::Stroke| s.points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
    assert!(
        strokes.iter().any(|s| min_y(s) < 200.0),
        "expected at least one stroke in the top band (y<200)"
    );
    assert!(
        strokes.iter().any(|s| min_y(s) > 250.0),
        "expected at least one stroke in the body (y>250)"
    );
}

#[test]
fn exact_stroke_geometry_matches_rmscene_oracle() {
    let bytes = load_rm_bytes();
    let scene = Scene::parse(&bytes).expect("parse v6 scene");
    let strokes = scene.strokes();

    // From the rmscene oracle (tool=18, color=9, 2 points each):
    //   x[-579,-386] y[153,153]
    //   x[-779,388]  y[307,307]
    //   x[-588,-339] y[156,156]
    //   x[-786,402]  y[320,320]
    let expected: &[(i32, i32, i32)] = &[
        (-579, -386, 153),
        (-779, 388, 307),
        (-588, -339, 156),
        (-786, 402, 320),
    ];

    assert_eq!(strokes.len(), 4, "fixture has exactly 4 strokes");
    for (stroke, &(xmin, xmax, y)) in strokes.iter().zip(expected) {
        assert_eq!(stroke.points.len(), 2, "each stroke has 2 points");
        let xs: Vec<f32> = stroke.points.iter().map(|p| p.x).collect();
        let ys: Vec<f32> = stroke.points.iter().map(|p| p.y).collect();
        let got_xmin = xs.iter().cloned().fold(f32::INFINITY, f32::min);
        let got_xmax = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let got_ymin = ys.iter().cloned().fold(f32::INFINITY, f32::min);
        let got_ymax = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert_eq!(got_xmin.round() as i32, xmin, "x-min");
        assert_eq!(got_xmax.round() as i32, xmax, "x-max");
        // Both points share the same y (rounded) in this fixture.
        assert_eq!(got_ymin.round() as i32, y, "y-min");
        assert_eq!(got_ymax.round() as i32, y, "y-max");
    }
}

#[test]
fn rejects_non_v6() {
    // A well-formed header that declares version=5 must be rejected.
    let mut header = b"reMarkable .lines file, version=5          ".to_vec();
    assert_eq!(header.len(), 43, "header is 43 bytes");
    // Append some arbitrary bytes; parsing should fail at the version check.
    header.extend_from_slice(&[0u8; 8]);

    match Scene::parse(&header) {
        Err(rm_files::Error::UnsupportedVersion(5)) => {}
        other => panic!("expected UnsupportedVersion(5), got {other:?}"),
    }
}
