//! Bundle-fetch seam.
//!
//! rmreader is a pure library now: transport (cloud upload/download) lives in
//! the `rmapps` binary. The only thing the library still needs from the cloud is
//! the ability to *fetch* an already-deployed bundle so read-back can inspect
//! on-device annotations. The caller (rmapps) implements this over the native
//! cloud client; tests implement it with a fixture path.

use std::path::PathBuf;

/// Download the bundle for `<folder>/<name>` to a local path so read-back can
/// open it. `Ok(None)` if the document does not exist yet (e.g. first run).
pub trait BundleFetch {
    fn fetch(&self, folder: &str, name: &str) -> anyhow::Result<Option<PathBuf>>;
}
