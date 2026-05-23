mod common;

use inkapp_harness::recording::{catalog, render_calibration, render_template};

use common::{rmapi_get, rmapi_mkdir, rmapi_put};

const FIXTURES_FOLDER: &str = "/InkAppDev/fixtures";

/// Push all gesture templates and the calibration sheet to `/InkAppDev/fixtures/`
/// on the paired reMarkable device. After drawing on each document and syncing
/// back, run `pull_recordings` to harvest the `.rmdoc` files.
#[test]
#[ignore = "requires a paired reMarkable; run: cargo test -p inkapp-harness --test record push_templates -- --ignored --nocapture"]
fn push_templates() {
    rmapi_mkdir("/InkAppDev");
    rmapi_mkdir(FIXTURES_FOLDER);
    let dir = tempfile::tempdir().unwrap();

    let cal = dir.path().join("calibration.pdf");
    std::fs::write(&cal, render_calibration().unwrap()).unwrap();
    rmapi_put(&cal, FIXTURES_FOLDER);

    for entry in catalog() {
        let path = dir.path().join(format!("{}.pdf", entry.name));
        std::fs::write(&path, render_template(entry).unwrap()).unwrap();
        rmapi_put(&path, FIXTURES_FOLDER);
    }
    eprintln!(
        "pushed templates to {FIXTURES_FOLDER}; draw on each, sync, then run pull_recordings"
    );
}

/// Pull `/InkAppDev/fixtures/` back from the device into
/// `tests/fixtures/recordings/` so `regen` can extract gesture fixtures.
#[test]
#[ignore = "requires a paired reMarkable; run after drawing: cargo test -p inkapp-harness --test record pull_recordings -- --ignored --nocapture"]
fn pull_recordings() {
    let dest = format!("{}/tests/fixtures/recordings", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dest).unwrap();
    rmapi_get(FIXTURES_FOLDER, std::path::Path::new(&dest));
    eprintln!("pulled {FIXTURES_FOLDER} into {dest}; re-run the regen test to extract fixtures");
}
