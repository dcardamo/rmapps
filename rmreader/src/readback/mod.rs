//! Read on-device annotations and turn them into Readwise operations.
pub mod classify;

pub use classify::{classify, PageHighlight, Plan};
