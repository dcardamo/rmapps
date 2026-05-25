//! Path-op porcelain against the fake cloud: `mkdir_p` (path→id, idempotent) and
//! `DocFiles::new_pdf` (a fresh PDF doc that round-trips through put/get).

use rm_cloud::fake::FakeCloud;
use rm_cloud::{Client, Config, DocFiles};

#[tokio::test]
async fn mkdir_p_creates_nested_and_is_idempotent() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");

    // Create a two-level path; a leading slash is optional.
    let id = client.mkdir_p("/InkAppDev/acceptance").await.unwrap();
    assert!(!id.is_empty(), "leaf folder has an id");

    // The intermediate folder exists under root and the leaf under it.
    let root = client.ls("").await.unwrap();
    let dev = root.iter().find(|e| e.name == "InkAppDev").unwrap();
    assert!(dev.is_folder);
    let children = client.ls(&dev.id).await.unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].name, "acceptance");
    assert_eq!(children[0].id, id);

    // Re-resolving the same path is a no-op: same id, no duplicate folders.
    let again = client.mkdir_p("InkAppDev/acceptance").await.unwrap();
    assert_eq!(again, id, "idempotent: resolves to the existing leaf");
    assert_eq!(
        client.ls("").await.unwrap().len(),
        1,
        "no duplicate InkAppDev"
    );
    assert_eq!(
        client.ls(&dev.id).await.unwrap().len(),
        1,
        "no duplicate leaf"
    );
}

#[tokio::test]
async fn empty_path_resolves_to_root() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");
    assert_eq!(client.mkdir_p("").await.unwrap(), "");
    assert_eq!(client.mkdir_p("/").await.unwrap(), "");
}

#[tokio::test]
async fn new_pdf_round_trips_through_put_get() {
    let cloud = FakeCloud::spawn().await;
    let client = Client::from_user_token(Config::single_host(&cloud.base), "user-token");

    let folder = client.mkdir_p("/ReadingQueue").await.unwrap();
    let df = DocFiles::new_pdf("article-7", &folder, b"%PDF-1.4 body".to_vec());
    let id = df.id.clone();
    client.put(df).await.unwrap();

    // It lists under the folder with the visible name we set, as a document.
    let listing = client.ls(&folder).await.unwrap();
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].name, "article-7");
    assert!(!listing[0].is_folder);

    // Its blobs round-trip, and the metadata is a DocumentType under the folder.
    let got = client.get(&id).await.unwrap();
    assert_eq!(got.get(&format!("{id}.pdf")).unwrap(), b"%PDF-1.4 body");
    let meta = got.metadata().unwrap();
    assert_eq!(meta.doc_type, "DocumentType");
    assert_eq!(meta.parent, folder);
}
