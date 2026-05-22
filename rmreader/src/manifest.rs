//! Sidecar manifest: maps Readwise doc ids to PDF anchors. Seam for the future
//! annotation phase (page -> doc id once page numbers are known post-render).
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 0-based inclusive PDF page range an article occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRange {
    pub first: usize,
    pub last: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestItem {
    pub id: String,
    pub title: String,
    pub url: String,
    pub article_anchor: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub source_url: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub page_range: Option<PageRange>,
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

/// Per-doc record embedded in the PDF for read-back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddedDoc {
    pub id: String,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub category: String,
    pub page_range: PageRange,
}

/// The self-describing manifest embedded inside each generated PDF.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddedManifest {
    pub v: u32,             // schema version (1)
    pub collection: String, // "Library" | "Feed"
    pub docs: Vec<EmbeddedDoc>,
}

impl EmbeddedManifest {
    /// doc whose page_range contains `page` (0-based).
    pub fn doc_for_page(&self, page: usize) -> Option<&EmbeddedDoc> {
        self.docs
            .iter()
            .find(|d| page >= d.page_range.first && page <= d.page_range.last)
    }
}
