//! `rm-cloud` — a pure-Rust client for the current reMarkable Cloud sync protocol.
//!
//! The cloud is a content-addressed store (blobs keyed by sha256, a root ref with a
//! compare-and-swap generation). This crate exposes it as immutable snapshots with
//! `diff`, an atomic commit, rmapi-style path operations, and a declarative working-set
//! sync for app loops. See `docs/rm-cloud-protocol.md`.

#![warn(missing_docs)]

mod auth;
mod config;
mod error;
mod plumbing;
mod transport;

#[cfg(feature = "fake")]
pub mod fake;

pub use auth::{refresh_user_token, register_device, Credentials};
pub use config::Config;
pub use error::{Error, Result};
pub use plumbing::index::{
    doc_hash, doc_size, parse_doc_index, parse_root_index, root_hash, serialize_doc_index,
    serialize_root_index, sha256_hex, DocEntry, FileEntry,
};
pub use plumbing::snapshot::{DocRef, Snapshot, TreeDiff};
