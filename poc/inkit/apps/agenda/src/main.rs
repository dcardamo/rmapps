//! Assemble and run the agenda app. The framework owns the loop body; on-device
//! transport (rmapi) lives in the manual device bar (`serve`). For now `main`
//! renders the initial set and reports.

use agenda::{update, view, App, Connectors};
use inkapp::{app, DocSet, SecretStore};

#[tokio::main]
async fn main() {
    let key = SecretStore::open_default()
        .and_then(|mut s| s.user_key())
        .expect("open secrets store / load user key");
    let mut application = app(App)
        .connector(Connectors::persisted(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/.localcal.json"
        )))
        .update(update)
        .view(view)
        .key(key)
        .build();
    let mut set = DocSet::default();
    let rendered = application.render(&mut set).await.expect("render");
    println!("agenda: rendered {} document(s)", rendered.len());
}
