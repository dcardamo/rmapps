use inkapp_core::device::Device;
use inkapp_core::geometry::PdfPoint;
use inkapp_core::ink::Stroke;
use rm_device::Remarkable;

// A4 height in pt. The 0.5pt round-trip tolerance below is calibrated to this
// page height: the f32 quantization error in `.rm` scene coordinates is
// proportional to page height, so a much taller page would need a looser bound.
const PAGE_H: f64 = 841.89;

// Conservative width: 0.5 * page_h gives ample interior room at any aspect.
// The transform inverts at any input, so we don't need the *real* page_w;
// we just need sample points in a known region.
fn page_w_for(rm: &Remarkable, page_h: f64) -> f64 {
    let _ = rm;
    page_h * 0.5
}

#[test]
fn transform_is_invertible_across_geometries() {
    let rm = Remarkable::new();
    for (page_h, label) in &[(560.0_f64, "inkapp-default"), (841.89_f64, "a4")] {
        let page_w = page_w_for(&rm, *page_h);
        let samples: &[(f64, f64, &str)] = &[
            (page_w / 2.0, *page_h / 2.0, "center"),
            (0.0, 0.0, "bottom-left"),
            (page_w, 0.0, "bottom-right"),
            (0.0, *page_h, "top-left"),
            (page_w, *page_h, "top-right"),
            (page_w / 2.0, 0.0, "mid-bottom"),
        ];
        for (x, y, name) in samples {
            let p = PdfPoint { x: *x, y: *y };
            let d = rm.pdf_to_device(p, *page_h);
            let back = rm.device_to_pdf(d, *page_h);
            assert!(
                (back.x - p.x).abs() < 1e-6,
                "[{label}/{name}] x inverts: {} vs {}",
                p.x,
                back.x
            );
            assert!(
                (back.y - p.y).abs() < 1e-6,
                "[{label}/{name}] y inverts: {} vs {}",
                p.y,
                back.y
            );
        }
    }
}

#[test]
fn ink_round_trips_through_rm() {
    let rm = Remarkable::new();
    let original = vec![Stroke {
        points: vec![
            PdfPoint { x: 50.0, y: 700.0 },
            PdfPoint { x: 150.0, y: 700.0 },
        ],
        highlighter: true,
    }];
    let bytes = rm.write_ink(&original, PAGE_H).unwrap();
    let got = rm.read_ink(&bytes, PAGE_H).unwrap();
    assert_eq!(got.len(), 1);
    assert!(got[0].highlighter, "highlighter flag preserved");
    for (a, b) in original[0].points.iter().zip(&got[0].points) {
        assert!(
            (a.x - b.x).abs() < 0.5,
            "x within tolerance: {} vs {}",
            a.x,
            b.x
        );
        assert!(
            (a.y - b.y).abs() < 0.5,
            "y within tolerance: {} vs {}",
            a.y,
            b.y
        );
    }
}

#[test]
fn non_highlighter_ink_round_trips() {
    let rm = Remarkable::new();
    let original = vec![Stroke {
        points: vec![
            PdfPoint { x: 100.0, y: 300.0 },
            PdfPoint { x: 120.0, y: 320.0 },
        ],
        highlighter: false,
    }];
    let bytes = rm.write_ink(&original, PAGE_H).unwrap();
    let got = rm.read_ink(&bytes, PAGE_H).unwrap();
    assert_eq!(got.len(), 1);
    assert!(!got[0].highlighter, "pen (non-highlighter) flag preserved");
    for (a, b) in original[0].points.iter().zip(&got[0].points) {
        assert!((a.x - b.x).abs() < 0.5, "x within tolerance");
        assert!((a.y - b.y).abs() < 0.5, "y within tolerance");
    }
}

#[test]
fn off_page_strokes_round_trip_without_clamping() {
    let rm = Remarkable::new();
    let page_h = 560.0_f64;
    let cases = [
        (
            "left-of-page",
            PdfPoint {
                x: -100.0,
                y: 200.0,
            },
        ),
        (
            "above-page",
            PdfPoint {
                x: 100.0,
                y: page_h + 100.0,
            },
        ),
        ("below-page", PdfPoint { x: 100.0, y: -50.0 }),
    ];
    for (label, p) in cases {
        let d = rm.pdf_to_device(p, page_h);
        let back = rm.device_to_pdf(d, page_h);
        assert!(
            (back.x - p.x).abs() < 1e-6 && (back.y - p.y).abs() < 1e-6,
            "[{label}] off-page point did not round-trip: {p:?} -> {d:?} -> {back:?}"
        );
    }
}

