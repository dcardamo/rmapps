use inkapp_harness::emit;
use inkapp_harness::trace::TraceEntry;
use serde_json::json;

#[test]
fn emit_basic_trace_compiles_to_recognisable_rust() {
    let trace = vec![
        TraceEntry::Call {
            ts: "2026-05-27T00:00:00Z".into(),
            cmd: vec!["device".into(), "new".into()],
            args: json!({ "name": "primary" }),
            result: json!({ "id": "dev-1" }),
        },
        TraceEntry::Call {
            ts: "2026-05-27T00:00:01Z".into(),
            cmd: vec!["document".into(), "publish".into()],
            args: json!({ "app_name": "smoke" }),
            result: json!({ "id": "dev-1-smoke", "version": 1, "pages": 1 }),
        },
        TraceEntry::Call {
            ts: "2026-05-27T00:00:02Z".into(),
            cmd: vec!["ink".into(), "tap".into()],
            args: json!({ "device": "dev-1", "doc_id": "dev-1-smoke", "page": 0, "region": "r1" }),
            result: json!({}),
        },
        TraceEntry::Assert {
            ts: "2026-05-27T00:00:03Z".into(),
            target: "step.cycle".into(),
            expected: json!(1),
        },
    ];

    let code = emit::to_rust(&trace, "smoke_test");

    assert!(code.contains("#[tokio::test]"));
    assert!(code.contains("async fn smoke_test()"));
    assert!(code.contains("s.device_new(Some(\"primary\"))"));
    assert!(code.contains("doc_id: &str = \"dev-1-smoke\""));
    assert!(code.contains("s.ink_tap(&dev_1, doc_id, 0, \"r1\")"));
    assert!(code.contains("assert step.cycle"));
}

#[test]
fn empty_trace_emits_minimal_test() {
    let code = emit::to_rust(&[], "empty");
    assert!(code.contains("async fn empty()"));
    assert!(code.contains("Session::new_fake"));
}
