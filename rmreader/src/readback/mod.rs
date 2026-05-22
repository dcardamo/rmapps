//! Read on-device annotations and turn them into Readwise operations.
pub mod classify;
pub mod coords;

pub use classify::{classify, PageHighlight, Plan};
pub use coords::{PdfRect, Transform};
