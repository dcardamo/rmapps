use inkapp_harness::observe;
use inkapp_harness::session::Session;
use inkapp_harness::tests_common::app_with_uri_link;
use tempfile::tempdir;

#[tokio::test]
async fn page_describe_includes_uri_link() {
    let dir = tempdir().unwrap();
    let mut s = Session::new_fake(dir.path()).await.unwrap();
    let dev = s.device_new(None).unwrap();
    let doc = s
        .document_publish(&dev, app_with_uri_link("d", "r1", "https://example.org"))
        .await
        .unwrap();

    let desc = observe::page_describe(&s, &doc.id, 0).unwrap();
    assert_eq!(desc.links.len(), 1, "exactly one link annotation");
    assert_eq!(
        serde_json::to_value(&desc.links[0].target).unwrap(),
        serde_json::json!("uri:https://example.org")
    );

    // Region containment should attribute the link to r1.
    let r1 = desc
        .regions
        .iter()
        .find(|r| r.name == "r1")
        .expect("r1 region");
    assert_eq!(
        r1.link.as_ref().map(|t| serde_json::to_value(t).unwrap()),
        Some(serde_json::json!("uri:https://example.org"))
    );

    let dd = observe::document_describe(&s, &doc.id).unwrap();
    assert_eq!(dd.links_per_page.iter().sum::<usize>(), 1);
}
