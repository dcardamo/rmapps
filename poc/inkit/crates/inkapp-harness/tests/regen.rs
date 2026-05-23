mod common;

use std::path::Path;

use inkapp_harness::recording::catalog;
use inkapp_remarkable::Remarkable;

use common::regen_fixture;

/// Each committed `gestures/<name>.json` must byte-equal the fixture regenerated
/// from its source (a real `recordings/<name>.rmdoc` if present, else synthetic
/// bootstrap strokes). The comparison is on the canonical `to_json()` string:
/// the committed file IS that serialization and the pipeline is deterministic,
/// so this is an exact golden-file check. (Comparing parsed structs instead
/// would spuriously fail by ~1 ULP — serde_json's f64 serialize/parse is not
/// perfectly symmetric — even though nothing actually changed.)
#[test]
fn fixtures_match_regenerated() {
    let device = Remarkable::new();
    let dir = format!("{}/tests/fixtures/gestures", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dir).unwrap();

    let mut wrote_any = false;
    for entry in catalog() {
        let regenerated = regen_fixture(entry, &device).to_json().unwrap();
        let path = format!("{dir}/{}.json", entry.name);

        match std::fs::read_to_string(&path) {
            Ok(committed) => {
                assert_eq!(
                    committed, regenerated,
                    "fixture {} differs from regenerated; re-run after reviewing the change",
                    entry.name
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::write(&path, regenerated).unwrap();
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
