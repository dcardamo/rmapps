//! Cross-app connector sharing: two apps holding clones of one `Arc<Readwise>`
//! share its cache and write queue, so a write through one is seen by the other.

use std::sync::Arc;

use inkapp_readwise::Readwise;
use reading_queue::Connectors;

#[test]
fn two_apps_share_one_connector_cache() {
    let shared = Arc::new(Readwise::fake());
    let app_a = Connectors::from_arc(shared.clone());
    let app_b = Connectors::from_arc(shared.clone());

    let id = app_a.readwise.queue()[0].id.clone();
    let before = app_b.readwise.queue().len();

    // Write through app A's handle…
    app_a.readwise.archive(&id);

    // …and app B sees it, because they share one connector.
    assert_eq!(
        app_b.readwise.queue().len(),
        before - 1,
        "B observes A's archive through the shared connector"
    );
    assert!(
        app_b.readwise.queue().iter().all(|x| x.id != id),
        "the archived article is gone from B's queue too"
    );
}
