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

fn setup_with_doc(home: &std::path::Path) -> (String, String, String) {
    let v = run(home, None, &["session", "new"]);
    let sid = v["data"]["session_id"].as_str().unwrap().to_string();
    let d = run(home, Some(&sid), &["device", "new"]);
    let did = d["data"]["device_id"].as_str().unwrap().to_string();
    let pubv = run(home, Some(&sid), &["document", "publish", &did, "smoke"]);
    let doc_id = pubv["data"]["doc_id"].as_str().unwrap().to_string();
    (sid, did, doc_id)
}

#[test]
fn ink_tap_then_list() {
    let tmp = tempfile::tempdir().unwrap();
    let (sid, did, doc_id) = setup_with_doc(tmp.path());
    let v = run(
        tmp.path(),
        Some(&sid),
        &["ink", "--device", &did, "tap", &doc_id, "0", "r1"],
    );
    assert_eq!(v["ok"], true, "{v}");
    let list = run(tmp.path(), Some(&sid), &["ink", "list", &doc_id, "0"]);
    assert_eq!(list["ok"], true);
    assert!(!list["data"]["strokes"].as_array().unwrap().is_empty());
}

#[test]
fn ink_draw_parses_path() {
    let tmp = tempfile::tempdir().unwrap();
    let (sid, did, doc_id) = setup_with_doc(tmp.path());
    let v = run(
        tmp.path(),
        Some(&sid),
        &[
            "ink",
            "--device",
            &did,
            "draw",
            &doc_id,
            "0",
            "--path",
            "10,20 30,40 50,60",
        ],
    );
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["data"]["points"], 3);
}
