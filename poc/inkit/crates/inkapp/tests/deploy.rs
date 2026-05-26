use inkapp::{resolve_transport, DeviceConfig, SecretStore};

fn empty_store() -> (tempfile::TempDir, SecretStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = SecretStore::open(dir.path().join("secrets.json")).unwrap();
    (dir, store)
}

#[test]
fn device_config_backend_defaults_to_remarkable() {
    assert_eq!(DeviceConfig::default().backend, "remarkable");
}

#[test]
fn resolve_transport_routes_known_backend() {
    // "remarkable" is a recognized backend. Building its cloud transport may still
    // fail without cloud credentials in the environment, so we assert routing — not
    // a live connection — by checking it does NOT produce the unknown-backend error.
    let (_d, secrets) = empty_store();
    std::env::remove_var("RM_CLOUD_DEVICE_TOKEN");
    std::env::remove_var("RM_CLOUD_USER_TOKEN");
    if let Err(e) = resolve_transport("remarkable", "/ReadingQueue".into(), &secrets) {
        assert!(
            !e.to_string().contains("unknown deploy backend"),
            "remarkable should be a known backend, got: {e}"
        );
    }
}

#[test]
fn resolve_transport_rejects_unknown_backend() {
    let (_d, secrets) = empty_store();
    let err = resolve_transport("supernote", "/Agenda".into(), &secrets);
    assert!(err.is_err());
    assert!(err
        .err()
        .unwrap()
        .to_string()
        .contains("unknown deploy backend"));
}
