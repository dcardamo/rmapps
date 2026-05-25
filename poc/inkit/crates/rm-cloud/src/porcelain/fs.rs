//! Path/listing view over a snapshot: names and parents come from each doc's metadata.

use uuid::Uuid;

use crate::client::Client;
use crate::error::Result;
use crate::porcelain::docfiles::{DocFiles, Metadata};
use crate::porcelain::document::now_millis;

/// One entry in a directory listing.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Document id.
    pub id: String,
    /// Visible name.
    pub name: String,
    /// Parent id ("" = root, "trash" = trash).
    pub parent: String,
    /// True if a folder (`CollectionType`).
    pub is_folder: bool,
}

impl Client {
    /// Metadata for a single document.
    pub async fn stat(&self, id: &str) -> Result<Metadata> {
        self.get(id).await?.metadata()
    }

    /// List the direct children of `parent` ("" = root).
    pub async fn ls(&self, parent: &str) -> Result<Vec<Entry>> {
        let snap = self.snapshot().await?;
        let mut out = Vec::new();
        for doc in snap.docs() {
            // Fetch each doc's metadata (small blobs). Acceptable for v1; an index-level
            // metadata cache is a later optimization.
            let meta = self.get(&doc.id).await?.metadata()?;
            if meta.deleted {
                continue;
            }
            if meta.parent == parent {
                out.push(Entry {
                    id: doc.id.clone(),
                    name: meta.visible_name,
                    parent: meta.parent,
                    is_folder: meta.doc_type == "CollectionType",
                });
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Create a folder under `parent`; returns the new folder id.
    pub async fn mkdir(&self, name: &str, parent: &str) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let meta = Metadata {
            visible_name: name.to_string(),
            doc_type: "CollectionType".to_string(),
            parent: parent.to_string(),
            last_modified: now_millis(),
            deleted: false,
            extra: Default::default(),
        };
        let files = vec![
            (format!("{id}.metadata"), serde_json::to_vec(&meta)?),
            (format!("{id}.content"), b"{}".to_vec()),
        ];
        self.put(DocFiles {
            id: id.clone(),
            files,
        })
        .await?;
        Ok(id)
    }
}
