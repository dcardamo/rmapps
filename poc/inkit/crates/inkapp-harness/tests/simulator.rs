use inkapp_core::manifest::recover_regions;
use inkapp_core::region_metadata;
use inkapp_core::render::compile_to_document;
use inkapp_harness::simulator::{simulate, Gesture, Scenario};
use rm_device::Remarkable;

#[test]
fn stroke_in_region_is_read_back() {
    // Render a 200pt page with one region "done" at (20,40,16,16) Typst-space.
    let body = region_metadata("done", 0, 20.0, 40.0, 16.0, 16.0);
    let src = format!("#set page(width: 200pt, height: 200pt, margin: 0pt)\n{body}");

    let doc = compile_to_document(&src).unwrap();
    let manifest = recover_regions(&doc).unwrap().with_version(1);

    let device = Remarkable::new();
    let scenario = Scenario::new().mark("done", Gesture::Tap);

    let trace = simulate(&src, &manifest, &device, &scenario).expect("simulate");

    let done = trace.readback.iter().find(|ri| ri.region == "done");
    assert!(done.is_some(), "the 'done' region received ink");
    assert!(
        !trace.inspector_png.is_empty(),
        "an inspector image was produced"
    );
}
