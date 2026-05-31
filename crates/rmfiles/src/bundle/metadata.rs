//! Deserialisation of the `.metadata` JSON sidecar file.

use serde::Deserialize;

/// Human-readable document metadata from the `.metadata` JSON file.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct Metadata {
    /// The name shown in the reMarkable UI.
    #[serde(default, rename = "visibleName")]
    pub visible_name: String,

    /// Unix-millisecond timestamp of the last modification (as a string).
    #[serde(default, rename = "lastModified")]
    pub last_modified: String,

    /// Document type, e.g. `"DocumentType"`.
    #[serde(default, rename = "type")]
    pub doc_type: String,
}
