//! Path/listing view over a snapshot: names and parents come from each doc's metadata.

use std::sync::Arc;

use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::client::Client;
use crate::error::{Error, Result};
use crate::plumbing::snapshot::Snapshot;
use crate::porcelain::docfiles::{DocFiles, Metadata};
use crate::porcelain::document::now_millis;

/// Max concurrent per-doc metadata fetches during `ls` (the cloud has no server-side
/// "list children", so listing reads every doc's metadata — done in parallel).
const LS_CONCURRENCY: usize = 16;

/// Reject path segments / folder names that should never be turned into a folder.
///
/// A segment is invalid if it is empty, whitespace-only, the relative markers `.`
/// or `..`, or "flag-like" (starts with `-`). The last case is the important one:
/// it stops a stray CLI argument such as `--help` from being silently materialised
/// as a folder on the reMarkable account when a path string is passed through to
/// [`Client::mkdir_p`].
fn validate_segment(segment: &str) -> Result<()> {
    if segment.trim().is_empty() || segment == "." || segment == ".." || segment.starts_with('-') {
        return Err(Error::InvalidName(segment.to_string()));
    }
    Ok(())
}

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
    /// Cloud content hash of the document (changes when any blob changes).
    pub hash: String,
}

impl Client {
    /// Metadata for a single document (fetches only its `.metadata` blob).
    pub async fn stat(&self, id: &str) -> Result<Metadata> {
        let snap = self.snapshot().await?;
        self.metadata_from(&snap, id).await
    }

    /// List direct children of `parent` against an already-fetched snapshot.
    ///
    /// The cloud has no server-side child listing, so this reads every document's
    /// `.metadata` (only that blob, not the PDF/ink) and filters by `parent`. Fetches run
    /// up to [`LS_CONCURRENCY`] at a time. Docs whose metadata can't be read are skipped.
    /// Because the snapshot is provided by the caller, no generation poll is issued.
    pub async fn ls_with(&self, snap: &Snapshot, parent: &str) -> Result<Vec<Entry>> {
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
                (id, hash, meta)
            });
        }

        let mut out = Vec::new();
        while let Some(joined) = set.join_next().await {
            let (id, hash, meta) =
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
                hash,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// List the direct children of `parent` ("" = root). Snapshots, then delegates to
    /// [`Self::ls_with`].
    pub async fn ls(&self, parent: &str) -> Result<Vec<Entry>> {
        let snap = self.snapshot().await?;
        self.ls_with(&snap, parent).await
    }

    /// Resolve a slash-separated folder path to its folder id, creating any missing
    /// segments along the way (like `mkdir -p`). A leading slash is optional; an empty
    /// path (or `"/"`) resolves to the root (`""`). Each segment is matched
    /// case-sensitively against existing `CollectionType` children. This is the
    /// path→id bridge for callers that think in rmapi-style paths (e.g. a device
    /// transport deploying under `/ReadingQueue`).
    pub async fn mkdir_p(&self, path: &str) -> Result<String> {
        let mut parent = String::new();
        for segment in path.split('/').filter(|s| !s.is_empty()) {
            validate_segment(segment)?;
            let existing = self
                .ls(&parent)
                .await?
                .into_iter()
                .find(|e| e.is_folder && e.name == segment);
            parent = match existing {
                Some(e) => e.id,
                None => self.mkdir(segment, &parent).await?,
            };
        }
        Ok(parent)
    }

    /// Create a folder under `parent`; returns the new folder id.
    pub async fn mkdir(&self, name: &str, parent: &str) -> Result<String> {
        validate_segment(name)?;
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

#[cfg(all(test, feature = "fake"))]
mod ls_with_tests {
    use crate::client::Client;
    use crate::config::Config;
    use crate::fake::FakeCloud;
    use crate::porcelain::docfiles::DocFiles;

    #[tokio::test]
    async fn recursive_listing_reuses_one_snapshot() {
        let fake = FakeCloud::spawn().await;
        let client = Client::from_user_token(Config::single_host(&fake.base), "user-token");

        let a = client.mkdir("FolderA", "").await.unwrap();
        let b = client.mkdir("FolderB", "").await.unwrap();
        client.put(DocFiles::new_pdf("DocA", &a, b"%PDF\n".to_vec())).await.unwrap();
        client.put(DocFiles::new_pdf("DocB", &b, b"%PDF\n".to_vec())).await.unwrap();

        let snap = client.snapshot().await.unwrap();
        let root_hash = snap.root_hash.clone();
        let before = fake.blob_get_count(&root_hash);

        let _ = client.ls_with(&snap, "").await.unwrap();
        let _ = client.ls_with(&snap, &a).await.unwrap();
        let _ = client.ls_with(&snap, &b).await.unwrap();

        assert_eq!(
            fake.blob_get_count(&root_hash), before,
            "ls_with must not refetch the root index per folder"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::validate_segment;
    use crate::error::Error;

    #[test]
    fn accepts_ordinary_names() {
        for name in ["ReadingQueue", "rmapps-test", "May 2026", "a.b.c", "-leading-only-bad"]
            .iter()
            .filter(|n| !n.starts_with('-'))
        {
            assert!(validate_segment(name).is_ok(), "{name} should be valid");
        }
    }

    #[test]
    fn rejects_empty_and_whitespace() {
        for bad in ["", "   ", "\t", "\n"] {
            assert!(
                matches!(validate_segment(bad), Err(Error::InvalidName(_))),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn rejects_dot_segments() {
        assert!(matches!(validate_segment("."), Err(Error::InvalidName(_))));
        assert!(matches!(validate_segment(".."), Err(Error::InvalidName(_))));
    }

    #[test]
    fn rejects_flag_like_names() {
        for bad in ["--help", "-h", "-rf", "--recursive"] {
            assert!(
                matches!(validate_segment(bad), Err(Error::InvalidName(_))),
                "{bad} should be rejected as flag-like"
            );
        }
    }
}