#[test]
fn read_ink_does_not_drop_off_canvas_points() {
    let rm = Remarkable::new();
    let page_h = 560.0_f64;
    let original = vec![Stroke {
        points: vec![
            PdfPoint { x: -50.0, y: -50.0 }, // off-page
            PdfPoint { x: 100.0, y: 100.0 }, // on-page
            PdfPoint {
                x: 9999.0,
                y: 9999.0,
            }, // far off-page
        ],
        highlighter: false,
    }];
    let bytes = rm.write_ink(&original, page_h).unwrap();
    let got = rm.read_ink(&bytes, page_h).unwrap();
    assert_eq!(got.len(), 1, "stroke count preserved");
    assert_eq!(
        got[0].points.len(),
        original[0].points.len(),
        "point count preserved — off-page points not dropped"
    );
}

const CALIBRATION_FIXTURE: &str = "../inkapp-harness/tests/fixtures/recordings/calibration.rmdoc";

#[test]
fn calibration_fixture_decodes_to_expected_pdf_region() {
    // Fixture: crates/inkapp-harness/tests/fixtures/recordings/calibration.rmdoc
    // (5-cross tap sheet, reMarkable Paper Pro Move, captured 2026-05-23).
    // The expected bbox below was captured by _dump_calibration_strokes
    // (Phase A of this task) and is pinned with 4pt tolerance — matching
    // the per-point residuals quoted in rm-device/src/lib.rs.
    use rm_files::Bundle;

    let bundle =
        Bundle::open(std::path::Path::new(CALIBRATION_FIXTURE)).expect("open calibration bundle");
    let (_w, page_h) = bundle.canvas_size();
    let pages = bundle.pages();
    let bytes = pages[0]
        .scene_bytes()
        .expect("calibration fixture page 0 scene bytes");

    let rm = Remarkable::new();
    let strokes = rm.read_ink(bytes, page_h).expect("read_ink");
    assert!(
        !strokes.is_empty(),
        "calibration fixture decoded to zero strokes"
    );

    let s = &strokes[0];
    let (mut x0, mut y0, mut x1, mut y1) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for p in &s.points {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }

    // VALUES FROM PHASE A — captured from _dump_calibration_strokes output.
    let (ex0, ey0, ex1, ey1) = (703.15, 933.96, 706.67, 939.91);
    let tol = 4.0;
    assert!(
        (x0 - ex0).abs() < tol,
        "x0 drift: {x0} vs expected {ex0} (tol {tol})"
    );
    assert!(
        (y0 - ey0).abs() < tol,
        "y0 drift: {y0} vs expected {ey0} (tol {tol})"
    );
    assert!(
        (x1 - ex1).abs() < tol,
        "x1 drift: {x1} vs expected {ex1} (tol {tol})"
    );
    assert!(
        (y1 - ey1).abs() < tol,
        "y1 drift: {y1} vs expected {ey1} (tol {tol})"
    );
}

// Fixture: crates/rm-files/tests/fixtures/rmtest-glyph.rmdoc
// (per crates/rm-files/tests/highlights.rs, page index 1 carries a
// GlyphRange text="ARCHIVE"; page index 2 carries another. Either works.)
const GLYPH_FIXTURE_BUNDLE: &str = "../rm-files/tests/fixtures/rmtest-glyph.rmdoc";

#[test]
fn text_highlight_rect_synthesizes_swipe() {
    use rm_files::{Bundle, Scene};

    let bundle = Bundle::open(std::path::Path::new(GLYPH_FIXTURE_BUNDLE))
        .expect("open glyph fixture bundle");
    let (_w, page_h) = bundle.canvas_size();

    let pages = bundle.pages();
    let mut glyph_bytes: Option<Vec<u8>> = None;
    for page in &pages {
        let Some(bytes) = page.scene_bytes() else {
            continue;
        };
        let scene = Scene::parse(bytes).expect("parse scene");
        if !scene.text_highlights().is_empty() {
            glyph_bytes = Some(bytes.to_vec());
            break;
        }
    }
    let bytes = glyph_bytes.expect("no page in fixture had a GlyphRange");

    let rm = Remarkable::new();
    let strokes = rm.read_ink(&bytes, page_h).expect("read_ink");

    let synthesized: Vec<&Stroke> = strokes
        .iter()
        .filter(|s| s.highlighter && s.points.len() == 17)
        .collect();
    assert!(
        !synthesized.is_empty(),
        "expected at least one 17-point highlighter swipe synthesized from a GlyphRange (got {} strokes total)",
        strokes.len()
    );

    let swipe = synthesized[0];
    let y0 = swipe.points[0].y;
    for (i, pt) in swipe.points.iter().enumerate() {
        assert!(
            (pt.y - y0).abs() < 1e-6,
            "swipe point {i} y drifted: {} vs {}",
            pt.y,
            y0
        );
    }
    let xs: Vec<f64> = swipe.points.iter().map(|p| p.x).collect();
    for w in xs.windows(2) {
        assert!(w[1] >= w[0] - 1e-9, "swipe x not monotonic: {:?}", xs);
    }
}
