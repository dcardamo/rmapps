//! Error types for parsing reMarkable `.rm` files.

use thiserror::Error;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur while parsing a `.rm` scene file.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// An underlying I/O error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// The file header was present but declared an unsupported version.
    #[error("unsupported .rm version: {0}")]
    UnsupportedVersion(u32),

    /// The file did not start with a recognizable reMarkable `.lines` header.
    #[error("bad or missing reMarkable .lines file header")]
    BadHeader,

    /// A structural problem was encountered while decoding the byte stream.
    #[error("parse error: {0}")]
    Parse(String),
}
