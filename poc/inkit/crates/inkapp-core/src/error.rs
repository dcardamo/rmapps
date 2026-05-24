/// Errors produced by the framework core.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("typst compile failed: {0}")]
    Compile(String),
    #[error("typst pdf export failed: {0}")]
    Pdf(String),
    #[error("region recovery failed: {0}")]
    Region(String),
    #[error("manifest (de)serialisation failed: {0}")]
    Manifest(String),
    #[error("readback failed: {0}")]
    Readback(String),
    #[error("render/raster failed: {0}")]
    Render(String),
    #[error("encryption/decryption failed: {0}")]
    Crypto(String),
}

pub type Result<T> = std::result::Result<T, Error>;
