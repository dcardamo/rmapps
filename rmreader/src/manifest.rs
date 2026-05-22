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

    /// Build the embeddable manifest. `page_range` defaults to (0,0) here and is
    /// overwritten by postprocess once page numbers are known.
    pub fn to_embedded(&self) -> EmbeddedManifest {
        EmbeddedManifest {
            schema_version: 1,
            collection: self.collection.clone(),
            docs: self
                .items
                .iter()
                .map(|i| EmbeddedDoc {
                    id: i.id.clone(),
                    title: i.title.clone(),
                    url: if i.source_url.is_empty() {
                        i.url.clone()
                    } else {
                        i.source_url.clone()
                    },
                    author: i.author.clone(),
                    category: i.category.clone(),
                    page_range: i.page_range.unwrap_or(PageRange { first: 0, last: 0 }),
                })
                .collect(),
            // Filled by postprocess::finalize_pdf once page geometry is known.
            label_rects: Vec::new(),
        }
    }
}

/// Axis-aligned rectangle in PDF points, bottom-left origin (matches readback coords).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ManifestRect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// One stamped action-label column with its tap rect.
/// `kind` is one of `"inbox"`, `"archive"`, `"later"`, `"delete"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelRect {
    pub kind: String,
    pub rect: ManifestRect,
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
    #[serde(rename = "v")]
    pub schema_version: u32, // embedded-manifest schema version
    pub collection: String, // "Library" | "Feed"
    pub docs: Vec<EmbeddedDoc>,
    /// Stamped action-label column rects (inbox/archive/later/delete), filled by
    /// postprocess. Empty until postprocess runs.
    #[serde(default)]
    pub label_rects: Vec<LabelRect>,
}

impl EmbeddedManifest {
    /// doc whose page_range contains `page` (0-based).
    pub fn doc_for_page(&self, page: usize) -> Option<&EmbeddedDoc> {
        self.docs
            .iter()
            .find(|d| page >= d.page_range.first && page <= d.page_range.last)
    }
}
