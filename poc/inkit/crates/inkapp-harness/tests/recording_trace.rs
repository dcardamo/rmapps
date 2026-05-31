//! Trace recording: when `record_start` is set, mutating Session methods append
//! a JSON-lines entry to `state_dir/trace.jsonl`. Off by default.

use inkapp_harness::session::Session;
use inkapp_harness::tests_common::single_region_app;
use inkapp_harness::trace::{self, TraceEntry};
use tempfile::tempdir;

#[tokio::test]
async fn trace_records_only_when_recording_on() {
    let dir = tempdir().unwrap();
    let mut s = Session::new_fake(dir.path()).await.unwrap();

    // OFF by default: no trace file should be created by a device_new.
    let _dev1 = s.device_new(Some("offdev")).unwrap();
    let trace_path = dir.path().join("trace.jsonl");
    assert!(!trace_path.exists(), "no trace when recording is off");

    // Turn on, do work, verify entries.
    s.record_start().unwrap();
    let dev = s.device_new(Some("ondev")).unwrap();
    let doc = s
        .document_publish(&dev, single_region_app("traced"))
        .await
        .unwrap();
    s.ink_tap(&dev, &doc.id, 0, "r1").unwrap();

    let entries = trace::read_trace(&trace_path).unwrap();
    assert!(entries.len() >= 3, "got: {entries:?}");
    let cmds: Vec<Vec<String>> = entries
        .iter()
        .filter_map(|e| match e {
            TraceEntry::Call { cmd, .. } => Some(cmd.clone()),
            _ => None,
        })
        .collect();
    assert!(cmds.iter().any(|c| c == &["device", "new"]));
    assert!(cmds.iter().any(|c| c == &["document", "publish"]));
    assert!(cmds.iter().any(|c| c == &["ink", "tap"]));

    // record_assert appends even with recording on (and would also append when off).
    s.record_assert("page.describe.regions.len", serde_json::json!(1))
        .unwrap();
    let after_assert = trace::read_trace(&trace_path).unwrap();
    assert!(after_assert.iter().any(|e| matches!(
        e,
        TraceEntry::Assert { target, .. } if target == "page.describe.regions.len"
    )));

    s.record_stop().unwrap();
    let prev_count = trace::read_trace(&trace_path).unwrap().len();
    let _dev3 = s.device_new(Some("after-stop")).unwrap();
    let after_stop_count = trace::read_trace(&trace_path).unwrap().len();
    assert_eq!(
        prev_count, after_stop_count,
        "no new trace entries after record_stop"
    );
}
