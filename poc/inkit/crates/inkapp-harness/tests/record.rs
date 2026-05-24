mod common;

use std::path::Path;

use inkapp_harness::recording::{catalog, render_calibration, render_template};

use common::{rmapi_mget, rmapi_mkdir, rmapi_put};

const FIXTURES_FOLDER: &str = "/InkAppDev/fixtures";

/// Push all gesture templates and the calibration sheet to `/InkAppDev/fixtures/`
/// on the paired reMarkable device. After drawing on each document and syncing
/// back, run `pull_recordings` to harvest the `.rmdoc` files.
#[test]
#[ignore = "requires a paired reMarkable; run: cargo test -p inkapp-harness --test record push_templates -- --ignored --nocapture"]
fn push_templates() {
    let key = common::test_key();
    rmapi_mkdir("/InkAppDev");
    rmapi_mkdir(FIXTURES_FOLDER);
    let dir = tempfile::tempdir().unwrap();

    let cal = dir.path().join("calibration.pdf");
    std::fs::write(&cal, render_calibration(&key).unwrap()).unwrap();
    rmapi_put(&cal, FIXTURES_FOLDER);

    for entry in catalog() {
        let path = dir.path().join(format!("{}.pdf", entry.name));
        std::fs::write(&path, render_template(entry, &key).unwrap()).unwrap();
        rmapi_put(&path, FIXTURES_FOLDER);
    }
    eprintln!(
        "pushed calibration sheet + templates to {FIXTURES_FOLDER}; tap the calibration crosses \
         and draw on each template, sync, then run pull_recordings"
    );
}

/// Pull `/InkAppDev/fixtures/` back from the device into
/// `tests/fixtures/recordings/` so `regen` can extract gesture fixtures.
#[test]
#[ignore = "requires a paired reMarkable; run after drawing: cargo test -p inkapp-harness --test record pull_recordings -- --ignored --nocapture"]
fn pull_recordings() {
    let dest = format!("{}/tests/fixtures/recordings", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dest).unwrap();

    // mget into a temp dir, then flatten: regen expects recordings/<name>.rmdoc,
    // but mget nests the pull under a subdir named after the remote basename.
    let tmp = tempfile::tempdir().unwrap();
    rmapi_mget(FIXTURES_FOLDER, tmp.path());

    let mut pulled = 0;
    for entry in walkdir_rmdoc(tmp.path()) {
        let to = Path::new(&dest).join(entry.file_name().unwrap());
        std::fs::copy(&entry, &to).unwrap();
        pulled += 1;
    }
    eprintln!("pulled {pulled} .rmdoc into {dest}; re-run the regen test to extract fixtures");
}

/// Collect every `*.rmdoc` file under `root` (recursive), so the pull is robust to
/// however rmapi nests the download.
fn walkdir_rmdoc(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|x| x.to_str()) == Some("rmdoc") {
                out.push(p);
            }
        }
    }
    out
}
