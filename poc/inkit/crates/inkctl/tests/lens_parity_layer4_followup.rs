//! Layer-4 follow-up lens parity: `inkctl page describe` must agree with the
//! library's recovered manifest on EVERY page of a multi-component fixture
//! whose regions come from `ActionBand`, `Section`, `Heading`, and
//! `GestureAction`.

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
fn layer4_followup_page_describe_matches_recovered_regions_all_pages() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();

    let session = run(home, None, &["session", "new"]);
    let sid = session["data"]["session_id"].as_str().unwrap().to_string();

    let device = run(home, Some(&sid), &["device", "new"]);
    let did = device["data"]["device_id"].as_str().unwrap().to_string();

    let doc = run(home, Some(&sid), &["document", "publish", &did, "multi"]);
    assert_eq!(doc["ok"], true, "publish multi failed: {doc}");
    let doc_id = doc["data"]["doc_id"].as_str().unwrap().to_string();

    let published = inkapp_harness::tests_common::multi_component_app("multi");
    let pages: std::collections::BTreeSet<usize> =
        published.manifest.regions.iter().map(|r| r.page).collect();
    assert!(
        pages.len() >= 2,
        "expected multi-page fixture; got pages: {pages:?}"
    );

    for page in &pages {
        let described = run(
            home,
            Some(&sid),
            &["page", "describe", &doc_id, &page.to_string()],
        );
        assert_eq!(
            described["ok"], true,
            "page describe failed for page {page}: {described}"
        );
        let cli_regions = described["data"]["regions"]
            .as_array()
            .unwrap_or_else(|| panic!("data.regions not array for page {page}: {described}"));
        let lib_page: Vec<&inkapp_core::manifest::Region> = published
            .manifest
            .regions
            .iter()
            .filter(|r| r.page == *page)
            .collect();

        assert_eq!(
            cli_regions.len(),
            lib_page.len(),
            "page {page} region count mismatch — CLI={} lib={};\n  CLI names: {:?}\n  lib names: {:?}",
            cli_regions.len(),
            lib_page.len(),
            cli_regions
                .iter()
                .map(|r| r["name"].as_str())
                .collect::<Vec<_>>(),
            lib_page.iter().map(|r| &r.name).collect::<Vec<_>>(),
        );

        for c in cli_regions {
            let name = c["name"].as_str().unwrap();
            let l = lib_page
                .iter()
                .find(|r| r.name == name)
                .unwrap_or_else(|| {
                    panic!(
                        "page {page}: CLI region '{name}' has no library counterpart;\n  lib names: {:?}",
                        lib_page.iter().map(|r| &r.name).collect::<Vec<_>>()
                    )
                });

            let arr = c["rect"].as_array().expect("rect must be 4-elem array");
            let cx0 = arr[0].as_f64().unwrap();
            let cy0 = arr[1].as_f64().unwrap();
            let cx1 = arr[2].as_f64().unwrap();
            let cy1 = arr[3].as_f64().unwrap();
            let tol = 0.01;
            assert!(
                (cx0 - l.rect.x0).abs() < tol,
                "{name} x0: CLI={cx0:.4} lib={:.4}",
                l.rect.x0
            );
            assert!(
                (cy0 - l.rect.y0).abs() < tol,
                "{name} y0: CLI={cy0:.4} lib={:.4}",
                l.rect.y0
            );
            assert!(
                (cx1 - l.rect.x1).abs() < tol,
                "{name} x1: CLI={cx1:.4} lib={:.4}",
                l.rect.x1
            );
            assert!(
                (cy1 - l.rect.y1).abs() < tol,
                "{name} y1: CLI={cy1:.4} lib={:.4}",
                l.rect.y1
            );
        }

        for l in &lib_page {
            let name = &l.name;
            assert!(
                cli_regions.iter().any(|c| c["name"].as_str() == Some(name)),
                "page {page}: lib region '{name}' has no CLI counterpart;\n  CLI names: {:?}",
                cli_regions
                    .iter()
                    .map(|c| c["name"].as_str())
                    .collect::<Vec<_>>(),
            );
        }
    }
}
