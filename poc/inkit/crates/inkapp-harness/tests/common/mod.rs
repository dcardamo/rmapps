#![allow(dead_code)]

use std::io::Read;
use std::path::Path;

use inkapp_core::crypto::Key;
use inkapp_core::device::Device;
use inkapp_core::embed::extract_manifest;
use inkapp_core::geometry::PdfRect;
use inkapp_core::ink::Stroke;
use inkapp_core::manifest::Manifest;
use inkapp_harness::fixtures::{GestureFixture, Source};
use inkapp_harness::recording::{
    bootstrap_strokes, extract_fixture, render_template, CatalogEntry, PAGE_H,
};
use rm_device::Remarkable;

/// Load a committed gesture fixture by name from the harness fixtures dir.
pub fn fixture(name: &str) -> GestureFixture {
    let path = format!(
        "{}/tests/fixtures/gestures/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    GestureFixture::from_json(&bytes).unwrap()
}

/// The rect of the named region in `m` (panics if absent — region names are
/// developer-chosen, so a miss is a test bug, not input data).
pub fn region_rect(m: &Manifest, name: &str) -> PdfRect {
    m.regions
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("region {name:?} not found in manifest"))
        .rect
}

/// Transplant `fix` into `rect`, then route through the device write/read path
/// so the test exercises the real .rm byte path.
pub fn device_ink(
    device: &Remarkable,
    fix: &GestureFixture,
    rect: PdfRect,
    page_h: f64,
) -> Vec<Stroke> {
    let pdf = fix.transplant_default(rect);
    let bytes = device.write_ink(&pdf, page_h).unwrap();
    device.read_ink(&bytes, page_h).unwrap()
}

/// Fixed key used across harness tests so embed and extract agree.
pub fn test_key() -> Key {
    Key::from_bytes([42u8; 32])
}

/// Read the `.pdf` and the `.rm` entry from an `.rmdoc` zip (single-page recording assumed).
pub fn open_rmdoc(path: &Path) -> (Vec<u8>, Vec<u8>) {
    let file = std::fs::File::open(path).expect("open rmdoc");
    let mut archive = zip::ZipArchive::new(file).expect("read zip");
    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .collect();
    let mut read = |suffix: &str| -> Vec<u8> {
        let n = names
            .iter()
            .find(|n| n.ends_with(suffix))
            .unwrap_or_else(|| panic!("no {suffix} entry"));
        let mut e = archive.by_name(n).unwrap();
        let mut b = Vec::new();
        e.read_to_end(&mut b).unwrap();
        b
    };
    (read(".pdf"), read(".rm"))
}

/// Compare `png` to the committed golden at `tests/golden/<name>.png`.
/// On first run (golden absent), write it and fail with a clear message so the
/// developer reviews and commits it.
///
/// Byte equality is only meaningful when the rendering stack (typst-render,
/// tiny-skia, image/zlib) is pinned to the same versions — which the Nix devshell
/// enforces. Running outside `nix develop` on a different platform may produce a
/// false mismatch; regenerate the golden inside the devshell.
pub fn assert_golden(name: &str, png: &[u8]) {
    let path = format!("{}/tests/golden/{name}.png", env!("CARGO_MANIFEST_DIR"));
    match std::fs::read(&path) {
        Ok(expected) => assert_eq!(
            png,
            expected.as_slice(),
            "inspector image differs from golden {name}"
        ),
        // Only "file not found" triggers bootstrap; any other I/O error (e.g.
        // unreadable file) is a real failure and must not silently rewrite the
        // golden — that would compare against freshly-written bytes and pass.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(format!("{}/tests/golden", env!("CARGO_MANIFEST_DIR")))
                .unwrap();
            std::fs::write(&path, png).unwrap();
            // Must panic: if we returned, the (now-equal) file written above would
            // make a same-run comparison pass with no human review.
            panic!("golden {name} did not exist; wrote it — review and re-run");
        }
        Err(e) => panic!("could not read golden {name}: {e}"),
    }
}

// ── reMarkable cloud helpers (manual on-device bars) ─────────────────────────
//
// Back the `#[ignore]`d on-device tests. They talk to the real reMarkable cloud
// via `rm-cloud` (credentials from `RM_CLOUD_DEVICE_TOKEN` / `RM_CLOUD_USER_TOKEN`),
// replacing the old `rmapi` CLI shell-outs. Each builds a short-lived runtime
// because the callers are synchronous `#[test]`s.

