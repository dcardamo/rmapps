use inkapp_harness::observe;
use inkapp_harness::session::Session;
use inkapp_harness::tests_common::single_region_app;
use tempfile::tempdir;

#[tokio::test]
async fn page_snapshot_returns_valid_png() {
    let dir = tempdir().unwrap();
    let mut s = Session::new_fake(dir.path()).await.unwrap();
    let dev = s.device_new(None).unwrap();
    let doc = s
        .document_publish(&dev, single_region_app("snap"))
        .await
        .unwrap();

    let png = observe::page_snapshot(&s, &doc.id, 0).unwrap();
    assert!(png.len() > 8);
    assert_eq!(&png[0..4], &[0x89, 0x50, 0x4E, 0x47], "PNG magic header");
}
