//! Layer-2 lens parity: `inkctl ink load-rm` followed by `inkctl ink list`
//! must report the same strokes that `Remarkable::read_ink` produces on the
//! same fixture bytes, at the same page height as stored in the session PDF.

use assert_cmd::Command;
use serde_json::Value;

const CALIBRATION: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../inkapp-harness/tests/fixtures/recordings/calibration.rmdoc"
);

fn run(home: &std::path::Path, sess: Option<&str>, args: &[&str]) -> Value {
    let mut cmd = Command::cargo_bin("inkctl").unwrap();
    cmd.env("INKCTL_HOME", home);
    if let Some(s) = sess {
        cmd.env("INKCTL_SESSION", s);
    }
    let out = cmd.args(args).output().unwrap();
    serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
        panic!(
            "not JSON from inkctl {:?}:\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

/// Extract page 0 height (in PDF points) from a PDF file via lopdf MediaBox.
/// Falls back to 560.0 on parse errors. Mirrors `pdf_page_height` in session.rs.
fn read_pdf_page_height(pdf_path: &std::path::Path) -> f64 {
    let bytes = match std::fs::read(pdf_path) {
        Ok(b) => b,
        Err(_) => return 560.0,
    };
    let doc = match lopdf::Document::load_mem(&bytes) {
        Ok(d) => d,
        Err(_) => return 560.0,
    };
    let pages = doc.get_pages();
    let Some(page_id) = pages.get(&1) else {
        return 560.0;
    };
    let page_obj = match doc.get_object(*page_id) {
        Ok(lopdf::Object::Dictionary(d)) => d.clone(),
        _ => return 560.0,
    };
    let media_box = match page_obj.get(b"MediaBox") {
        Ok(mb) => mb,
        Err(_) => return 560.0,
    };
    let arr = match doc.dereference(media_box) {
        Ok((_, lopdf::Object::Array(a))) => a.clone(),
        _ => return 560.0,
    };
    if arr.len() < 4 {
        return 560.0;
    }
    let y0 = arr[1].as_float().unwrap_or(0.0) as f64;
    let y1 = arr[3].as_float().unwrap_or(560.0) as f64;
    (y1 - y0).max(1.0)
}

#[test]
fn layer2_lens_matches_library() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();

    // Spin up session + device + smoke doc.
    let session = run(home, None, &["session", "new"]);
    let sid = session["data"]["session_id"].as_str().unwrap().to_string();

    let device = run(home, Some(&sid), &["device", "new"]);
    let did = device["data"]["device_id"].as_str().unwrap().to_string();

    let doc = run(home, Some(&sid), &["document", "publish", &did, "smoke"]);
    let doc_id = doc["data"]["doc_id"].as_str().unwrap().to_string();

    // Apply the calibration fixture (page 0) via inkctl ink load-rm.
    let applied = run(
        home,
        Some(&sid),
        &[
            "ink",
            "--device",
            &did,
            "load-rm",
            &doc_id,
            "0",
            "--path",
            CALIBRATION,
        ],
    );
    assert_eq!(applied["ok"], true, "load-rm failed: {applied}");
    let n_cli = applied["data"]["applied"].as_u64().unwrap() as usize;

    // Read page height from the stored PDF — same source as the session's pdf_page_height.
    let pdf_path = home.join(&sid).join("docs").join(&doc_id).join("pdf.pdf");
    let page_h = read_pdf_page_height(&pdf_path);

    // Decode the same fixture via the library at the same page height.
    let bundle = rm_files::Bundle::open(std::path::Path::new(CALIBRATION)).expect("open bundle");
    let pages = bundle.pages();
    let scene_bytes = pages
        .first()
        .and_then(|p| p.scene_bytes())
        .expect("first page scene bytes")
        .to_vec();
    let rm = rm_device::Remarkable::new();
    use inkapp_core::device::Device as _;
    let lib_strokes = rm.read_ink(&scene_bytes, page_h).expect("library read_ink");

    assert_eq!(
        n_cli,
        lib_strokes.len(),
        "stroke count mismatch: CLI reported {n_cli}, library decoded {}",
        lib_strokes.len()
    );

    // Fetch strokes via inkctl ink list and compare point-by-point.
    let listed = run(home, Some(&sid), &["ink", "list", &doc_id, "0"]);
    assert_eq!(listed["ok"], true, "ink list failed: {listed}");

    let cli_strokes = listed["data"]["strokes"].as_array().expect("strokes array");
    assert_eq!(
        cli_strokes.len(),
        lib_strokes.len(),
        "list stroke count mismatch"
    );

    for (i, (cli, lib)) in cli_strokes.iter().zip(&lib_strokes).enumerate() {
        let cli_pts = cli["points"].as_array().expect("points array");
        assert_eq!(
            cli_pts.len(),
            lib.points.len(),
            "stroke {i} point count mismatch"
        );
        for (j, (cp, lp)) in cli_pts.iter().zip(&lib.points).enumerate() {
            let cx = cp["x"].as_f64().unwrap();
            let cy = cp["y"].as_f64().unwrap();
            assert!(
                (cx - lp.x).abs() < 1e-4 && (cy - lp.y).abs() < 1e-4,
                "stroke {i} point {j}: CLI=({cx:.6},{cy:.6}) lib=({:.6},{:.6})",
                lp.x,
                lp.y
            );
        }
    }
}
