use rm_cloud::fake::FakeCloud;
use rm_cloud::plumbing::index::{serialize_root_index, DocEntry};
use rm_cloud::{Client, Config};

#[tokio::test]
async fn empty_account_snapshot() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");
    let snap = client.snapshot().await.unwrap();
    assert_eq!(snap.generation, 0);
    assert_eq!(snap.docs().count(), 0);
}

#[tokio::test]
async fn snapshot_reflects_uploaded_root() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");

    // Hand-place a root index blob + root ref via the fake's own HTTP.
    let docs = vec![DocEntry {
        id: "doc-a".into(),
        hash: "ab".repeat(32),
        num_files: 2,
        size: 12,
    }];
    let bytes = serialize_root_index(&docs);
    let root_hash = rm_cloud::sha256_hex(&bytes);
    let http = reqwest::Client::new();
    http.put(format!("{}/sync/v3/files/{root_hash}", cloud.base))
        .header("rm-filename", "root.docSchema")
        .body(bytes)
        .send()
        .await
        .unwrap();
    let body = serde_json::json!({"broadcast": false, "hash": root_hash, "generation": 0});
    http.put(format!("{}/sync/v3/root", cloud.base))
        .json(&body)
        .send()
        .await
        .unwrap();

    let snap = client.snapshot().await.unwrap();
    assert_eq!(snap.generation, 1);
    assert_eq!(snap.doc("doc-a").unwrap().num_files, 2);
}

#[tokio::test]
async fn snapshot_retries_after_one_unauthorized() {
    let cloud = FakeCloud::spawn().await;
    // device-token client so force_refresh() can mint a user token from the fake.
    let client = Client::from_device_token(Config::single_host(&cloud.base), "dev-token");

    // Seed a root so a successful GET returns a real snapshot.
    let docs = vec![DocEntry {
        id: "doc-z".into(),
        hash: "cd".repeat(32),
        num_files: 1,
        size: 3,
    }];
    let bytes = serialize_root_index(&docs);
    let root_hash = rm_cloud::sha256_hex(&bytes);
    let http = reqwest::Client::new();
    http.put(format!("{}/sync/v3/files/{root_hash}", cloud.base))
        .header("rm-filename", "root.docSchema")
        .body(bytes)
        .send()
        .await
        .unwrap();
    let body = serde_json::json!({"broadcast": false, "hash": root_hash, "generation": 0});
    http.put(format!("{}/sync/v3/root", cloud.base))
        .json(&body)
        .send()
        .await
        .unwrap();

    // Force the first root GET to 401; snapshot() must refresh + retry and still succeed.
    cloud.inject_unauthorized_once();
    let snap = client.snapshot().await.unwrap();
    assert!(snap.doc("doc-z").is_some());
}
