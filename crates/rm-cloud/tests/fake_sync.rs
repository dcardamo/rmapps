use rm_cloud::fake::FakeCloud;
use rm_cloud::{Client, Config, DocFiles, Metadata, WorkingSet, APP_KEY_FIELD};
use std::collections::BTreeMap;
use uuid::Uuid;

fn keyed_doc(key: &str, pdf: &[u8]) -> DocFiles {
    let id = Uuid::new_v4().to_string();
    let mut extra = serde_json::Map::new();
    extra.insert(APP_KEY_FIELD.into(), serde_json::Value::String(key.into()));
    let meta = Metadata {
        visible_name: key.into(),
        doc_type: "DocumentType".into(),
        parent: "".into(),
        last_modified: "0".into(),
        deleted: false,
        extra,
    };
    DocFiles {
        id: id.clone(),
        files: vec![
            (format!("{id}.metadata"), serde_json::to_vec(&meta).unwrap()),
            (format!("{id}.content"), b"{}".to_vec()),
            (format!("{id}.pdf"), pdf.to_vec()),
        ],
    }
}

#[tokio::test]
async fn sync_creates_then_no_ops_then_removes() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");

    // Create two app docs.
    let mut docs = BTreeMap::new();
    docs.insert("a".to_string(), keyed_doc("a", b"%PDF-a"));
    docs.insert("b".to_string(), keyed_doc("b", b"%PDF-b"));
    let (rep, snap1) = client.sync(WorkingSet { docs }, None).await.unwrap();
    assert!(rep.committed);
    assert_eq!(snap1.docs().count(), 2);

    // Re-sync with the SAME generation -> no-op fast path.
    let (rep, _snap) = client
        .sync(WorkingSet::default(), Some(&snap1))
        .await
        .unwrap();
    assert!(!rep.committed);
    assert!(rep.changed_keys.is_empty());

    // Drop "b" from the target -> it gets removed in one commit.
    let mut docs = BTreeMap::new();
    docs.insert("a".to_string(), keyed_doc("a", b"%PDF-a2"));
    let (rep, snap2) = client.sync(WorkingSet { docs }, None).await.unwrap();
    assert!(rep.committed);
    assert_eq!(snap2.docs().count(), 1);
}
