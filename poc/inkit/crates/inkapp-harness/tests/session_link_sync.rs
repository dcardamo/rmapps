//! Task 12 — `Session::link_follow` resolves a region's link target and
//! `Session::device_sync` reports the cloud's visible docs.

use inkapp_harness::session::Session;
use inkapp_harness::tests_common::{app_with_uri_link, single_region_app};
use tempfile::tempdir;

#[tokio::test]
async fn link_follow_returns_uri() {
    let dir = tempdir().unwrap();
    let mut s = Session::new_fake(dir.path()).await.unwrap();
    let dev = s.device_new(None).unwrap();
    let doc = s
        .document_publish(&dev, app_with_uri_link("d", "r1", "https://example.org"))
        .await
        .unwrap();

    let r = s.link_follow(&dev, &doc.id, 0, "r1").unwrap();
    assert_eq!(r.target_uri.as_deref(), Some("https://example.org"));
    assert_eq!(r.target_page, None);
}

#[tokio::test]
async fn device_sync_lists_published_doc() {
    let dir = tempdir().unwrap();
    let mut s = Session::new_fake(dir.path()).await.unwrap();
    let dev = s.device_new(None).unwrap();
    let _ = s
        .document_publish(&dev, single_region_app("synced"))
        .await
        .unwrap();

    let r = s.device_sync(&dev).await.unwrap();
    assert!(
        r.pulled.iter().any(|n| n == "synced"),
        "expected synced doc in pulled list: {:?}",
        r.pulled
    );
    assert!(r.pushed.is_empty());
    assert!(r.conflicts.is_empty());

    // sync_cursor should now be set.
    let devs = s.device_list().unwrap();
    assert!(devs
        .iter()
        .any(|d| d.id == dev.as_str() && d.sync_cursor.is_some()));
}
