//! Sidecar manifest: maps Readwise doc ids to PDF anchors. Seam for the future
//! annotation phase (page -> doc id once page numbers are known post-render).
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestItem {
    pub id: String,
    pub title: String,
    pub url: String,
    pub article_anchor: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub collection: String, // "Library" | "Feed"
    pub items: Vec<ManifestItem>,
}

impl Manifest {
    pub fn write(&self, path: &Path) -> anyhow::Result<()> {
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}
