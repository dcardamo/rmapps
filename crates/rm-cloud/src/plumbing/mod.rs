//! Plumbing: the content-addressed primitives (indexes, hashing, blobs, snapshots,
//! commit). Higher layers (porcelain, sync) are built on these.

pub(crate) mod blob;
pub mod commit;
pub mod index;
pub mod snapshot;
