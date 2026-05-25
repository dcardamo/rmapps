use rm_cloud::fake::FakeCloud;
use rm_cloud::{Client, Config, DocFiles, Metadata};
use uuid::Uuid;

fn doc_with_pdf(id: &str, name: &str, parent: &str, pdf: &[u8]) -> DocFiles {
    let meta = Metadata {
        visible_name: name.into(),
        doc_type: "DocumentType".into(),
        parent: parent.into(),
        last_modified: "0".into(),
        deleted: false,
        extra: Default::default(),
    };
    DocFiles {
        id: id.into(),
        files: vec![
            (format!("{id}.metadata"), serde_json::to_vec(&meta).unwrap()),
            (format!("{id}.content"), b"{}".to_vec()),
            (format!("{id}.pdf"), pdf.to_vec()),
        ],
    }
}

#[tokio::test]
async fn put_get_ls_mkdir_mv_rm() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");

    let folder = client.mkdir("Reading", "").await.unwrap();
    let id = Uuid::new_v4().to_string();
    client
        .put(doc_with_pdf(&id, "Article", &folder, b"%PDF-1"))
        .await
        .unwrap();

    let listing = client.ls(&folder).await.unwrap();
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].name, "Article");

    let got = client.get(&id).await.unwrap();
    assert_eq!(got.get(&format!("{id}.pdf")).unwrap(), b"%PDF-1");

    client.mv(&id, None, Some("Renamed")).await.unwrap();
    assert_eq!(client.stat(&id).await.unwrap().visible_name, "Renamed");

    client.rm(&id).await.unwrap();
    assert!(client.ls(&folder).await.unwrap().is_empty());
}

#[tokio::test]
async fn bundle_round_trip() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");
    let id = Uuid::new_v4().to_string();
    client
        .put(doc_with_pdf(&id, "Doc", "", b"%PDF-bundle"))
        .await
        .unwrap();
    // get_bundle writes a temp .rmdoc and opens it via rm-files.
    let bundle = client.get_bundle(&id).await.unwrap();
    assert_eq!(bundle.source_pdf(), Some(b"%PDF-bundle".as_slice()));
}
