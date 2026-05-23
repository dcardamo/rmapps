//! Deserialisation of the `.content` JSON sidecar file.
//!
//! The schema varies between reMarkable software versions, so we are lenient:
//! all fields default to empty/absent rather than failing on unknown keys.

use serde::Deserialize;

/// Treat an explicit JSON `null` the same as a missing field: produce `T::default()`.
///
/// `#[serde(default)]` handles a *missing* key, but when a freshly-deployed PDF
/// writes `"pages":null` serde still errors ("invalid type: null, expected a
/// sequence").  This deserializer wraps the value in `Option` so `null` unwraps
/// to the type's `Default`.
fn null_to_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// Top-level structure of a `.content` JSON file.
#[derive(Debug, Deserialize, Default)]
pub struct Content {
    /// Legacy page-id list (older firmware).
    // `default` handles a missing key; `deserialize_with` handles an explicit `null`.
    #[serde(default, deserialize_with = "null_to_default")]
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
    // Same null-tolerance as `Content::pages`.
    #[serde(default, deserialize_with = "null_to_default")]
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A freshly-deployed PDF that hasn't been opened on-device writes `"pages":null`
    /// (and similarly `"cPages":null`).  Verify that this parses without error and
    /// yields an empty page list rather than a serde type-mismatch error.
    #[test]
    fn null_pages_fields_deserialize_as_empty() {
        let json = r#"{"pages":null,"cPages":null,"customZoomPageWidth":1404,"customZoomPageHeight":1872}"#;
        let c: Content = serde_json::from_str(json).expect("should parse without error");
        assert!(
            c.page_ids().is_empty(),
            "expected empty page list, got {:?}",
            c.page_ids()
        );
        assert!(c.pages.is_empty());
        assert!(c.c_pages.is_none() || c.c_pages.as_ref().unwrap().pages.is_empty());
    }
}
