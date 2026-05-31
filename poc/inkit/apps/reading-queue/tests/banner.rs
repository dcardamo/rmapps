//! A permanently-failed write surfaces as a banner document in `view` —
//! app-driven (the framework contributes nothing).

use std::sync::Arc;

use inkapp::{document_source, render_document, Key};
use inkapp_core::connector::Connector;
use inkapp_readwise_reader::{Readwise, ScriptedTransport, MAX_ATTEMPTS};
use reading_queue::{view, App, Connectors};

#[tokio::test]
async fn failed_write_surfaces_as_banner() {
    let rw = Readwise::fake().with_transport(Arc::new(ScriptedTransport::always_failing()));
    let cx = Connectors::from_arc(Arc::new(rw));

    let id = cx.readwise.queue()[0].id.clone();
    cx.readwise.archive(&id);
    for _ in 0..MAX_ATTEMPTS {
        cx.readwise.flush().await;
    }
    assert!(!cx.readwise.failed_writes().is_empty());

    let docs = view(&App, &cx);
    let banner = docs
        .0
        .iter()
        .find(|d| d.key.0 == "_banner")
        .expect("banner document present when a write failed");
    assert!(
        document_source(banner).contains("couldn't sync"),
        "banner names the sync failure"
    );

    // The banner must actually compile through Typst (locks in the escaping in
    // Banner::render — a string-only assertion can't catch malformed markup).
    let key = Key::from_bytes([42u8; 32]);
    let rendered = render_document(banner, 1, &key).expect("banner document compiles");
    assert!(
        !rendered.pdf.is_empty(),
        "banner renders to a non-empty PDF"
    );
}
