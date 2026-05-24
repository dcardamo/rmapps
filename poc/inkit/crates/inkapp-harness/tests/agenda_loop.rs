use std::collections::HashMap;

mod common;

use agenda::{update, view, App, Connectors, Msg};
use inkapp_core::device::Device;
use inkapp_core::document::DocKey;
use inkapp_core::geometry::PdfRect;
use inkapp_core::ink::Stroke;
use inkapp_core::manifest::Manifest;
use inkapp_core::runtime::{app, DocSet};
use inkapp_harness::fixtures::GestureFixture;
use inkapp_remarkable::Remarkable;

fn fixture(name: &str) -> GestureFixture {
    let path = format!(
        "{}/tests/fixtures/gestures/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    GestureFixture::from_json(&bytes).unwrap()
}

fn region_rect(m: &Manifest, name: &str) -> PdfRect {
    m.regions
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("region {name:?} not found in manifest"))
        .rect
}

/// Transplant `fix` into `rect`, then route through the device write/read path
/// so the test exercises the real .rm byte path.
fn device_ink(device: &Remarkable, fix: &GestureFixture, rect: PdfRect, page_h: f64) -> Vec<Stroke> {
    let pdf = fix.transplant_default(rect);
    let bytes = device.write_ink(&pdf, page_h).unwrap();
    device.read_ink(&bytes, page_h).unwrap()
}

#[tokio::test]
async fn agenda_cancel_marks_editable_event_only() {
    let device = Remarkable::new();
    let mut application = app(App)
        .connector(Connectors::fake())
        .update(update)
        .view(view)
        .key(common::test_key())
        .build();
    let mut set = DocSet::default();

    // Cycle 0: render the agenda document.
    let rendered = application.render(&mut set).await.unwrap();
    assert_eq!(rendered.len(), 1, "one agenda document");

    let key = DocKey::new("agenda");
    let manifest = set.manifest(&key).unwrap().clone();
    let page_h = set.page_h(&key).unwrap();

    // The editable (local) calendar mints evt-0/evt-1; the read-only feed mints
    // none — so every region in the manifest belongs to the editable calendar.
    assert!(manifest.regions.iter().any(|r| r.name == "evt-0"));
    assert!(manifest.regions.iter().any(|r| r.name == "evt-1"));
    assert!(
        manifest.regions.iter().all(|r| r.name.starts_with("evt-")),
        "read-only feed contributes no editable regions: {:?}",
        manifest.regions.iter().map(|r| &r.name).collect::<Vec<_>>()
    );

    // Mark the first editable event (evt-0 -> localcal uid "mine-1").
    let check = fixture("checkmark");
    let mut ink: HashMap<String, Vec<Stroke>> = HashMap::new();
    ink.insert(
        key.0.clone(),
        device_ink(&device, &check, region_rect(&manifest, "evt-0"), page_h),
    );

    // Cycle 1: step.
    let cycle = application.step(&mut set, &ink).await.unwrap();

    assert!(
        cycle.decoded.contains(&Msg::EventCancelled { uid: "mine-1".to_string() }),
        "decoded a cancel for the editable event: {:?}",
        cycle.decoded
    );

    // The writable calendar recorded the cancel.
    assert!(
        application
            .connectors
            .cal
            .events()
            .iter()
            .find(|e| e.uid == "mine-1")
            .unwrap()
            .cancelled,
        "mine-1 is cancelled on the local calendar"
    );
}
