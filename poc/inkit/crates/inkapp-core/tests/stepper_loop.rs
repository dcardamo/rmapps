use std::collections::HashMap;
use std::sync::Arc;

use inkapp_core::components::stepper::Stepper;
use inkapp_core::connector::{Connector, ConnectorSet};
use inkapp_core::crypto::Key;
use inkapp_core::document::{DocKey, Document, Documents};
use inkapp_core::flow;
use inkapp_core::geometry::PdfPoint;
use inkapp_core::ink::Stroke;
use inkapp_core::runtime::{app, DocSet};

// Model is just the current counter value; Msg is the decoded new count.
type Model = u64;
type Msg = u64;

struct Cx;
impl ConnectorSet for Cx {
    fn connectors(&self) -> Vec<Arc<dyn Connector>> {
        vec![]
    }
}

fn update(msg: Msg, m: &mut Model, _cx: &Cx) {
    *m = msg;
}

fn view(m: &Model, _cx: &Cx) -> Documents<Msg> {
    Documents(vec![Document::keyed("c", flow![Stepper::new("c", *m)])])
}

fn tick(set: &DocSet, key: &str) -> Vec<Stroke> {
    let m = set.manifest(&DocKey::new(key)).expect("rendered doc");
    let r = m
        .regions
        .iter()
        .find(|r| r.name == "stepper:c")
        .expect("stepper region");
    let cx = (r.rect.x0 + r.rect.x1) / 2.0;
    let cy = (r.rect.y0 + r.rect.y1) / 2.0;
    vec![Stroke {
        points: vec![PdfPoint { x: cx, y: cy }],
        highlighter: false,
    }]
}

#[tokio::test]
async fn decode_uses_carried_base_through_loop() {
    let mut app = app(5u64)
        .connector(Cx)
        .update(update)
        .view(view)
        .key(Key::from_bytes([9u8; 32]))
        .build();
    let mut set = DocSet::default();

    // Cycle 0: render at count 5 -> stored manifest carries base 5.
    app.render(&mut set).await.unwrap();

    // Server state moves on to 9 with NO re-render (the device still shows 5).
    app.model = 9;

    // The user inks one increment on the stale (base-5) document.
    let mut ink: HashMap<String, Vec<Vec<Stroke>>> = HashMap::new();
    ink.insert("c".into(), vec![tick(&set, "c")]);

    let cycle = app.step(&mut set, &ink).await.unwrap();

    // Decoded against carried base 5 (=5+1=6), NOT current model 9 (=10).
    assert_eq!(cycle.decoded, vec![6u64]);
}
