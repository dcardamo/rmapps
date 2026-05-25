use inkapp_core::connector::Connector;
use inkapp_core::secrets::SecretStore;
use inkapp_readwise_reader::{ReaderConfig, Readwise};

#[tokio::test]
#[ignore = "hits the real Readwise API; requires a readwise-reader token in the secret store"]
async fn live_readwise_reader() {
    let store = SecretStore::open_default().expect("secret store");
    let cache_dir = std::env::temp_dir().join("inkapp-readwise-reader-livetest");
    let rw = Readwise::live(&store, &cache_dir, ReaderConfig::default())
        .await
        .expect("live ctor");
    rw.refresh().await.expect("refresh");
    assert!(
        rw.feed().len() + rw.library().len() > 0,
        "expected some articles"
    );
    // Read-only: no move/delete/highlight here.
}
