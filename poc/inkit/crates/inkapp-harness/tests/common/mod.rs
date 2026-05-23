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
