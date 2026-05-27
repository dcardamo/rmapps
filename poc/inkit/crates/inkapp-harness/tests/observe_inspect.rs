//! Integration test for `observe::page_inspect`: ensure the default opts produce a
//! valid PNG with region overlays drawn, and that toggling `show.regions = false`
//! actually removes the blue rect outlines.

use inkapp_harness::inspector::InspectOpts;
use inkapp_harness::observe;
use inkapp_harness::session::Session;
use inkapp_harness::tests_common::single_region_app;
use tempfile::tempdir;

/// Count pixels that look like the inspector's region-outline blue
/// (Rgba [0, 80, 220, 255]). Loose thresholds so anti-aliasing/PNG round-trip
/// don't break the assertion.
fn count_blue_ish(png: &[u8]) -> usize {
    let img = image::load_from_memory(png).unwrap().to_rgba8();
    img.pixels()
        .filter(|p| p.0[2] > 200 && p.0[0] < 60 && p.0[1] < 120)
        .count()
}

#[tokio::test]
async fn page_inspect_default_shows_region_outline() {
    let dir = tempdir().unwrap();
    let mut s = Session::new_fake(dir.path()).await.unwrap();
    let dev = s.device_new(None).unwrap();
    let doc = s
        .document_publish(&dev, single_region_app("d"))
        .await
        .unwrap();

    let with_regions = observe::page_inspect(&s, &doc.id, 0, &InspectOpts::default()).unwrap();
    assert_eq!(&with_regions[0..4], &[0x89, 0x50, 0x4E, 0x47], "PNG magic");

    let mut opts = InspectOpts::default();
    opts.show.regions = false;
    let no_regions = observe::page_inspect(&s, &doc.id, 0, &opts).unwrap();

    let blue_with = count_blue_ish(&with_regions);
    let blue_without = count_blue_ish(&no_regions);
    assert!(
        blue_with > blue_without,
        "regions=false should reduce blue overlay pixels (got {blue_with} vs {blue_without})"
    );
}
