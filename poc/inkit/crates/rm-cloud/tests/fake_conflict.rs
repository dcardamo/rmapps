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
async fn commit_succeeds_after_injected_conflict() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");

    cloud.inject_conflict_once(); // first root PUT will 412
    let snap = client
        .commit(Mutation {
            upserts: vec![one_doc("doc-1")],
            removals: vec![],
        })
        .await
        .unwrap();
    assert!(snap.doc("doc-1").is_some());
    assert!(snap.generation >= 1);
}

#[tokio::test]
async fn commit_exhausts_after_persistent_conflicts() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");

    cloud.inject_conflicts(10); // every one of the 10 attempts will 412
    let err = client
        .commit(Mutation {
            upserts: vec![one_doc("doc-x")],
            removals: vec![],
        })
        .await
        .unwrap_err();
    assert!(
        matches!(err, rm_cloud::Error::CommitExhausted(10)),
        "got {err:?}"
    );
}

#[tokio::test]
async fn commit_round_trips_into_snapshot() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");
    client
        .commit(Mutation {
            upserts: vec![one_doc("a"), one_doc("b")],
            removals: vec![],
        })
        .await
        .unwrap();
    let snap = client.snapshot().await.unwrap();
    assert_eq!(snap.docs().count(), 2);

    client
        .commit(Mutation {
            upserts: vec![],
            removals: vec!["a".into()],
        })
        .await
        .unwrap();
    let snap = client.snapshot().await.unwrap();
    assert_eq!(snap.docs().count(), 1);
    assert!(snap.doc("b").is_some());
}
