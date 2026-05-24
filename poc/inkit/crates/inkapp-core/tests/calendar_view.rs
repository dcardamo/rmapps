use inkapp_core::calendar::EventRow;
use inkapp_core::component::Component;
use inkapp_core::components::calendar_view::CalendarView;
use inkapp_core::crypto::Key;
use inkapp_core::document::Document;
use inkapp_core::flow;
use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::{Manifest, Region};
use inkapp_core::runtime::render_document;
use inkapp_core::widget::RenderCx;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Msg {
    Cancel(String),
}

fn ev(uid: &str, summary: &str) -> EventRow {
    EventRow {
        uid: uid.into(),
        summary: summary.into(),
        start: "20260524T090000Z".into(),
        end: "20260524T100000Z".into(),
        cancelled: false,
    }
}

#[test]
fn read_only_emits_no_region_or_affordance() {
    let cv = CalendarView::<Msg>::read_only(vec![ev("e1", "Standup")]);
    let src = cv.render(&mut RenderCx::new(0));
    assert!(!src.contains("<region>"), "read-only renders no region: {src}");
    assert!(!src.contains("rect("), "read-only renders no affordance box: {src}");
    assert!(src.contains("Standup"), "event text present: {src}");
}

#[test]
fn editable_emits_region_and_affordance_per_event() {
    let cv = CalendarView::editable(vec![ev("e1", "Standup"), ev("e2", "Review")], |uid| {
        Msg::Cancel(uid.to_string())
    });
    let src = cv.render(&mut RenderCx::new(0));
    assert!(src.contains("name: \"evt-0\""), "first event region: {src}");
    assert!(src.contains("name: \"evt-1\""), "second event region: {src}");
    assert_eq!(src.matches("rect(").count(), 2, "one affordance box per event");
}

#[test]
fn both_modes_compile_through_typst() {
    let key = Key::from_bytes([0u8; 32]);
    let ro: Document<Msg> = Document::keyed("ro", flow![CalendarView::<Msg>::read_only(vec![ev("e1", "Standup")])]);
    let ed: Document<Msg> = Document::keyed("ed", flow![CalendarView::editable(vec![ev("e1", "Standup")], |uid| Msg::Cancel(uid.to_string()))]);
    assert!(render_document(&ro, 1, &key).is_ok(), "read-only compiles");
    assert!(render_document(&ed, 1, &key).is_ok(), "editable compiles");
}

#[test]
fn read_only_decodes_nothing_editable_decodes_cancel() {
    let manifest = Manifest {
        version: 1,
        regions: vec![Region {
            name: "evt-0".into(),
            page: 0,
            rect: PdfRect { x0: 0.0, y0: 0.0, x1: 14.0, y1: 14.0 },
        }],
    };
    let ink = vec![RegionInk {
        region: "evt-0".into(),
        strokes: vec![Stroke {
            points: vec![PdfPoint { x: 7.0, y: 7.0 }],
            highlighter: false,
        }],
    }];

    let ro = CalendarView::<Msg>::read_only(vec![ev("e1", "Standup")]);
    assert!(ro.decode(&ink, &manifest).is_empty(), "read-only discards ink");

    let ed = CalendarView::editable(vec![ev("e1", "Standup")], |uid| Msg::Cancel(uid.to_string()));
    assert_eq!(ed.decode(&ink, &manifest), vec![Msg::Cancel("e1".to_string())]);
}
