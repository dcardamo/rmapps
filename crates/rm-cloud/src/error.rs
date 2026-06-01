//! Error type for `rm-cloud`.

use thiserror::Error;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by the reMarkable Cloud client.
#[derive(Debug, Error)]
pub enum Error {
    /// 401 from the cloud (token missing/expired and refresh failed).
    #[error("unauthorized")]
    Unauthorized,
    /// 409 from the cloud.
    #[error("conflict")]
    Conflict,
    /// 412 — the supplied root generation was stale (CAS failure).
    #[error("wrong generation (stale root)")]
    WrongGeneration,
    /// 404 — blob, root, or document not found.
    #[error("not found")]
    NotFound,
    /// CAS commit exhausted its retry budget against persistent conflicts.
    #[error("commit failed after {0} attempts")]
    CommitExhausted(u32),
    /// 429 — the cloud rate-limited us and we exhausted the automatic Retry-After backoff.
    #[error("rate limited (429): retry budget exhausted")]
    RateLimited,
    /// A required credential was absent.
    #[error("missing credential: {0}")]
    MissingCredential(&'static str),
    /// Malformed index/JSON/bundle content.
    #[error("parse error: {0}")]
    Parse(String),
    /// A path segment / folder name was rejected by validation (empty,
    /// whitespace-only, "." / "..", or flag-like with a leading '-').
    #[error("invalid name: {0}")]
    InvalidName(String),
    /// Any other HTTP-layer failure (with the status code if present).
    #[error("http error: {0}")]
    Http(String),
    /// Underlying reqwest transport failure.
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    /// Filesystem / IO failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON (de)serialization failure.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
