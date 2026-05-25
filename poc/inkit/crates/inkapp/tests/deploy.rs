use inkapp::{resolve_transport, DeviceConfig};

#[test]
fn device_config_backend_defaults_to_remarkable() {
    assert_eq!(DeviceConfig::default().backend, "remarkable");
}

#[test]
fn resolve_transport_builds_known_backend() {
    assert!(resolve_transport("remarkable", "/ReadingQueue".into()).is_ok());
}

#[test]
fn resolve_transport_rejects_unknown_backend() {
    assert!(resolve_transport("supernote", "/Agenda".into()).is_err());
}
