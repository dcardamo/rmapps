use inkapp_core::geometry::PdfPoint;
use inkapp_harness::observe::{self, ObserveGroup};
use inkapp_harness::session::Session;
use inkapp_harness::tests_common::single_region_app;
use tempfile::tempdir;

#[tokio::test]
async fn ink_tap_persists_and_is_listed() {
    let dir = tempdir().unwrap();
    let mut s = Session::new_fake(dir.path()).await.unwrap();
    let dev = s.device_new(None).unwrap();
    let doc = s
        .document_publish(&dev, single_region_app("d"))
        .await
        .unwrap();

    s.ink_tap(&dev, &doc.id, 0, "r1").unwrap();
    let list = observe::ink_list(&s, &doc.id, 0, ObserveGroup::Flat).unwrap();
    assert_eq!(list.strokes.len(), 1);
    assert!(!list.strokes[0].highlighter);
    assert_eq!(list.strokes[0].points.len(), 1);
}

#[tokio::test]
async fn ink_swipe_appends_second_stroke() {
    let dir = tempdir().unwrap();
    let mut s = Session::new_fake(dir.path()).await.unwrap();
    let dev = s.device_new(None).unwrap();
    let doc = s
        .document_publish(&dev, single_region_app("d"))
        .await
        .unwrap();

    s.ink_tap(&dev, &doc.id, 0, "r1").unwrap();
    s.ink_swipe(&dev, &doc.id, 0, "r1").unwrap();
    let list = observe::ink_list(&s, &doc.id, 0, ObserveGroup::Flat).unwrap();
    assert_eq!(list.strokes.len(), 2);
    assert!(list.strokes[1].highlighter);
}

#[tokio::test]
async fn ink_draw_freeform() {
    let dir = tempdir().unwrap();
    let mut s = Session::new_fake(dir.path()).await.unwrap();
    let dev = s.device_new(None).unwrap();
    let doc = s
        .document_publish(&dev, single_region_app("d"))
        .await
        .unwrap();

    let points = vec![
        PdfPoint { x: 20.0, y: 50.0 },
        PdfPoint { x: 30.0, y: 60.0 },
        PdfPoint { x: 40.0, y: 50.0 },
    ];
    s.ink_draw(&dev, &doc.id, 0, &points, false).unwrap();

    let list = observe::ink_list(&s, &doc.id, 0, ObserveGroup::Flat).unwrap();
    assert_eq!(list.strokes.len(), 1);
    assert_eq!(list.strokes[0].points.len(), 3);
}
