use inkapp_harness::observe::{self, ObserveGroup};
use inkapp_harness::session::Session;
use inkapp_harness::tests_common::single_region_app;
use tempfile::tempdir;

#[tokio::test]
async fn device_tree_includes_published_doc() {
    let dir = tempdir().unwrap();
    let mut s = Session::new_fake(dir.path()).await.unwrap();
    let dev = s.device_new(None).unwrap();
    let doc = s
        .document_publish(&dev, single_region_app("tree-doc"))
        .await
        .unwrap();

    let tree = observe::device_tree(&s, dev.as_str(), "/").await.unwrap();
    let names = collect_names(&tree.root);
    assert!(
        names.iter().any(|n| n.contains("tree-doc")),
        "expected doc in tree, got: {names:?}"
    );
    let _ = doc;
}

fn collect_names(node: &observe::DeviceTreeNode) -> Vec<String> {
    let mut v = vec![node.name.clone()];
    for c in &node.children {
        v.extend(collect_names(c));
    }
    v
}

#[tokio::test]
async fn ink_list_empty_when_no_pending() {
    let dir = tempdir().unwrap();
    let mut s = Session::new_fake(dir.path()).await.unwrap();
    let dev = s.device_new(None).unwrap();
    let doc = s
        .document_publish(&dev, single_region_app("d"))
        .await
        .unwrap();

    let list = observe::ink_list(&s, &doc.id, 0, ObserveGroup::Flat).unwrap();
    assert!(list.strokes.is_empty());
    assert!(list.by_layer.is_none());
    assert!(list.by_region.is_none());
    let _ = dev;
}
