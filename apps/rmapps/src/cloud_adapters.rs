//! Shared adapters bridging the native `Cloud` to the domain crates' fetch/deploy seams.
//! Used by `digest`, `reader`, and the `watch` reactor so the mapping lives in one place.
use anyhow::Result;
use std::path::{Path, PathBuf};

use rmdigest::deploy::{Backend, CloudDoc};
use rmreader::deploy::BundleFetch;

use crate::cloud::Cloud;

/// Adapts the native cloud client to rmdigest's `Backend` seam.
pub struct CloudBackend<'a> {
    pub cloud: &'a Cloud,
}

impl Backend for CloudBackend<'_> {
    fn list(&self, root: &str, exclude_suffixes: &[String]) -> Result<Vec<CloudDoc>> {
        Ok(self
            .cloud
            .list_recursive(root, exclude_suffixes)?
            .into_iter()
            .map(|d| CloudDoc {
                path: d.path,
                name: d.name,
                folder: d.folder,
                version: Some(d.hash),
            })
            .collect())
    }
    fn fetch(&self, doc: &CloudDoc) -> Result<Option<PathBuf>> {
        self.cloud.fetch_bundle(&doc.folder, &doc.name)
    }
    fn put(&self, pdf: &Path, folder: &str, name: &str) -> Result<()> {
        self.cloud.replace(folder, name, std::fs::read(pdf)?)
    }
}

/// Adapts the native cloud client to rmreader's `BundleFetch` seam.
pub struct CloudFetch<'a> {
    pub cloud: &'a Cloud,
}

impl BundleFetch for CloudFetch<'_> {
    fn fetch(&self, folder: &str, name: &str) -> Result<Option<PathBuf>> {
        self.cloud.fetch_bundle(folder, name)
    }
}
