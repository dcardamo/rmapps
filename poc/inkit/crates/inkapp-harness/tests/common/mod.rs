#![allow(dead_code)]

use std::io::Read;
use std::path::Path;

use inkapp_core::device::Device;
use inkapp_core::embed::extract_manifest;
use inkapp_harness::fixtures::Source;
use inkapp_harness::recording::{
    bootstrap_strokes, extract_fixture, render_template, CatalogEntry, PAGE_H,
};

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
        let manifest = extract_manifest(&pdf).unwrap();
        let strokes = device.read_ink(&rm, PAGE_H).unwrap();
        let source = Source {
            recording: format!("recordings/{}.rmdoc", entry.name),
            device: "reMarkable Paper Pro Move".into(),
            recorded: "recorded".into(),
        };
        extract_fixture(entry, &strokes, &manifest, source)
    } else {
        let pdf = render_template(entry).unwrap();
        let manifest = extract_manifest(&pdf).unwrap();
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
