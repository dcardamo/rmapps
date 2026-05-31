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
            "not JSON: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

fn setup_with_doc(home: &std::path::Path) -> (String, String) {
    let v = run(home, None, &["session", "new"]);
    let sid = v["data"]["session_id"].as_str().unwrap().to_string();
    let d = run(home, Some(&sid), &["device", "new"]);
    let did = d["data"]["device_id"].as_str().unwrap().to_string();
    let pubv = run(home, Some(&sid), &["document", "publish", &did, "uri-link"]);
    let doc_id = pubv["data"]["doc_id"].as_str().unwrap().to_string();
    (sid, doc_id)
}

#[test]
fn page_describe_and_links() {
    let tmp = tempfile::tempdir().unwrap();
    let (sid, doc_id) = setup_with_doc(tmp.path());
    let desc = run(tmp.path(), Some(&sid), &["page", "describe", &doc_id, "0"]);
    assert_eq!(desc["ok"], true, "{desc}");
    assert_eq!(desc["data"]["page"], 0);

    let links = run(tmp.path(), Some(&sid), &["page", "links", &doc_id, "0"]);
    assert_eq!(links["ok"], true);
    assert!(links["data"]["links"].is_array());
}

#[test]
fn page_snapshot_writes_png() {
    let tmp = tempfile::tempdir().unwrap();
    let (sid, doc_id) = setup_with_doc(tmp.path());
    let out = tmp.path().join("page.png");
    let v = run(
        tmp.path(),
        Some(&sid),
        &[
            "page",
            "snapshot",
            &doc_id,
            "0",
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(v["ok"], true, "{v}");
    assert!(v["data"]["bytes"].as_u64().unwrap() > 0);
    assert!(out.exists());
}
