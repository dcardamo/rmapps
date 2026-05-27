use inkapp_harness::session::Session;
use inkapp_harness::tests_common::single_region_app;
use tempfile::tempdir;

#[tokio::test]
async fn publish_writes_doc_and_increments_version() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("s");
    let mut s = Session::new_fake(&path).await.unwrap();
    let dev = s.device_new(Some("rm")).unwrap();

    let app = single_region_app("smoke");
    let d1 = s.document_publish(&dev, app.clone()).await.unwrap();
    assert_eq!(d1.version, 1);
    assert!(path.join("docs").join(&d1.id).join("pdf.pdf").exists());
    assert!(path
        .join("docs")
        .join(&d1.id)
        .join("manifest.json")
        .exists());

    let d2 = s.document_publish(&dev, app).await.unwrap();
    assert_eq!(d2.id, d1.id, "re-publish same app keeps id");
    assert_eq!(d2.version, 2);
}
