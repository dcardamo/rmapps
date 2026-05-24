use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use inkapp_core::connector::{Connector, ConnectorSet};
use inkapp_core::crypto::Key;
use inkapp_core::document::{DocKey, Document, Documents};
use inkapp_core::flow;
use inkapp_core::geometry::PdfPoint;
use inkapp_core::ink::Stroke;
use inkapp_core::reconcile::DocOp;
use inkapp_core::runtime::{app, DocSet};
use inkapp_core::widgets::checkbox::Checkbox;

#[derive(Clone, PartialEq, Eq, Debug)]
enum Msg {
    Archive(String),
}

// A trivial connector: a queue of ids, with an archived set (interior mutability
// so `&self` writes work, as the real connectors will).
struct Db {
    archived: RefCell<Vec<String>>,
}
struct Cx {
    db: Db,
}
impl Cx {
    fn fake() -> Self {
        Cx {
            db: Db {
                archived: RefCell::new(Vec::new()),
            },
        }
    }
    fn queue(&self) -> Vec<String> {
        ["a", "b", "c"]
            .iter()
            .map(|s| s.to_string())
            .filter(|id| !self.db.archived.borrow().contains(id))
            .collect()
    }
}

impl ConnectorSet for Cx {
    fn connectors(&self) -> Vec<Arc<dyn Connector>> {
        vec![]
    }
}

struct Model;

fn update(msg: Msg, _m: &mut Model, cx: &Cx) {
    match msg {
        Msg::Archive(id) => cx.db.archived.borrow_mut().push(id),
    }
}

fn view(_m: &Model, cx: &Cx) -> Documents<Msg> {
    Documents(
        cx.queue()
            .into_iter()
            .map(|id| {
                Document::keyed(
                    id.clone(),
                    flow![Checkbox::with_msg("done", Msg::Archive(id.clone())).label("Archive")],
                )
            })
            .collect::<Vec<_>>(),
    )
}

/// Build a manifest-attributed mark in the "done" region of `key`'s doc.
fn ink_for(set: &DocSet, key: &str) -> Vec<Stroke> {
    let m = set.manifest(&DocKey::new(key)).expect("rendered doc");
    let r = m
        .regions
        .iter()
        .find(|r| r.name == "done")
        .expect("done region");
    let cx = (r.rect.x0 + r.rect.x1) / 2.0;
    let cy = (r.rect.y0 + r.rect.y1) / 2.0;
    vec![Stroke {
        points: vec![PdfPoint { x: cx, y: cy }],
        highlighter: false,
    }]
}

#[tokio::test]
async fn two_cycle_archive_and_delete() {
    let mut app = app(Model)
        .connector(Cx::fake())
        .update(update)
        .view(view)
        .key(Key::from_bytes([9u8; 32]))
        .build();
    let mut set = DocSet::default();

    // Cycle 0: initial render -> a, b, c.
    let rendered = app.render(&mut set).await.unwrap();
    assert_eq!(rendered.len(), 3);

    // Draw: archive "b" and "c" (mark their checkboxes).
    let mut ink: HashMap<String, Vec<Stroke>> = HashMap::new();
    ink.insert("b".into(), ink_for(&set, "b"));
    ink.insert("c".into(), ink_for(&set, "c"));

    // Cycle 1: step.
    let cycle = app.step(&mut set, &ink).await.unwrap();

    assert!(cycle.decoded.contains(&Msg::Archive("b".into())));
    assert!(cycle.decoded.contains(&Msg::Archive("c".into())));

    // Next view drops archived b and c -> both Deleted; a is a no-op.
    assert!(cycle.ops.contains(&DocOp::Delete(DocKey::new("b"))));
    assert!(cycle.ops.contains(&DocOp::Delete(DocKey::new("c"))));
    assert!(!cycle.ops.iter().any(|o| matches!(o, DocOp::Create(_))));

    // DocSet now holds only "a".
    let mut keys: Vec<String> = set.keys().into_iter().map(|k| k.0).collect();
    keys.sort();
    assert_eq!(keys, vec!["a".to_string()]);
}

#[tokio::test]
async fn surviving_key_entry_retained() {
    let mut app = app(Model)
        .connector(Cx::fake())
        .update(update)
        .view(view)
        .key(Key::from_bytes([9u8; 32]))
        .build();
    let mut set = DocSet::default();
    app.render(&mut set).await.unwrap();

    // Mark only "b"; a and c survive.
    let mut ink: HashMap<String, Vec<Stroke>> = HashMap::new();
    ink.insert("b".into(), ink_for(&set, "b"));
    app.step(&mut set, &ink).await.unwrap();

    assert!(set.manifest(&DocKey::new("a")).is_some());
    assert!(set.manifest(&DocKey::new("c")).is_some());
    assert!(
        set.manifest(&DocKey::new("b")).is_none(),
        "archived b dropped"
    );
}
