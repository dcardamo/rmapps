use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::Stroke;
use inkapp_harness::fixtures::{normalize, transplant, Fit, Sample};

fn s(points: &[(f64, f64)]) -> Stroke {
    Stroke {
        points: points.iter().map(|&(x, y)| PdfPoint { x, y }).collect(),
        highlighter: false,
    }
}

#[test]
fn normalize_unit_box_and_aspect() {
    let sample = normalize(&[s(&[(10.0, 100.0), (50.0, 120.0)])]);
    assert!((sample.native_aspect - 2.0).abs() < 1e-9);
    assert_eq!(sample.strokes.len(), 1);
    let p = &sample.strokes[0].points;
    assert_eq!(p[0], [0.0, 0.0]);
    assert_eq!(p[1], [1.0, 1.0]);
}

#[test]
fn stretch_fills_target() {
    let sample = Sample {
        native_aspect: 2.0,
        strokes: vec![us(&[[0.0, 0.0], [1.0, 1.0]])],
    };
    let t = PdfRect {
        x0: 100.0,
        y0: 200.0,
        x1: 140.0,
        y1: 230.0,
    };
    let out = transplant(&sample, t, Fit::Stretch, false);
    let p = &out[0].points;
    assert_eq!(p[0], PdfPoint { x: 100.0, y: 200.0 });
    assert_eq!(p[1], PdfPoint { x: 140.0, y: 230.0 });
}

#[test]
fn aspect_fit_centers_and_preserves_shape() {
    let sample = Sample {
        native_aspect: 2.0,
        strokes: vec![us(&[[0.0, 0.0], [1.0, 1.0]])],
    };
    let t = PdfRect {
        x0: 0.0,
        y0: 0.0,
        x1: 40.0,
        y1: 40.0,
    };
    let out = transplant(&sample, t, Fit::AspectFit, false);
    let p = &out[0].points;
    assert!((p[0].x - 0.0).abs() < 1e-9 && (p[0].y - 10.0).abs() < 1e-9);
    assert!((p[1].x - 40.0).abs() < 1e-9 && (p[1].y - 30.0).abs() < 1e-9);
}

#[test]
fn stretch_x_fills_width_keeps_proportion() {
    let sample = Sample {
        native_aspect: 4.0,
        strokes: vec![us(&[[0.0, 0.0], [1.0, 1.0]])],
    };
    let t = PdfRect {
        x0: 0.0,
        y0: 0.0,
        x1: 80.0,
        y1: 40.0,
    };
    let out = transplant(&sample, t, Fit::StretchX, true);
    assert!(out[0].highlighter, "tool flag carried through");
    let p = &out[0].points;
    assert!((p[0].x - 0.0).abs() < 1e-9 && (p[0].y - 10.0).abs() < 1e-9);
    assert!((p[1].x - 80.0).abs() < 1e-9 && (p[1].y - 30.0).abs() < 1e-9);
}

fn us(points: &[[f64; 2]]) -> inkapp_harness::fixtures::UnitStroke {
    inkapp_harness::fixtures::UnitStroke {
        points: points.to_vec(),
    }
}
