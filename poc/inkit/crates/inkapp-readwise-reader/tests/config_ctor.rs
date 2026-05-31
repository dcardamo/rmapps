use inkapp_config::SecretRef;
use inkapp_core::connector::ConnectorError;
use inkapp_core::secrets::{Scope, SecretStore};
use inkapp_readwise_reader::{ReaderConfig, Readwise};

#[tokio::test]
async fn from_config_errors_when_token_secret_absent() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = SecretStore::open(dir.path().join("secrets.json")).unwrap();
    let cfg = ReaderConfig {
        token: SecretRef("readwise-reader".into()),
        ..Default::default()
    };
    let err = Readwise::from_config(cfg, &secrets, dir.path().join("cache")).await;
    assert!(
        matches!(err, Err(ConnectorError::Auth(_))),
        "absent token must produce an Auth error, got {:?}",
        err.err()
    );
}

#[tokio::test]
async fn from_config_resolves_named_token() {
    let dir = tempfile::tempdir().unwrap();
    let mut secrets = SecretStore::open(dir.path().join("secrets.json")).unwrap();
    secrets
        .set(Scope::ConnectorCred, "my-token", b"abc123")
        .unwrap();
    let cfg = ReaderConfig {
        token: SecretRef("my-token".into()),
        ..Default::default()
    };
    let conn = Readwise::from_config(cfg, &secrets, dir.path().join("cache")).await;
    assert!(conn.is_ok());
    conn.unwrap().close().await.unwrap();
}

#[tokio::test]
async fn from_config_errors_on_empty_token() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = SecretStore::open(dir.path().join("secrets.json")).unwrap();
    // Default config has an empty SecretRef → the is_empty() guard must fire.
    let err =
        Readwise::from_config(ReaderConfig::default(), &secrets, dir.path().join("cache")).await;
    assert!(
        matches!(err, Err(ConnectorError::Auth(_))),
        "empty token must produce an Auth error, got {:?}",
        err.err()
    );
}
