//! Read on-device annotations and turn them into Readwise operations.
pub mod classify;
pub mod coords;
pub mod textlayer;

pub use classify::{classify, Plan, StrokeHit};
pub use coords::{PdfRect, Transform};
pub use textlayer::{TextLayer, Word};
