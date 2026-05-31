use inkapp_config::SecretRef;
use inkapp_core::secrets::{Scope, SecretStore};
use inkapp_readwise_reader::{ReaderConfig, Readwise};

#[tokio::test]
async fn from_config_builds_with_token() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = SecretStore::open(dir.path().join("secrets.json")).unwrap();
    store
        .set(Scope::ConnectorCred, "readwise-reader", b"tok")
        .unwrap();
    let cfg = ReaderConfig {
        token: SecretRef("readwise-reader".into()),
        ..Default::default()
    };
    let rw = Readwise::from_config(cfg, &store, dir.path().join("cache"))
        .await
        .unwrap();
    // Default config: library locations (new/later/shortlist) + feed (enabled).
    assert_eq!(
        rw.locations_for_test(),
        vec!["new", "later", "shortlist", "feed"]
    );
}

#[tokio::test]
async fn from_config_omits_feed_when_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = SecretStore::open(dir.path().join("secrets.json")).unwrap();
    store
        .set(Scope::ConnectorCred, "readwise-reader", b"tok")
        .unwrap();
    let cfg = ReaderConfig {
        token: SecretRef("readwise-reader".into()),
        feed_enabled: false,
        ..Default::default()
    };
    let rw = Readwise::from_config(cfg, &store, dir.path().join("cache"))
        .await
        .unwrap();
    assert!(
        !rw.locations_for_test().iter().any(|l| l == "feed"),
        "feed omitted when disabled"
    );
}
