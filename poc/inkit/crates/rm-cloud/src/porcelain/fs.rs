//! Path/listing view over a snapshot: names and parents come from each doc's metadata.

use std::sync::Arc;

use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::client::Client;
use crate::error::Result;
use crate::porcelain::docfiles::{DocFiles, Metadata};
use crate::porcelain::document::now_millis;

/// Max concurrent per-doc metadata fetches during `ls` (the cloud has no server-side
/// "list children", so listing reads every doc's metadata — done in parallel).
const LS_CONCURRENCY: usize = 16;

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
    /// Metadata for a single document (fetches only its `.metadata` blob).
    pub async fn stat(&self, id: &str) -> Result<Metadata> {
        let snap = self.snapshot().await?;
        self.metadata_from(&snap, id).await
    }

    /// List the direct children of `parent` ("" = root).
    ///
    /// The cloud has no server-side child listing, so this reads every document's
    /// `.metadata` (only that blob, not the PDF/ink) and filters by `parent`. Fetches run
    /// up to [`LS_CONCURRENCY`] at a time. Docs whose metadata can't be read are skipped.
    pub async fn ls(&self, parent: &str) -> Result<Vec<Entry>> {
        let snap = self.snapshot().await?;
        let docs: Vec<(String, String)> = snap
            .docs()
            .map(|d| (d.id.clone(), d.hash.clone()))
            .collect();

        let sem = Arc::new(Semaphore::new(LS_CONCURRENCY));
        let mut set = tokio::task::JoinSet::new();
        for (id, hash) in docs {
            let client = self.clone();
            let sem = sem.clone();
            set.spawn(async move {
                let _permit = sem.acquire_owned().await.expect("semaphore not closed");
                let meta = client.metadata_by(&hash, &id).await;
                (id, meta)
            });
        }

        let mut out = Vec::new();
        while let Some(joined) = set.join_next().await {
            let (id, meta) =
                joined.map_err(|e| crate::error::Error::Http(format!("ls join: {e}")))?;
            // Skip docs whose metadata can't be read rather than failing the whole listing.
            let Ok(meta) = meta else { continue };
            if meta.deleted || meta.parent != parent {
                continue;
            }
            out.push(Entry {
                id,
                name: meta.visible_name,
                parent: meta.parent,
                is_folder: meta.doc_type == "CollectionType",
            });
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
