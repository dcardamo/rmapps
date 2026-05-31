mod common;

use inkapp_core::embed::extract_manifest;
use inkapp_harness::recording::{
    calibration_points, catalog, render_calibration, render_template, BOXES_PER_GESTURE,
};

#[test]
fn catalog_has_seven_gestures() {
    let names: Vec<&str> = catalog().iter().map(|e| e.name).collect();
    assert!(names.contains(&"checkmark"));
    assert!(names.contains(&"scribble-out"));
    assert!(names.contains(&"highlight-swipe"));
    assert_eq!(names.len(), 7);
}

#[test]
fn template_declares_three_boxes() {
    let key = common::test_key();
    let entry = catalog().iter().find(|e| e.name == "checkmark").unwrap();
    let pdf = render_template(entry, &key).unwrap();
    let manifest = extract_manifest(&pdf, &key).unwrap();
    for i in 0..BOXES_PER_GESTURE {
        let name = format!("box:checkmark:{i}");
        assert!(
            manifest.regions.iter().any(|r| r.name == name),
            "missing region {name}"
        );
    }
}

#[test]
fn calibration_declares_crosses_with_known_points() {
    let key = common::test_key();
    let pdf = render_calibration(&key).unwrap();
    let manifest = extract_manifest(&pdf, &key).unwrap();
    let pts = calibration_points();
    assert!(pts.len() >= 4, "at least 4 calibration points");
    for i in 0..pts.len() {
        assert!(
            manifest
                .regions
                .iter()
                .any(|r| r.name == format!("cross:{i}")),
            "missing cross:{i}"
        );
    }
}
