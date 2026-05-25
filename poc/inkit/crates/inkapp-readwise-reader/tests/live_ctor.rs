use inkapp_core::connector::ConnectorError;
use inkapp_core::secrets::{Scope, SecretStore};
use inkapp_readwise_reader::{ReaderConfig, Readwise};

#[tokio::test]
async fn live_requires_token() {
    let dir = tempfile::tempdir().unwrap();
    let store = SecretStore::open(dir.path().join("secrets.json")).unwrap();
    match Readwise::live(&store, dir.path().join("cache"), ReaderConfig::default()).await {
        Err(ConnectorError::Auth(_)) => {}
        Err(e) => panic!("expected ConnectorError::Auth, got: {e:?}"),
        Ok(_) => panic!("expected Err, got Ok"),
    }
}

#[tokio::test]
async fn live_builds_with_token() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = SecretStore::open(dir.path().join("secrets.json")).unwrap();
    store
        .set(Scope::ConnectorCred, "readwise-reader", b"tok")
        .unwrap();
    let rw = Readwise::live(&store, dir.path().join("cache"), ReaderConfig::default())
        .await
        .unwrap();
    // Default config: library locations (new/later/shortlist) + feed (enabled).
    assert_eq!(
        rw.locations_for_test(),
        vec!["new", "later", "shortlist", "feed"]
    );
}

#[tokio::test]
async fn live_omits_feed_when_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = SecretStore::open(dir.path().join("secrets.json")).unwrap();
    store
        .set(Scope::ConnectorCred, "readwise-reader", b"tok")
        .unwrap();
    let config = ReaderConfig {
        feed_enabled: false,
        ..ReaderConfig::default()
    };
    let rw = Readwise::live(&store, dir.path().join("cache"), config)
        .await
        .unwrap();
    assert!(
        !rw.locations_for_test().iter().any(|l| l == "feed"),
        "feed omitted when disabled"
    );
}
