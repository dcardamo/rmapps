use inkapp::ConfigStore;
use inkapp_core::secrets::{Scope, SecretStore};
use reading_queue::{AppConfig, Connectors};

#[tokio::test]
async fn wires_readwise_from_config() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("config.toml");
    std::fs::write(
        &cfg_path,
        "[app.reading-queue.default]\ndevice_folder = \"/RQ\"\nreadwise = \"readwise.main\"\n\
         [connector.readwise.main]\ntoken = \"rw\"\n",
    )
    .unwrap();
    let mut secrets = SecretStore::open(dir.path().join("secrets.json")).unwrap();
    secrets.set(Scope::ConnectorCred, "rw", b"tok").unwrap();

    let store = ConfigStore::open(&cfg_path).unwrap();
    let app_cfg: AppConfig = store.resolve("default").unwrap();
    assert_eq!(app_cfg.device_folder, "/RQ");
    // Verify from_config succeeds and the connector is live.
    let conn = Connectors::from_config(&store, &app_cfg, &secrets, dir.path().join("cache"))
        .await
        .expect("wire");
    // Close the durable cache so its entries flush to disk (matches the
    // readwise connector's shutdown contract).
    conn.readwise.close().await.unwrap();
}
