//! Dogfood: drive the real inkctl CLI through record -> emit and verify
//! the emitted source's shape. This is the end-to-end proof that all the
//! prior wiring composes.
//!
//! We don't try to compile the emitted source — `document publish` is
//! emitted as a `// TODO: re-publish app` placeholder (the CLI can't
//! reconstruct a `PublishedApp` from a trace alone), so a strict
//! round-trip is out of scope for v1. We assert the shape instead.

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

#[tokio::test]
async fn dogfood_record_to_emit_pipeline() {
    let home = tempfile::tempdir().unwrap();
    let home_path = home.path();

    // 1. New session
    let session = run(home_path, None, &["session", "new"]);
    assert_eq!(session["ok"], true, "{session}");
    let sid = session["data"]["session_id"].as_str().unwrap().to_string();

    // 2. Start recording first, so all subsequent calls land in the trace.
    let start = run(home_path, Some(&sid), &["record", "start"]);
    assert_eq!(start["ok"], true, "{start}");

    // 3. New device (now traced)
    let dev = run(home_path, Some(&sid), &["device", "new"]);
    assert_eq!(dev["ok"], true, "{dev}");
    let did = dev["data"]["device_id"].as_str().unwrap().to_string();

    // 4. Publish a document
    let publish = run(
        home_path,
        Some(&sid),
        &["document", "publish", &did, "smoke"],
    );
    assert_eq!(publish["ok"], true, "{publish}");
    let doc_id = publish["data"]["doc_id"].as_str().unwrap().to_string();

    // 5. Tap ink on a region
    let tap = run(
        home_path,
        Some(&sid),
        &["ink", "--device", &did, "tap", &doc_id, "0", "r1"],
    );
    assert_eq!(tap["ok"], true, "{tap}");

    // 6. Stop recording
    let stop = run(home_path, Some(&sid), &["record", "stop"]);
    assert_eq!(stop["ok"], true, "{stop}");

    // 7. Verify trace.jsonl
    let trace_path = home_path.join(&sid).join("trace.jsonl");
    assert!(
        trace_path.exists(),
        "trace.jsonl missing at {}",
        trace_path.display()
    );
    let trace_content = std::fs::read_to_string(&trace_path).unwrap();
    let lines: Vec<&str> = trace_content.lines().filter(|l| !l.is_empty()).collect();
    let call_lines: Vec<&&str> = lines
        .iter()
        .filter(|l| l.contains("\"kind\":\"call\"") || l.contains("\"call\""))
        .collect();
    assert!(
        call_lines.len() >= 3,
        "expected >= 3 call entries in trace, got {} (total lines {}):\n{}",
        call_lines.len(),
        lines.len(),
        trace_content
    );

    // 8. Emit a test from the trace
    let emit_out = home_path.join("emitted.rs");
    let emit = run(
        home_path,
        None,
        &[
            "record",
            "emit-test",
            "--from",
            trace_path.to_str().unwrap(),
            "--name",
            "dogfood_smoke",
            "--out",
            emit_out.to_str().unwrap(),
        ],
    );
    assert_eq!(emit["ok"], true, "{emit}");
    assert!(
        emit_out.exists(),
        "emitted file missing at {}",
        emit_out.display()
    );

    // 9. Verify the emitted source's shape.
    let src = std::fs::read_to_string(&emit_out).unwrap();
    for needle in [
        "#[tokio::test]",
        "async fn dogfood_smoke()",
        "Session::new_fake",
        "s.device_new(",
        "s.ink_tap(",
    ] {
        assert!(
            src.contains(needle),
            "emitted source missing {needle:?}; full src:\n{src}"
        );
    }
    assert!(
        src.len() >= 200,
        "emitted source suspiciously short: {} bytes:\n{src}",
        src.len()
    );
}
