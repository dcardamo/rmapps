//! 429 backoff/retry: the client must transparently retry rate-limited sync requests
//! (honoring `Retry-After`) and only surface [`Error::RateLimited`] once the retry
//! budget is exhausted. The fake injects 429 + `Retry-After: 0`, so these run instantly.

use rm_cloud::fake::FakeCloud;
use rm_cloud::{Client, Config, DocUpsert, Mutation};

fn one_doc(id: &str) -> DocUpsert {
    DocUpsert {
        id: id.to_string(),
        files: vec![
            (format!("{id}.metadata"), br#"{"visibleName":"t"}"#.to_vec()),
            (format!("{id}.content"), b"{}".to_vec()),
        ],
    }
}

#[tokio::test]
async fn root_get_retries_through_rate_limit() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");

    // The first 3 sync requests get 429'd; the client should retry past them. With an
    // empty account the eventual root GET is a 404 → None.
    cloud.inject_rate_limited(3);
    let gen = client.current_generation().await.unwrap();
    assert_eq!(gen, None);
    // All injected 429s were consumed by retries.
    assert_eq!(cloud.state.lock().unwrap().rate_limited_remaining, 0);
}

#[tokio::test]
async fn commit_retries_through_rate_limit() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");

    // Rate-limit the first couple of blob/root requests in the commit path; the commit
    // must still land the document.
    cloud.inject_rate_limited(2);
    let snap = client
        .commit(Mutation {
            upserts: vec![one_doc("doc-1")],
            removals: vec![],
        })
        .await
        .unwrap();
    assert!(snap.doc("doc-1").is_some());
    assert_eq!(cloud.state.lock().unwrap().rate_limited_remaining, 0);
}

#[tokio::test]
async fn rate_limit_exhausts_after_persistent_429() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");

    // Far more 429s than the retry budget → the client gives up with RateLimited rather
    // than retrying forever.
    cloud.inject_rate_limited(100);
    let err = client.current_generation().await.unwrap_err();
    assert!(matches!(err, rm_cloud::Error::RateLimited), "got {err:?}");
}
