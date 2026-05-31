use inkapp_harness::observe;
use inkapp_harness::session::Session;
use inkapp_harness::tests_common::single_region_app;
use tempfile::tempdir;

#[tokio::test]
async fn page_describe_returns_regions_from_manifest() {
    let dir = tempdir().unwrap();
    let mut s = Session::new_fake(dir.path()).await.unwrap();
    let dev = s.device_new(None).unwrap();
    let doc = s
        .document_publish(&dev, single_region_app("d1"))
        .await
        .unwrap();

    let desc = observe::page_describe(&s, &doc.id, 0).unwrap();
    assert_eq!(desc.regions.len(), 1);
    assert_eq!(desc.regions[0].name, "r1");
    assert_eq!(desc.version, 1);
    assert!(desc.links.is_empty(), "links populated in Task 6");
}

#[tokio::test]
async fn document_describe_returns_summary() {
    let dir = tempdir().unwrap();
    let mut s = Session::new_fake(dir.path()).await.unwrap();
    let dev = s.device_new(None).unwrap();
    let doc = s
        .document_publish(&dev, single_region_app("d2"))
        .await
        .unwrap();

    let desc = observe::document_describe(&s, &doc.id).unwrap();
    assert_eq!(desc.app_name, "d2");
    assert_eq!(desc.version, 1);
    assert!(desc.pages >= 1);
    assert_eq!(desc.regions_per_page.iter().sum::<usize>(), 1);
}
