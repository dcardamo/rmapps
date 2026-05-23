mod common;

use std::path::Path;

use inkapp_core::device::Device;
use inkapp_core::geometry::{DevicePoint, PdfPoint};
use inkapp_harness::recording::{calibration_points, fit_scale, synth_calibration, PAGE_H};
use inkapp_remarkable::Remarkable;
use rm_files::Scene;

use common::open_rmdoc;

/// Max acceptable per-point error (device px) before adoption is required.
const TOL: f64 = 2.0;

fn tap_centroids(rm: &[u8]) -> Vec<DevicePoint> {
    Scene::parse(rm)
        .unwrap()
        .strokes()
        .into_iter()
        .map(|s| {
            let n = s.points.len().max(1) as f64;
            let (sx, sy) = s
                .points
                .iter()
                .fold((0.0, 0.0), |(ax, ay), p| (ax + p.x as f64, ay + p.y as f64));
            DevicePoint {
                x: sx / n,
                y: sy / n,
            }
        })
        .collect()
}

fn dist2(a: &PdfPoint, b: &PdfPoint) -> f64 {
    (a.x - b.x).powi(2) + (a.y - b.y).powi(2)
}

#[test]
fn transform_matches_calibration_within_tolerance() {
    let device = Remarkable::new();

    let real = format!(
        "{}/tests/fixtures/recordings/calibration.rmdoc",
        env!("CARGO_MANIFEST_DIR")
    );
    let (_pdf, rm) = if Path::new(&real).exists() {
        open_rmdoc(Path::new(&real))
    } else {
        synth_calibration(&device).unwrap()
    };

    let known = calibration_points();
    let actual = tap_centroids(&rm);
    assert_eq!(actual.len(), known.len(), "one tap per cross");

    let mut expected_dev = Vec::new();
    let mut actual_dev = Vec::new();
    let mut max_err: f64 = 0.0;
    for a in &actual {
        let a_pdf = device.device_to_pdf(*a, PAGE_H);
        let k = known
            .iter()
            .min_by(|p, q| dist2(p, &a_pdf).partial_cmp(&dist2(q, &a_pdf)).unwrap())
            .unwrap();
        let predicted = device.pdf_to_device(*k, PAGE_H);
        let err = ((predicted.x - a.x).powi(2) + (predicted.y - a.y).powi(2)).sqrt();
        max_err = max_err.max(err);
        expected_dev.push(predicted);
        actual_dev.push(*a);
    }

    if max_err >= TOL {
        let suggested = fit_scale(&expected_dev, &actual_dev);
        panic!(
            "transform error {max_err:.2}px exceeds tolerance {TOL}px. Gated adoption: \
             refit and adopt constants in inkapp-remarkable (suggested uniform scale x{suggested:.4}), \
             regenerate goldens, record provenance, then re-run."
        );
    }
}
