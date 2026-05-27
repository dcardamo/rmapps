use assert_cmd::Command;
use serde_json::Value;

fn run(home: &std::path::Path, args: &[&str], extra_env: &[(&str, &str)]) -> Value {
    let mut cmd = Command::cargo_bin("inkctl").unwrap();
    cmd.env("INKCTL_HOME", home);
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let out = cmd.args(args).output().unwrap();
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout was not JSON: {}\nstderr: {}\nerr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
            e
        )
    })
}

#[test]
fn session_new_then_list_then_destroy() {
    let tmp = tempfile::tempdir().unwrap();
    let v = run(tmp.path(), &["session", "new"], &[]);
    assert_eq!(v["ok"], true, "{v}");
    let id = v["data"]["session_id"].as_str().unwrap().to_string();
    assert_eq!(v["data"]["backend"], "fake");

    let list = run(tmp.path(), &["session", "list"], &[]);
    assert_eq!(list["ok"], true);
    let sessions = list["data"]["sessions"].as_array().unwrap();
    assert!(sessions.iter().any(|s| s["id"] == id), "{list}");

    let destroyed = run(tmp.path(), &["session", "destroy", &id], &[]);
    assert_eq!(destroyed["ok"], true);
}

#[test]
fn session_env_emits_export_line() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("inkctl").unwrap();
    cmd.env("INKCTL_HOME", tmp.path());
    let out = cmd.args(["session", "env", "abc-123"]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout.trim(), "INKCTL_SESSION=abc-123");
}

#[test]
fn session_step_is_not_implemented() {
    let tmp = tempfile::tempdir().unwrap();
    let v = run(tmp.path(), &["session", "step", "--device", "dev-1"], &[]);
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"]["kind"], "not_implemented");
}
