mod common;

use std::path::Path;

use inkapp_harness::fixtures::{GestureFixture, Sample};
use inkapp_harness::recording::catalog;
use inkapp_remarkable::Remarkable;

use common::regen_fixture;

/// Maximum allowable per-coordinate deviation between committed and regenerated
/// fixtures. The device codec quantizes to fixed-point; sub-ULP variance is
/// expected and acceptable. 1e-6 is well above codec noise (~1e-9) but far
/// below any perceptible geometric difference.
const FIXTURE_EPSILON: f64 = 1e-6;

/// Returns true if two samples are equal within FIXTURE_EPSILON on all coordinates.
fn samples_approx_eq(a: &Sample, b: &Sample) -> bool {
    if (a.native_aspect - b.native_aspect).abs() > FIXTURE_EPSILON {
        return false;
    }
    if a.strokes.len() != b.strokes.len() {
        return false;
    }
    for (sa, sb) in a.strokes.iter().zip(b.strokes.iter()) {
        if sa.points.len() != sb.points.len() {
            return false;
        }
        for (pa, pb) in sa.points.iter().zip(sb.points.iter()) {
            if (pa[0] - pb[0]).abs() > FIXTURE_EPSILON || (pa[1] - pb[1]).abs() > FIXTURE_EPSILON {
                return false;
            }
        }
    }
    true
}

/// Returns true if two GestureFixtures are equal within FIXTURE_EPSILON on
/// all f64 fields; all other fields are compared exactly.
fn fixtures_approx_eq(a: &GestureFixture, b: &GestureFixture) -> bool {
    a.name == b.name
        && a.tool == b.tool
        && a.fit == b.fit
        && a.default == b.default
        && a.source == b.source
        && a.samples.len() == b.samples.len()
        && a.samples
            .iter()
            .zip(b.samples.iter())
            .all(|(sa, sb)| samples_approx_eq(sa, sb))
}

#[test]
fn fixtures_match_regenerated() {
    let device = Remarkable::new();
    let dir = format!("{}/tests/fixtures/gestures", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dir).unwrap();

    let mut wrote_any = false;
    for entry in catalog() {
        let fixture = regen_fixture(entry, &device);
        let json = fixture.to_json().unwrap();
        let path = format!("{dir}/{}.json", entry.name);

        match std::fs::read_to_string(&path) {
            Ok(existing) => {
                let committed: GestureFixture =
                    GestureFixture::from_json(existing.as_bytes()).unwrap();
                assert!(
                    fixtures_approx_eq(&committed, &fixture),
                    "fixture {} differs from regenerated (within epsilon {})\ncommitted:    {:?}\nregenerated:  {:?}",
                    entry.name,
                    FIXTURE_EPSILON,
                    committed,
                    fixture,
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::write(&path, json).unwrap();
                wrote_any = true;
            }
            Err(e) => panic!("read {path}: {e}"),
        }
    }
    assert!(
        !wrote_any,
        "wrote missing bootstrap fixtures; review and re-run"
    );
    for entry in catalog() {
        assert!(Path::new(&format!("{dir}/{}.json", entry.name)).exists());
    }
}
