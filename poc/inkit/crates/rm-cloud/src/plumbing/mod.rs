//! Plumbing: the content-addressed primitives (indexes, hashing, blobs, snapshots,
//! commit). Higher layers (porcelain, sync) are built on these.

pub mod index;
pub mod snapshot;