/// A fresh tokio runtime for one blocking cloud call.
fn cloud_rt() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().expect("build tokio runtime")
}

/// A cloud client from the environment (panics with a clear message if creds are absent).
fn cloud_client() -> rm_cloud::Client {
    rm_cloud::Client::from_env()
        .expect("RM_CLOUD_DEVICE_TOKEN or RM_CLOUD_USER_TOKEN must be set for on-device bars")
}

/// Ensure a folder path exists on the cloud (like `mkdir -p`); returns its id.
/// Unlike the old `rmapi mkdir`, this creates every missing ancestor in one call.
pub fn cloud_mkdir(folder: &str) -> String {
    cloud_rt().block_on(async { cloud_client().mkdir_p(folder).await.expect("mkdir_p") })
}

/// Push a PDF to `folder` as a document named after the file stem, preserving any
/// on-device ink (a content-only PDF-blob swap when the doc already exists; a new
/// document otherwise).
pub fn cloud_put(pdf_path: &Path, folder: &str) {
    let name = pdf_path
        .file_stem()
        .and_then(|s| s.to_str())
        .expect("utf-8 pdf stem")
        .to_string();
    let pdf = std::fs::read(pdf_path).expect("read pdf");
    cloud_rt().block_on(async {
        let client = cloud_client();
        let folder_id = client.mkdir_p(folder).await.expect("mkdir_p");
        let existing = client
            .ls(&folder_id)
            .await
            .expect("ls")
            .into_iter()
            .find(|e| !e.is_folder && e.name == name);
        match existing {
            Some(e) => client
                .put_content_only(&e.id, pdf)
                .await
                .expect("put_content_only"),
            None => client
                .put(rm_cloud::DocFiles::new_pdf(&name, &folder_id, pdf))
                .await
                .expect("put"),
        }
    });
}

/// Pull every document under `folder` into `dest_dir` as `<visibleName>.rmdoc`.
/// Replaces `rmapi mget`; the cloud has no folder-nesting quirk, so files land flat.
pub fn cloud_mget(folder: &str, dest_dir: &Path) {
    cloud_rt().block_on(async {
        let client = cloud_client();
        let folder_id = client.mkdir_p(folder).await.expect("mkdir_p");
        for entry in client.ls(&folder_id).await.expect("ls") {
            if entry.is_folder {
                continue;
            }
            let df = client.get(&entry.id).await.expect("get");
            let path = dest_dir.join(format!("{}.rmdoc", entry.name));
            df.write_rmdoc(&path).expect("write_rmdoc");
        }
    });
}

/// Regenerate a gesture fixture from its real recording if present, else from
/// synthetic bootstrap strokes (both via the real write/read path).
pub fn regen_fixture(
    entry: &CatalogEntry,
    device: &dyn Device,
) -> inkapp_harness::fixtures::GestureFixture {
    let rec_path = format!(
        "{}/tests/fixtures/recordings/{}.rmdoc",
        env!("CARGO_MANIFEST_DIR"),
        entry.name
    );
    if Path::new(&rec_path).exists() {
        let (pdf, rm) = open_rmdoc(Path::new(&rec_path));
        let manifest = extract_manifest(&pdf, &test_key()).unwrap();
        let strokes = device.read_ink(&rm, PAGE_H).unwrap();
        let source = Source {
            recording: format!("recordings/{}.rmdoc", entry.name),
            device: "reMarkable Paper Pro Move".into(),
            recorded: "recorded".into(),
        };
        extract_fixture(entry, &strokes, &manifest, source)
    } else {
        let pdf = render_template(entry, &test_key()).unwrap();
        let manifest = extract_manifest(&pdf, &test_key()).unwrap();
        let synth = bootstrap_strokes(entry, &manifest);
        let bytes = device.write_ink(&synth, PAGE_H).unwrap();
        let strokes = device.read_ink(&bytes, PAGE_H).unwrap();
        let source = Source {
            recording: "synthetic".into(),
            device: "synthetic-bootstrap".into(),
            recorded: "synthetic".into(),
        };
        extract_fixture(entry, &strokes, &manifest, source)
    }
}
