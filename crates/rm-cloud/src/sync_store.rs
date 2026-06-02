//! Persistent local sync state: the durable `{generation, docId → (hash, parent, name)}`
//! index that mirrors what the tablet keeps, so listing/path resolution need not re-read
//! every doc's metadata. Fully reconstructible from the cloud, so a missing or corrupt
//! file is not an error — it yields an empty store and forces a cold rebuild.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::porcelain::fs::Entry;

/// Current on-disk schema version. Bump on any breaking shape change; an older/newer
/// value loads as empty (forcing a cold rebuild) rather than erroring.
const SCHEMA_VERSION: u32 = 1;

/// One resolved document: its content hash plus the path facts that live in `.metadata`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedDoc {
    /// Cloud doc hash (Merkle) — the change-detection key.
    pub hash: String,
    /// Parent folder id ("" = root, "trash" = trash).
    pub parent: String,
    /// Visible name.
    pub name: String,
    /// True if this doc is a folder (`CollectionType`).
    pub is_folder: bool,
}

/// An account view resolved to ids + paths at a single generation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedTree {
    /// Account generation this view was built at.
    pub generation: i64,
    /// docId → resolved doc.
    pub docs: BTreeMap<String, ResolvedDoc>,
}

impl ResolvedTree {
    /// Direct children of `parent`, as listing `Entry`s, sorted by name. Trash/deleted
    /// exclusion is the tree builder's job — this filters purely on `parent`.
    pub fn children(&self, parent: &str) -> Vec<Entry> {
        let mut out: Vec<Entry> = self
            .docs
            .iter()
            .filter(|(_, d)| d.parent == parent)
            .map(|(id, d)| Entry {
                id: id.clone(),
                name: d.name.clone(),
                parent: d.parent.clone(),
                is_folder: d.is_folder,
                hash: d.hash.clone(),
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Resolve a slash path ("" / "/" = root) to a folder id, matching `CollectionType`
    /// children case-sensitively. `None` if any segment is missing.
    pub fn resolve_folder(&self, path: &str) -> Option<String> {
        let mut parent = String::new();
        for seg in path.split('/').filter(|s| !s.is_empty()) {
            let next = self
                .children(&parent)
                .into_iter()
                .find(|e| e.is_folder && e.name == seg)?;
            parent = next.id;
        }
        Some(parent)
    }
}

/// On-disk envelope (carries the schema tag alongside the tree).
#[derive(Serialize, Deserialize)]
struct StoredIndex {
    schema_version: u32,
    tree: ResolvedTree,
}

/// A persistent local sync index at a fixed path.
pub struct SyncStore {
    path: PathBuf,
    tree: RwLock<ResolvedTree>,
}

impl SyncStore {
    /// Open (or initialize empty) the index at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let tree = Self::load(&path).unwrap_or_default();
        Self {
            path,
            tree: RwLock::new(tree),
        }
    }

    /// Tolerant load: `None` on any missing-file / parse / schema-mismatch condition.
    fn load(path: &std::path::Path) -> Option<ResolvedTree> {
        let bytes = std::fs::read(path).ok()?;
        let stored: StoredIndex = serde_json::from_slice(&bytes).ok()?;
        if stored.schema_version != SCHEMA_VERSION {
            return None;
        }
        Some(stored.tree)
    }

    /// A clone of the current in-memory tree.
    pub fn tree(&self) -> ResolvedTree {
        self.tree.read().expect("sync store lock poisoned").clone()
    }

    /// Replace the index with `tree`, persisting atomically. The temp+rename guarantees
    /// the previous file stays intact until a complete new file is in place; a persist
    /// failure is best-effort and must not corrupt the live file.
    pub fn store(&self, tree: &ResolvedTree) {
        *self.tree.write().expect("sync store lock poisoned") = tree.clone();
        let _ = self.persist(tree);
    }

    fn persist(&self, tree: &ResolvedTree) -> std::io::Result<()> {
        let stored = StoredIndex {
            schema_version: SCHEMA_VERSION,
            tree: tree.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&stored)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self
            .path
            .with_extension(format!("tmp.{}", std::process::id()));
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(hash: &str, parent: &str, name: &str, is_folder: bool) -> ResolvedDoc {
        ResolvedDoc {
            hash: hash.into(),
            parent: parent.into(),
            name: name.into(),
            is_folder,
        }
    }

    fn sample() -> ResolvedTree {
        let mut docs = BTreeMap::new();
        docs.insert("rw".into(), doc("h1", "", "Readwise", true));
        docs.insert("feed".into(), doc("h2", "rw", "Feed", false));
        docs.insert("lib".into(), doc("h3", "rw", "Library", false));
        ResolvedTree {
            generation: 7,
            docs,
        }
    }

    #[test]
    fn children_filters_and_sorts() {
        let t = sample();
        let kids: Vec<String> = t.children("rw").into_iter().map(|e| e.name).collect();
        assert_eq!(kids, vec!["Feed", "Library"]);
        assert_eq!(t.children("").len(), 1); // just the Readwise folder
    }

    #[test]
    fn resolve_folder_walks_segments() {
        let t = sample();
        assert_eq!(t.resolve_folder("/Readwise"), Some("rw".into()));
        assert_eq!(t.resolve_folder(""), Some("".into()));
        assert_eq!(t.resolve_folder("/Readwise/Nope"), None);
    }

    #[test]
    fn store_then_reopen_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync-index.json");
        let store = SyncStore::new(&path);
        store.store(&sample());
        let reopened = SyncStore::new(&path);
        assert_eq!(reopened.tree(), sample());
    }

    #[test]
    fn missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = SyncStore::new(dir.path().join("absent.json"));
        assert_eq!(store.tree(), ResolvedTree::default());
    }

    #[test]
    fn corrupt_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync-index.json");
        std::fs::write(&path, b"not json at all").unwrap();
        let store = SyncStore::new(&path);
        assert_eq!(store.tree(), ResolvedTree::default());
    }

    #[test]
    fn wrong_schema_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync-index.json");
        std::fs::write(&path, br#"{"schema_version":999,"tree":{"generation":1,"docs":{}}}"#)
            .unwrap();
        let store = SyncStore::new(&path);
        assert_eq!(store.tree(), ResolvedTree::default());
    }
}
