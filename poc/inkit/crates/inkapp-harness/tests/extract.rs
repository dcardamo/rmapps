use inkapp_core::crypto::Key;
use inkapp_core::device::Device;
use inkapp_core::embed::extract_manifest;
use inkapp_harness::fixtures::Tool;
use inkapp_harness::recording::{
    bootstrap_strokes, catalog, extract_fixture, render_template, Source, BOXES_PER_GESTURE, PAGE_H,
};
use inkapp_remarkable::Remarkable;

#[test]
fn bootstrap_round_trip_yields_one_sample_per_box() {
    let key = Key::from_bytes([42u8; 32]);
    let entry = catalog().iter().find(|e| e.name == "checkmark").unwrap();
    let pdf = render_template(entry, &key).unwrap();
    let manifest = extract_manifest(&pdf, &key).unwrap();
    let device = Remarkable::new();

    let synth = bootstrap_strokes(entry, &manifest);
    let bytes = device.write_ink(&synth, PAGE_H).unwrap();
    let strokes_pdf = device.read_ink(&bytes, PAGE_H).unwrap();

    let source = Source {
        recording: "synthetic".into(),
        device: "synthetic-bootstrap".into(),
        recorded: "2026-05-23".into(),
    };
    let fixture = extract_fixture(entry, &strokes_pdf, &manifest, source);

    assert_eq!(fixture.name, "checkmark");
    assert_eq!(fixture.tool, Tool::Pen);
    assert_eq!(fixture.samples.len(), BOXES_PER_GESTURE);
    for s in &fixture.samples {
        assert!(!s.strokes.is_empty(), "each box has ink");
        for st in &s.strokes {
            for p in &st.points {
                assert!((-0.001..=1.001).contains(&p[0]) && (-0.001..=1.001).contains(&p[1]));
            }
        }
    }
}
