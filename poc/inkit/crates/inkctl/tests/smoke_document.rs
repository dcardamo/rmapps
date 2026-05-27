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

fn setup(home: &std::path::Path) -> (String, String) {
    let v = run(home, None, &["session", "new"]);
    let sid = v["data"]["session_id"].as_str().unwrap().to_string();
    let d = run(home, Some(&sid), &["device", "new"]);
    let did = d["data"]["device_id"].as_str().unwrap().to_string();
    (sid, did)
}

#[test]
fn document_publish_then_describe() {
    let tmp = tempfile::tempdir().unwrap();
    let (sid, did) = setup(tmp.path());
    let pub_v = run(
        tmp.path(),
        Some(&sid),
        &["document", "publish", &did, "smoke"],
    );
    assert_eq!(pub_v["ok"], true, "{pub_v}");
    let doc_id = pub_v["data"]["doc_id"].as_str().unwrap().to_string();
    assert_eq!(pub_v["data"]["app_name"], "smoke");

    let desc = run(tmp.path(), Some(&sid), &["document", "describe", &doc_id]);
    assert_eq!(desc["ok"], true);
    assert_eq!(desc["data"]["app_name"], "smoke");
}

#[test]
fn document_open_records_current() {
    let tmp = tempfile::tempdir().unwrap();
    let (sid, _did) = setup(tmp.path());
    let v = run(tmp.path(), Some(&sid), &["document", "open", "doc-1"]);
    assert_eq!(v["ok"], true);
    assert_eq!(v["data"]["doc_id"], "doc-1");
}

#[test]
fn document_publish_unknown_app_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let (sid, did) = setup(tmp.path());
    let v = run(
        tmp.path(),
        Some(&sid),
        &["document", "publish", &did, "no-such-app"],
    );
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"]["kind"], "unknown_app");
}

#[test]
fn document_pdf_writes_file() {
    let tmp = tempfile::tempdir().unwrap();
    let (sid, did) = setup(tmp.path());
    let pub_v = run(
        tmp.path(),
        Some(&sid),
        &["document", "publish", &did, "smoke"],
    );
    let doc_id = pub_v["data"]["doc_id"].as_str().unwrap().to_string();
    let out = tmp.path().join("out.pdf");
    let v = run(
        tmp.path(),
        Some(&sid),
        &["document", "pdf", &doc_id, "--out", out.to_str().unwrap()],
    );
    assert_eq!(v["ok"], true, "{v}");
    assert!(v["data"]["bytes"].as_u64().unwrap() > 0);
    assert!(out.exists());
}
