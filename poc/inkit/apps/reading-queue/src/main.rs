//! Assemble and run the reading-queue app. The framework owns the loop body
//! (`App::step`); on-device transport (rmapi push/pull) lives in the manual
//! device bar (added in a later task). For now `main` renders the initial set
//! and reports.

use inkapp::{app, DocSet, SecretStore};
use reading_queue::{update, view, App, Connectors};

fn main() {
    let key = SecretStore::open_default()
        .and_then(|mut s| s.user_key())
        .expect("open secrets store / load user key");
    let mut application = app(App)
        .connector(Connectors::persisted(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/.overlay.json"
        )))
        .update(update)
        .view(view)
        .key(key)
        .build();
    let mut set = DocSet::default();
    let rendered = application.render(&mut set).expect("render");
    println!("reading-queue: rendered {} document(s)", rendered.len());
}
