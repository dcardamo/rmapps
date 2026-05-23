mod common;

use inkapp_core::device::Device;
use inkapp_core::geometry::PdfPoint;
use inkapp_core::ink::Stroke;
use inkapp_harness::recording::{catalog, render_template, PAGE_H};
use inkapp_remarkable::Remarkable;

use common::{rmapi_mkdir, rmapi_put};

const ACCEPTANCE_FOLDER: &str = "/InkAppDev/acceptance";

/// Write a known stroke via `write_ink`, push the corresponding template PDF to
/// `/InkAppDev/acceptance/`, then print instructions for the eyeball check.
///
/// This bar confirms the full PDF→device→PDF round-trip on real hardware: push
/// the template, sideload the generated `.rm` into the page's annotation bundle,
/// open on the device, and verify the horizontal line at y=500 appears correctly.
#[test]
#[ignore = "requires a paired reMarkable; run: cargo test -p inkapp-harness --test acceptance writes_and_pushes_rm -- --ignored --nocapture"]
fn writes_and_pushes_rm() {
    let device = Remarkable::new();
    let entry = catalog().iter().find(|e| e.name == "checkmark").unwrap();
    let pdf = render_template(entry).unwrap();

    let stroke = Stroke {
        points: vec![
            PdfPoint { x: 40.0, y: 500.0 },
            PdfPoint { x: 380.0, y: 500.0 },
        ],
        highlighter: false,
    };
    let rm = device.write_ink(&[stroke], PAGE_H).unwrap();
    // Write to a persistent path (not the tempdir) so the operator can sideload it manually.
    let rm_path = std::env::temp_dir().join("inkapp-acceptance.rm");
    std::fs::write(&rm_path, &rm).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("acceptance.pdf");
    std::fs::write(&path, &pdf).unwrap();

    rmapi_mkdir("/InkAppDev");
    rmapi_mkdir(ACCEPTANCE_FOLDER);
    rmapi_put(&path, ACCEPTANCE_FOLDER);
    // NOTE: the sideload step below is a MANUAL operator action — this test cannot do it
    // automatically because rmapi content-only push carries the PDF only, not .rm bundles.
    eprintln!(
        "pushed acceptance PDF to {ACCEPTANCE_FOLDER}; wrote the framework-generated .rm to {}.\n\
         content-only push carries the PDF only — to verify the WRITTEN .rm renders, MANUALLY \
         sideload that .rm into the page's annotation bundle and confirm the horizontal line at \
         y=500 appears. See spec section H#3.",
        rm_path.display()
    );
}
