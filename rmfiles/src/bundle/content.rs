//! Deserialisation of the `.content` JSON sidecar file.
//!
//! The schema varies between reMarkable software versions, so we are lenient:
//! all fields default to empty/absent rather than failing on unknown keys.

use serde::Deserialize;

/// Top-level structure of a `.content` JSON file.
#[derive(Debug, Deserialize, Default)]
pub struct Content {
    /// Legacy page-id list (older firmware).
    #[serde(default)]
    pub pages: Vec<String>,

    /// Newer `cPages` structure (recent firmware / Paper Pro).
    #[serde(default, rename = "cPages")]
    pub c_pages: Option<CPages>,

    /// Canvas width in device pixels.
    #[serde(default, rename = "customZoomPageWidth")]
    pub page_width: Option<f64>,

    /// Canvas height in device pixels.
    #[serde(default, rename = "customZoomPageHeight")]
    pub page_height: Option<f64>,
}

/// Container for the newer `cPages` page list.
#[derive(Debug, Deserialize, Default)]
pub struct CPages {
    /// Page entries in reading order.
    #[serde(default)]
    pub pages: Vec<CPage>,
}

/// A single page entry in the `cPages` list.
#[derive(Debug, Deserialize)]
pub struct CPage {
    /// The page UUID.
    pub id: String,
}

impl Content {
    /// Return page IDs in reading order, preferring `cPages` when available.
    pub fn page_ids(&self) -> Vec<String> {
        if let Some(cp) = &self.c_pages {
            if !cp.pages.is_empty() {
                return cp.pages.iter().map(|p| p.id.clone()).collect();
            }
        }
        self.pages.clone()
    }
}
