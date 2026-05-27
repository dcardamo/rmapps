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
fn record_start_stop() {
    let tmp = tempfile::tempdir().unwrap();
    let sid = new_session(tmp.path());
    let start = run(tmp.path(), Some(&sid), &["record", "start"]);
    assert_eq!(start["ok"], true);
    assert_eq!(start["data"]["recording"], true);
    let stop = run(tmp.path(), Some(&sid), &["record", "stop"]);
    assert_eq!(stop["ok"], true);
    assert_eq!(stop["data"]["recording"], false);
}

#[test]
fn record_assert_appends() {
    let tmp = tempfile::tempdir().unwrap();
    let sid = new_session(tmp.path());
    let v = run(
        tmp.path(),
        Some(&sid),
        &["record", "assert", "page.0.regions", "[]"],
    );
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["data"]["asserted"], true);
}

#[test]
fn record_replay_is_not_implemented() {
    let tmp = tempfile::tempdir().unwrap();
    let v = run(tmp.path(), None, &["record", "replay", "/tmp/no.jsonl"]);
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"]["kind"], "not_implemented");
}

#[test]
fn record_emit_test_writes_source() {
    let tmp = tempfile::tempdir().unwrap();
    let sid = new_session(tmp.path());
    // produce a trace by start + assert + stop
    let _ = run(tmp.path(), Some(&sid), &["record", "start"]);
    let _ = run(tmp.path(), Some(&sid), &["record", "assert", "x", "true"]);
    let _ = run(tmp.path(), Some(&sid), &["record", "stop"]);
    let trace = tmp.path().join(&sid).join("trace.jsonl");
    assert!(
        trace.exists(),
        "trace file should exist: {}",
        trace.display()
    );
    let out = tmp.path().join("emitted.rs");
    let v = run(
        tmp.path(),
        None,
        &[
            "record",
            "emit-test",
            "--from",
            trace.to_str().unwrap(),
            "--name",
            "smoke",
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(v["ok"], true, "{v}");
    assert!(out.exists());
}
