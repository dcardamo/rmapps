//! Shared helpers for reader tests — not itself a test file.
#![allow(dead_code)]

use inkapp::app;
use inkapp_core::crypto::Key;
use inkapp_core::runtime::App;
use inkapp_core::theme::Theme;
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
