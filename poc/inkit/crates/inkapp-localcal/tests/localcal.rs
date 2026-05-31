use inkapp_core::connector::Connector;
use inkapp_localcal::LocalCal;
use std::sync::Arc;
use tempfile::NamedTempFile;

#[tokio::test]
async fn cancel_is_optimistically_visible_same_render() {
    let c = LocalCal::fake();
    let before = c.events();
    assert!(before.iter().all(|e| !e.cancelled), "starts uncancelled");
    let uid = before[0].uid.clone();
    c.cancel(&uid);
    let after = c.events();
    assert!(
        after.iter().find(|e| e.uid == uid).unwrap().cancelled,
        "cancel visible before flush"
    );
}

#[tokio::test]
async fn flush_persists_and_survives_reload() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    let uid;
    {
        let c = LocalCal::persisted(&path);
        uid = c.events()[0].uid.clone();
        c.cancel(&uid);
        c.flush().await;
    }
    let c2 = LocalCal::persisted(&path);
    assert!(
        c2.events().iter().find(|e| e.uid == uid).unwrap().cancelled,
        "cancel persisted across reload"
    );
}

#[tokio::test]
async fn refresh_preserves_pending_overlay() {
    let c = Arc::new(LocalCal::fake());
    let uid = c.events()[0].uid.clone();
    c.cancel(&uid);
    c.refresh().await.unwrap();
    assert!(
        c.events().iter().find(|e| e.uid == uid).unwrap().cancelled,
        "pending (un-flushed) cancel survives refresh"
    );
}
