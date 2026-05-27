//! Layer-4 lens parity: `inkctl page describe` returns the same region set
//! that the library's `recover_regions` produces on the same compiled smoke doc.
//!
//! The smoke app (`inkctl document publish <did> smoke`) calls
//! `inkapp_harness::tests_common::single_region_app("smoke")`, which compiles
//! a Typst doc, calls `recover_regions`, and persists the manifest as JSON.
//! `inkctl page describe` reads that JSON back. This test verifies the round-
//! trip is lossless: CLI-side regions ↔ library-side manifest regions agree on
//! name and rect (within 0.01pt), with set equality.

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
fn layer4_page_describe_matches_recovered_regions() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();

    // Session + device + smoke doc via CLI.
    let session = run(home, None, &["session", "new"]);
    let sid = session["data"]["session_id"].as_str().unwrap().to_string();

    let device = run(home, Some(&sid), &["device", "new"]);
    let did = device["data"]["device_id"].as_str().unwrap().to_string();

    let doc = run(home, Some(&sid), &["document", "publish", &did, "smoke"]);
    let doc_id = doc["data"]["doc_id"].as_str().unwrap().to_string();

    // CLI lens: page describe.
    let described = run(home, Some(&sid), &["page", "describe", &doc_id, "0"]);
    assert_eq!(described["ok"], true, "page describe failed: {described}");
    let cli_regions = described["data"]["regions"]
        .as_array()
        .expect("data.regions must be an array");

    // Library side: call the same builder the CLI's smoke registry calls.
    // `single_region_app` compiles a 200×200pt Typst doc with #region("r1")[hello],
    // runs recover_regions, and returns the resulting manifest inside PublishedApp.
    let published = inkapp_harness::tests_common::single_region_app("smoke");
    let lib_page0: Vec<&inkapp_core::manifest::Region> = published
        .manifest
        .regions
        .iter()
        .filter(|r| r.page == 0)
        .collect();

    // Set equality on count.
    assert_eq!(
        cli_regions.len(),
        lib_page0.len(),
        "region count mismatch — CLI={} lib={};\n  CLI names: {:?}\n  lib names: {:?}",
        cli_regions.len(),
        lib_page0.len(),
        cli_regions
            .iter()
            .map(|r| r["name"].as_str())
            .collect::<Vec<_>>(),
        lib_page0.iter().map(|r| &r.name).collect::<Vec<_>>()
    );

    // Every CLI region has a matching library region (same name, same rect within 0.01pt).
    for c in cli_regions {
        let name = c["name"].as_str().unwrap();
        let l = lib_page0
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "CLI region '{name}' has no library counterpart;\n  lib names: {:?}",
                    lib_page0.iter().map(|r| &r.name).collect::<Vec<_>>()
                )
            });

        // rect is serialized as [x0, y0, x1, y1] (confirmed in Layer 3).
        let arr = c["rect"]
            .as_array()
            .expect("rect should be a 4-element JSON array [x0, y0, x1, y1]");
        let cx0 = arr[0].as_f64().unwrap();
        let cy0 = arr[1].as_f64().unwrap();
        let cx1 = arr[2].as_f64().unwrap();
        let cy1 = arr[3].as_f64().unwrap();

        let tol = 0.01;
        assert!(
            (cx0 - l.rect.x0).abs() < tol,
            "{name} x0: CLI={cx0:.4} lib={:.4} (diff={:.6})",
            l.rect.x0,
            (cx0 - l.rect.x0).abs()
        );
        assert!(
            (cy0 - l.rect.y0).abs() < tol,
            "{name} y0: CLI={cy0:.4} lib={:.4} (diff={:.6})",
            l.rect.y0,
            (cy0 - l.rect.y0).abs()
        );
        assert!(
            (cx1 - l.rect.x1).abs() < tol,
            "{name} x1: CLI={cx1:.4} lib={:.4} (diff={:.6})",
            l.rect.x1,
            (cx1 - l.rect.x1).abs()
        );
        assert!(
            (cy1 - l.rect.y1).abs() < tol,
            "{name} y1: CLI={cy1:.4} lib={:.4} (diff={:.6})",
            l.rect.y1,
            (cy1 - l.rect.y1).abs()
        );
    }

    // Reverse direction: every library region also appears in CLI output.
    for l in &lib_page0 {
        let name = &l.name;
        let found = cli_regions.iter().any(|c| c["name"].as_str() == Some(name));
        assert!(
            found,
            "lib region '{name}' has no CLI counterpart;\n  CLI names: {:?}",
            cli_regions
                .iter()
                .map(|c| c["name"].as_str())
                .collect::<Vec<_>>()
        );
    }
}
