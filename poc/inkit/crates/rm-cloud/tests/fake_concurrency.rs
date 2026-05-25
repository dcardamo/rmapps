use rm_cloud::fake::FakeCloud;
use rm_cloud::{Client, Config, DocUpsert, Mutation};

#[tokio::test]
async fn parallel_commits_all_land() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");

    let mut handles = Vec::new();
    for i in 0..5 {
        let c = client.clone();
        handles.push(tokio::spawn(async move {
            let id = format!("doc-{i}");
            let up = DocUpsert {
                id: id.clone(),
                files: vec![(format!("{id}.metadata"), b"{}".to_vec())],
            };
            c.commit(Mutation {
                upserts: vec![up],
                removals: vec![],
            })
            .await
            .unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    let snap = client.snapshot().await.unwrap();
    assert_eq!(
        snap.docs().count(),
        5,
        "all 5 concurrent commits must land via rebase"
    );
    assert_eq!(snap.generation, 5);
}
