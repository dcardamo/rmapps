use inkapp::{resolve_transport, DeviceConfig};

#[test]
fn device_config_backend_defaults_to_remarkable() {
    assert_eq!(DeviceConfig::default().backend, "remarkable");
}

#[test]
fn resolve_transport_routes_known_backend() {
    // "remarkable" is a recognized backend. Building its cloud transport may still
    // fail without cloud credentials in the environment, so we assert routing — not
    // a live connection — by checking it does NOT produce the unknown-backend error.
    if let Err(e) = resolve_transport("remarkable", "/ReadingQueue".into()) {
        assert!(
            !e.to_string().contains("unknown deploy backend"),
            "remarkable should be a known backend, got: {e}"
        );
    }
}

#[test]
fn resolve_transport_rejects_unknown_backend() {
    let err = resolve_transport("supernote", "/Agenda".into());
    assert!(err.is_err());
    assert!(err
        .err()
        .unwrap()
        .to_string()
        .contains("unknown deploy backend"));
}
