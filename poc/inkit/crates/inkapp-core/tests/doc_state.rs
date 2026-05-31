use inkapp_core::manifest::{DocState, Manifest};
use serde_json::json;

#[test]
fn docstate_round_trips() {
    let mut s = DocState {
        doc: Some(json!({"cursor": 3})),
        ..Default::default()
    };
    s.components.insert("stepper:c".into(), json!(5u64));
    let bytes = serde_json::to_vec(&s).unwrap();
    let back: DocState = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(s, back);
}

#[test]
fn manifest_without_state_key_deserializes() {
    // Older sealed blobs carry no `state`; serde(default) must fill it.
    let json = r#"{"version":2,"regions":[]}"#;
    let m: Manifest = serde_json::from_str(json).unwrap();
    assert_eq!(m.version, 2);
    assert_eq!(m.state, DocState::default());
}
