use rm_cloud::fake::FakeCloud;
use rm_cloud::{Client, Config, DocFiles, Metadata};

fn doc_with_ink(id: &str) -> DocFiles {
    let meta = Metadata {
        visible_name: "Doc".into(),
        doc_type: "DocumentType".into(),
        parent: "".into(),
        last_modified: "0".into(),
        deleted: false,
        extra: Default::default(),
    };
    DocFiles {
        id: id.into(),
        files: vec![
            (format!("{id}.metadata"), serde_json::to_vec(&meta).unwrap()),
            (format!("{id}.content"), br#"{"pages":["p1"]}"#.to_vec()),
            (format!("{id}.pdf"), b"%PDF-original".to_vec()),
            (format!("{id}/p1.rm"), b"INK-BYTES-DO-NOT-TOUCH".to_vec()),
        ],
    }
}

#[tokio::test]
async fn content_only_preserves_ink_and_content() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");
    let id = "doc-co";
    client.put(doc_with_ink(id)).await.unwrap();

    // Hashes the cloud should keep across a content-only swap.
    let ink_hash = rm_cloud::sha256_hex(b"INK-BYTES-DO-NOT-TOUCH");
    let content_hash = rm_cloud::sha256_hex(br#"{"pages":["p1"]}"#);
    assert!(
        cloud.blob(&ink_hash).is_some(),
        "ink blob present after put"
    );
    assert!(
        cloud.blob(&content_hash).is_some(),
        "content blob present after put"
    );

    client
        .put_content_only(id, b"%PDF-UPDATED".to_vec())
        .await
        .unwrap();

    // Ink + content blobs still present, unchanged; new pdf blob present.
    assert_eq!(
        cloud.blob(&ink_hash).as_deref(),
        Some(b"INK-BYTES-DO-NOT-TOUCH".as_slice())
    );
    assert_eq!(
        cloud.blob(&content_hash).as_deref(),
        Some(br#"{"pages":["p1"]}"#.as_slice())
    );
    let new_pdf_hash = rm_cloud::sha256_hex(b"%PDF-UPDATED");
    assert_eq!(
        cloud.blob(&new_pdf_hash).as_deref(),
        Some(b"%PDF-UPDATED".as_slice())
    );

    // Downloaded doc reflects new pdf + original ink.
    let got = client.get(id).await.unwrap();
    assert_eq!(got.get(&format!("{id}.pdf")).unwrap(), b"%PDF-UPDATED");
    assert_eq!(
        got.get(&format!("{id}/p1.rm")).unwrap(),
        b"INK-BYTES-DO-NOT-TOUCH"
    );
}
