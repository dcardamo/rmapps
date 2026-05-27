//! Layer-3 lens parity: `inkctl ink list --by-region` must group strokes the
//! same way the library's `readback::attribute` does. Specifically:
//!   - any-point-in-rect (not midpoint-only)
//!   - multi-region attribution (a stroke can appear in two region buckets)

use assert_cmd::Command;
use serde_json::Value;

fn run(home: &std::path::Path, sess: Option<&str>, args: &[&str]) -> Value {
    let mut cmd = Command::cargo_bin("inkctl").unwrap();
    cmd.env("INKCTL_HOME", home);
    if let Some(s) = sess {
        cmd.env("INKCTL_SESSION", s);
    }
    let out = cmd.args(args).output().unwrap();
    serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
        panic!(
            "not JSON from inkctl {args:?}:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

#[test]
fn layer3_by_region_matches_attribute() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();

    // Session + device + smoke doc.
    let session = run(home, None, &["session", "new"]);
    let sid = session["data"]["session_id"].as_str().unwrap().to_string();

    let device = run(home, Some(&sid), &["device", "new"]);
    let did = device["data"]["device_id"].as_str().unwrap().to_string();

    let doc = run(home, Some(&sid), &["document", "publish", &did, "smoke"]);
    let doc_id = doc["data"]["doc_id"].as_str().unwrap().to_string();

    // Describe page 0; pick the first region.
    let described = run(home, Some(&sid), &["page", "describe", &doc_id, "0"]);
    assert_eq!(described["ok"], true, "page describe failed: {described}");
    let regions = described["data"]["regions"].as_array().expect("regions");
    assert!(
        !regions.is_empty(),
        "smoke app should publish ≥1 region; got: {described}"
    );
    let r = &regions[0];
    let r_name = r["name"].as_str().unwrap().to_string();

    // rect is serialized as [x0, y0, x1, y1] array.
    let rect = r["rect"].as_array().expect("rect array");
    let x0 = rect[0].as_f64().unwrap();
    let y0 = rect[1].as_f64().unwrap();
    let x1 = rect[2].as_f64().unwrap();
    let y1 = rect[3].as_f64().unwrap();
    let cx = (x0 + x1) / 2.0;
    let cy = (y0 + y1) / 2.0;

    eprintln!("Region '{r_name}' rect: [{x0}, {y0}, {x1}, {y1}]  center: ({cx}, {cy})");

    // Pathological stroke A: first and last points INSIDE region, midpoint WAY OUTSIDE.
    // The buggy lens (midpoint-only) uses index len/2 = index 1 (the far-outside point)
    // and misses this stroke. The library's any-point check catches it.
    //
    // Stroke: inside → far outside → inside+epsilon
    // Format: "x0,y0 x1,y1 x2,y2" (space-separated x,y tokens)
    let far_x = x1 + 500.0; // well outside the page (200pt wide)
    let far_y = y1 + 500.0;
    let path_a = format!("{cx},{cy} {far_x},{far_y} {cx},{}", cy + 0.5);
    run(
        home,
        Some(&sid),
        &[
            "ink", "--device", &did, "draw", &doc_id, "0", "--path", &path_a,
        ],
    );

    let listed = run(
        home,
        Some(&sid),
        &["ink", "list", &doc_id, "0", "--by-region"],
    );
    assert_eq!(listed["ok"], true, "ink list failed: {listed}");
    let by_region = listed["data"]["by_region"]
        .as_object()
        .expect("by_region object");
    let count_in_r = by_region
        .get(&r_name)
        .map(|v| v.as_array().map(|a| a.len()).unwrap_or(0))
        .unwrap_or(0);
    assert_eq!(
        count_in_r,
        1,
        "lens missed stroke A whose endpoints are inside region '{r_name}' but midpoint \
         ({far_x},{far_y}) is outside; by_region keys: {:?}",
        by_region.keys().collect::<Vec<_>>()
    );

    // Pathological stroke B: midpoint INSIDE, endpoints OUTSIDE. Both the library
    // and the buggy midpoint-only lens attribute this — confirm baseline still works
    // after the fix.
    let path_b = format!("{far_x},{far_y} {cx},{cy} {far_x},{}", far_y + 1.0);
    run(
        home,
        Some(&sid),
        &[
            "ink", "--device", &did, "draw", &doc_id, "0", "--path", &path_b,
        ],
    );

    let listed2 = run(
        home,
        Some(&sid),
        &["ink", "list", &doc_id, "0", "--by-region"],
    );
    assert_eq!(listed2["ok"], true, "ink list 2 failed: {listed2}");
    let count2 = listed2["data"]["by_region"]
        .as_object()
        .unwrap()
        .get(&r_name)
        .map(|v| v.as_array().map(|a| a.len()).unwrap_or(0))
        .unwrap_or(0);
    assert_eq!(
        count2, 2,
        "second stroke B (midpoint-inside) should also attribute to '{r_name}'; \
         by_region: {:?}",
        listed2["data"]["by_region"]
    );
}
