//! Assemble and run the reading-queue app. The framework owns the loop body
//! (`App::step`); on-device transport (rmapi push/pull) lives in the manual
//! device bar (added in a later task). For now `main` renders the initial set
//! and reports.

use inkapp::{app, DocSet};
use reading_queue::{update, view, App, Connectors};

fn main() {
    let mut application = app(App)
        .connector(Connectors::from_cassette())
        .update(update)
        .view(view)
        .build();
    let mut set = DocSet::default();
    let rendered = application.render(&mut set).expect("render");
    println!("reading-queue: rendered {} document(s)", rendered.len());
}
