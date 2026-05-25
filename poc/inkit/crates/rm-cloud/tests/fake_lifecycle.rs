//! Smoke-tests the fake cloud directly with raw reqwest (no client layer yet).

use rm_cloud::fake::FakeCloud;

#[tokio::test]
async fn root_cas_and_blob_storage() {
    let cloud = FakeCloud::spawn().await;
    let http = reqwest::Client::new();

    // Root is 404 before any write.
    let r = http
        .get(format!("{}/sync/v4/root", cloud.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), reqwest::StatusCode::NOT_FOUND);

    // Store a blob, read it back verbatim.
    let put = http
        .put(format!("{}/sync/v3/files/deadbeef", cloud.base))
        .header("rm-filename", "x.metadata")
        .body("hello".as_bytes().to_vec())
        .send()
        .await
        .unwrap();
    assert!(put.status().is_success());
    let got = http
        .get(format!("{}/sync/v3/files/deadbeef", cloud.base))
        .send()
        .await
        .unwrap();
    assert_eq!(got.bytes().await.unwrap().as_ref(), b"hello");

    // First root PUT (gen 0) succeeds -> gen 1.
    let body = serde_json::json!({"broadcast": false, "hash": "roothash1", "generation": 0});
    let r = http
        .put(format!("{}/sync/v3/root", cloud.base))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    let j: serde_json::Value = r.json().await.unwrap();
    assert_eq!(j["generation"], 1);

    // Stale generation -> 412.
    let stale = serde_json::json!({"broadcast": false, "hash": "roothash2", "generation": 0});
    let r = http
        .put(format!("{}/sync/v3/root", cloud.base))
        .json(&stale)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), reqwest::StatusCode::PRECONDITION_FAILED);

    // Forced conflict toggle.
    cloud.inject_conflict_once();
    let good = serde_json::json!({"broadcast": false, "hash": "roothash3", "generation": 1});
    let r = http
        .put(format!("{}/sync/v3/root", cloud.base))
        .json(&good)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), reqwest::StatusCode::PRECONDITION_FAILED);
}
