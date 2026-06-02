//! Path/listing view over a snapshot: names and parents come from each doc's metadata.

use uuid::Uuid;

use crate::client::Client;
use crate::error::{Error, Result};
use crate::porcelain::docfiles::{DocFiles, Metadata};
use crate::porcelain::document::now_millis;

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

    /// List the direct children of `parent` ("" = root), sourced from the resolved sync
    /// index. One generation poll; metadata is read only for docs whose hash moved.
    pub async fn ls(&self, parent: &str) -> Result<Vec<Entry>> {
        let tree = self.resolved_snapshot().await?;
        Ok(tree.children(parent))
    }

    /// Resolve a slash-separated folder path to its folder id, creating any missing
    /// segments along the way (like `mkdir -p`). A leading slash is optional; an empty
    /// path (or `"/"`) resolves to the root (`""`). Each segment is matched
    /// case-sensitively against existing `CollectionType` children. This is the
    /// path→id bridge for callers that think in rmapi-style paths (e.g. a device
    /// transport deploying under `/ReadingQueue`).
    ///
    /// Resolution reads from one resolved snapshot; only genuinely missing segments
    /// trigger a `mkdir`. Each real `mkdir` bumps the generation, so the tree is
    /// re-resolved before matching the next segment.
    pub async fn mkdir_p(&self, path: &str) -> Result<String> {
        let mut tree = self.resolved_snapshot().await?;
        let mut parent = String::new();
        for segment in path.split('/').filter(|s| !s.is_empty()) {
            validate_segment(segment)?;
            let existing = tree
                .children(&parent)
                .into_iter()
                .find(|e| e.is_folder && e.name == segment);
            parent = match existing {
                Some(e) => e.id,
                None => {
                    let id = self.mkdir(segment, &parent).await?;
                    // A real mkdir bumped the generation; re-resolve so later segments see it.
                    tree = self.resolved_snapshot().await?;
                    id
                }
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
mod fs_tests {
    use crate::client::Client;
    use crate::config::Config;
    use crate::fake::FakeCloud;
    use crate::porcelain::docfiles::DocFiles;
    use crate::sync_store::SyncStore;
    use crate::Metadata;

    /// A `CollectionType` (folder) doc with a fixed id, for deterministic test trees.
    fn folder_doc(id: &str, name: &str, parent: &str) -> DocFiles {
        let meta = Metadata {
            visible_name: name.into(),
            doc_type: "CollectionType".into(),
            parent: parent.into(),
            last_modified: "0".into(),
            deleted: false,
            extra: Default::default(),
        };
        DocFiles {
            id: id.into(),
            files: vec![
                (format!("{id}.metadata"), serde_json::to_vec(&meta).unwrap()),
                (format!("{id}.content"), b"{}".to_vec()),
            ],
        }
    }

    #[tokio::test]
    async fn warm_ls_costs_one_root_get() {
        let fake = FakeCloud::spawn().await;
        let dir = tempfile::tempdir().unwrap();
        let client = Client::from_user_token(Config::single_host(&fake.base), "user-token")
            .with_sync_store(SyncStore::new(dir.path().join("idx.json")));

        client.put(folder_doc("f", "Folder", "")).await.unwrap();
        let _ = client.ls("").await.unwrap(); // warm the store

        let roots_before = fake.root_get_count();
        let blobs_before = fake.blob_count_total();
        let entries = client.ls("").await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(fake.root_get_count() - roots_before, 1, "one generation poll");
        assert_eq!(fake.blob_count_total(), blobs_before, "no blob GETs when warm");
    }

    #[tokio::test]
    async fn ls_lists_children_by_parent() {
        let fake = FakeCloud::spawn().await;
        let dir = tempfile::tempdir().unwrap();
        let client = Client::from_user_token(Config::single_host(&fake.base), "user-token")
            .with_sync_store(SyncStore::new(dir.path().join("idx.json")));

        let a = client.mkdir("FolderA", "").await.unwrap();
        let b = client.mkdir("FolderB", "").await.unwrap();
        client.put(DocFiles::new_pdf("DocA", &a, b"%PDF\n".to_vec())).await.unwrap();
        client.put(DocFiles::new_pdf("DocB", &b, b"%PDF\n".to_vec())).await.unwrap();

        let roots: Vec<String> = client.ls("").await.unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(roots, vec!["FolderA", "FolderB"]);

        let in_a: Vec<String> = client.ls(&a).await.unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(in_a, vec!["DocA"]);
        let in_b: Vec<String> = client.ls(&b).await.unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(in_b, vec!["DocB"]);
    }

    #[tokio::test]
    async fn mkdir_p_creates_only_missing_segments() {
        let fake = FakeCloud::spawn().await;
        let dir = tempfile::tempdir().unwrap();
        let client = Client::from_user_token(Config::single_host(&fake.base), "user-token")
            .with_sync_store(SyncStore::new(dir.path().join("idx.json")));

        let first = client.mkdir_p("A/B/C").await.unwrap();
        // Re-resolving the same path must return the same ids, creating nothing new.
        let again = client.mkdir_p("A/B/C").await.unwrap();
        assert_eq!(first, again);

        // The tree should hold exactly A, B, C (three folders), each nested under the prior.
        let tree = client.resolved_snapshot().await.unwrap();
        assert_eq!(tree.docs.len(), 3, "no duplicate folders created");
        assert_eq!(tree.resolve_folder("A/B/C"), Some(again));
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
