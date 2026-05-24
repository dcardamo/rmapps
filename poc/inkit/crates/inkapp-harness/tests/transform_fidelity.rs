mod common;

use std::path::Path;

use inkapp_core::device::Device;
use inkapp_core::geometry::DevicePoint;
use inkapp_harness::recording::{calibration_points, fit_scale, synth_calibration, PAGE_H};
use inkapp_remarkable::Remarkable;
use rm_files::Scene;

use common::open_rmdoc;

/// Max acceptable per-point error (device px) before adoption is required.
///
/// Sized for *real* on-device calibration captures, whose floor is human tap
/// jitter (~3-4px on a cross). 8px sits ~2x above that jitter yet ~12x below the
/// systematic-error class this test exists to catch (e.g. a 6% transform-scale
/// drift is ~100px at the page corners). A synthetic capture (no recording
/// present) lands at ~0px and clears this trivially.
const TOL: f64 = 8.0;

fn tap_centroids(rm: &[u8]) -> Vec<DevicePoint> {
    Scene::parse(rm)
        .unwrap()
        .strokes()
        .into_iter()
        .map(|s| {
            assert!(
                !s.points.is_empty(),
                "stroke with no points in calibration capture"
            );
            let n = s.points.len() as f64;
            let (sx, sy) = s
                .points
                .iter()
                // f32 → f64 widening; .rm stores device coords as f32
                .fold((0.0, 0.0), |(ax, ay), p| (ax + p.x as f64, ay + p.y as f64));
            DevicePoint {
                x: sx / n,
                y: sy / n,
            }
        })
        .collect()
}

#[test]
fn transform_matches_calibration_within_tolerance() {
    let device = Remarkable::new();
    let key = common::test_key();

    let real = format!(
        "{}/tests/fixtures/recordings/calibration.rmdoc",
        env!("CARGO_MANIFEST_DIR")
    );
    let (_pdf, rm) = if Path::new(&real).exists() {
        open_rmdoc(Path::new(&real))
    } else {
        synth_calibration(&device, &key).unwrap()
    };

    let known = calibration_points();
    // Precompute where each known PDF point lands in device space under the model.
    let predicted: Vec<DevicePoint> = known
        .iter()
        .map(|k| device.pdf_to_device(*k, PAGE_H))
        .collect();
    let actual = tap_centroids(&rm);
    assert_eq!(actual.len(), predicted.len(), "one tap per cross");

    let dist =
        |a: &DevicePoint, b: &DevicePoint| ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();

    let mut expected_dev = Vec::new();
    let mut actual_dev = Vec::new();
    let mut max_err: f64 = 0.0;
    for a in &actual {
        // Pair each recorded tap to the nearest predicted device point (device-space
        // Euclidean distance), avoiding the inverse transform on a possibly miscalibrated device.
        let p = predicted
            .iter()
            .min_by(|x, y| dist(a, x).partial_cmp(&dist(a, y)).unwrap())
            .unwrap();
        max_err = max_err.max(dist(a, p));
        expected_dev.push(*p);
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

#[test]
fn fit_scale_recovers_known_scale() {
    let base = vec![
        DevicePoint { x: 100.0, y: 200.0 },
        DevicePoint { x: 300.0, y: 0.0 },
    ];
    let scaled: Vec<DevicePoint> = base
        .iter()
        .map(|p| DevicePoint {
            x: p.x * 2.0,
            y: p.y * 2.0,
        })
        .collect();
    // fit_scale(expected, actual): expected = model prediction (base), actual = observed (scaled).
    let s = fit_scale(&base, &scaled);
    assert!((s - 2.0).abs() < 1e-10, "expected 2.0, got {s}");
}
