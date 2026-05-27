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

fn new_session(home: &std::path::Path) -> String {
    let v = run(home, None, &["session", "new"]);
    v["data"]["session_id"].as_str().unwrap().to_string()
}

#[test]
fn device_new_then_list() {
    let tmp = tempfile::tempdir().unwrap();
    let id = new_session(tmp.path());
    let new = run(tmp.path(), Some(&id), &["device", "new", "--name", "rmpp"]);
    assert_eq!(new["ok"], true, "{new}");
    let dev_id = new["data"]["device_id"].as_str().unwrap().to_string();

    let list = run(tmp.path(), Some(&id), &["device", "list"]);
    assert_eq!(list["ok"], true);
    let devices = list["data"]["devices"].as_array().unwrap();
    assert!(devices.iter().any(|d| d["id"] == dev_id));
}

#[test]
fn device_sync_returns_envelope() {
    let tmp = tempfile::tempdir().unwrap();
    let id = new_session(tmp.path());
    let new = run(tmp.path(), Some(&id), &["device", "new"]);
    let dev_id = new["data"]["device_id"].as_str().unwrap().to_string();
    let v = run(tmp.path(), Some(&id), &["device", "sync", &dev_id]);
    assert_eq!(v["ok"], true, "{v}");
    assert!(v["data"]["pulled"].is_array());
}
