use agenda::{update, view, App, Connectors, Msg};
use inkapp::document_source;

#[test]
fn view_renders_one_document_with_an_editable_region() {
    let cx = Connectors::fake();
    let docs = view(&App, &cx);
    assert_eq!(docs.0.len(), 1, "one agenda document");
    let src = document_source(&docs.0[0]);
    // The editable (local) calendar mints per-event regions; the read-only feed
    // mints none — so the only regions present come from the editable calendar.
    assert!(src.contains("name: \"evt-0\""), "editable calendar has regions: {src}");
}

#[test]
fn cancel_routes_to_local_calendar() {
    let cx = Connectors::fake();
    let uid = cx.cal.events()[0].uid.clone();
    let mut m = App;
    update(Msg::EventCancelled { uid: uid.clone() }, &mut m, &cx);
    assert!(
        cx.cal.events().iter().find(|e| e.uid == uid).unwrap().cancelled,
        "cancel reached the writable calendar"
    );
}
