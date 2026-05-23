mod common;

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
    // Guard against orphaned/missing fixtures: the set of committed gestures/*.json
    // must exactly match the catalog (catches a fixture left behind after a gesture
    // is renamed or removed).
    use std::collections::BTreeSet;
    let committed: BTreeSet<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".json"))
        .map(|n| n.trim_end_matches(".json").to_string())
        .collect();
    let expected: BTreeSet<String> = catalog().iter().map(|e| e.name.to_string()).collect();
    assert_eq!(
        committed, expected,
        "committed fixtures do not match the catalog"
    );
}
