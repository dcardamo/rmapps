//! Shared helpers for reader tests — not itself a test file.
#![allow(dead_code)]

use std::sync::Arc;

use inkapp::app;
use inkapp_core::crypto::Key;
use inkapp_core::runtime::App;
use inkapp_core::theme::Theme;
use inkapp_readwise_reader::Readwise;
use reader::{update, view, App as Model, Connectors, Msg};

/// Build a test App wired to the fake Readwise cassette.
pub fn fake_app() -> App<Model, Msg, Connectors> {
    app(Model)
        .connector(Connectors::fake())
        .update(update)
        .view(view)
        .key(Key::from_bytes([0u8; 32]))
        .theme(Theme::reader())
        .build()
}

/// Build a reader App wired to the fake cassette, returning both the App and
/// the underlying `Arc<Readwise>` so loop tests can introspect the connector
/// overlay (archived ids, highlights, etc.) without needing an accessor on
/// `App` itself.
pub fn reader_app_fake_with_readwise() -> (App<Model, Msg, Connectors>, Arc<Readwise>) {
    let readwise = Arc::new(Readwise::fake());
    let cx = Connectors {
        readwise: readwise.clone(),
    };
    let app = app(Model)
        .connector(cx)
        .update(update)
        .view(view)
        .key(Key::from_bytes([0u8; 32]))
        .theme(Theme::reader())
        .build();
    (app, readwise)
}
