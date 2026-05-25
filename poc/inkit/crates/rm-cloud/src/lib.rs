//! `rm-cloud` — a pure-Rust client for the current reMarkable Cloud sync protocol.
//!
//! The cloud is a content-addressed store (blobs keyed by sha256, a root ref with a
//! compare-and-swap generation). This crate exposes it as immutable snapshots with
//! `diff`, an atomic commit, rmapi-style path operations, and a declarative working-set
//! sync for app loops. See `docs/rm-cloud-protocol.md`.

#![warn(missing_docs)]

mod config;
mod error;

pub use config::Config;
pub use error::{Error, Result};
