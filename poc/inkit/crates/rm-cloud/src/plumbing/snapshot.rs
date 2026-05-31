//! Immutable account snapshot + tree diff.

use std::collections::BTreeMap;

use crate::error::Result;
use crate::plumbing::index::{parse_root_index, DocEntry};

/// A document reference inside a snapshot.
pub type DocRef = DocEntry;

/// An immutable view of the whole account at one generation.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Server generation of the root ref (CAS token for the next commit).
    pub generation: i64,
    /// Root index hash.
    pub root_hash: String,
    /// Documents by id (sorted), each with its doc hash.
    docs: BTreeMap<String, DocRef>,
}

/// The set difference between two snapshots, by document id.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TreeDiff {
    /// Ids present in `other` but not `self`.
    pub added: Vec<String>,
    /// Ids present in `self` but not `other`.
    pub removed: Vec<String>,
    /// Ids present in both whose doc hash differs.
    pub changed: Vec<String>,
}

impl Snapshot {
    /// Build a snapshot from a fetched root index blob.
    pub fn from_root_index(generation: i64, root_hash: String, bytes: &[u8]) -> Result<Self> {
        let docs = parse_root_index(bytes)?
            .into_iter()
            .map(|d| (d.id.clone(), d))
            .collect();
        Ok(Self {
            generation,
            root_hash,
            docs,
        })
    }

    /// An empty snapshot (account never synced).
    pub fn empty() -> Self {
        Self {
            generation: 0,
            root_hash: String::new(),
            docs: BTreeMap::new(),
        }
    }

    /// Look up a document by id.
    pub fn doc(&self, id: &str) -> Option<&DocRef> {
        self.docs.get(id)
    }

    /// All documents, id order.
    pub fn docs(&self) -> impl Iterator<Item = &DocRef> {
        self.docs.values()
    }

    /// Classify changes going from `self` to `other`.
    pub fn diff(&self, other: &Snapshot) -> TreeDiff {
        let mut d = TreeDiff::default();
        for (id, b) in &other.docs {
            match self.docs.get(id) {
                None => d.added.push(id.clone()),
                Some(a) if a.hash != b.hash => d.changed.push(id.clone()),
                Some(_) => {}
            }
        }
        for id in self.docs.keys() {
            if !other.docs.contains_key(id) {
                d.removed.push(id.clone());
            }
        }
        d.added.sort();
        d.removed.sort();
        d.changed.sort();
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plumbing::index::{serialize_root_index, DocEntry};

    fn snap(gen: i64, docs: Vec<DocEntry>) -> Snapshot {
        let bytes = serialize_root_index(&docs);
        let hash = crate::plumbing::index::sha256_hex(&bytes);
        Snapshot::from_root_index(gen, hash, &bytes).unwrap()
    }

    #[test]
    fn diff_classifies_added_removed_changed() {
        let a = snap(
            1,
            vec![
                DocEntry {
                    id: "keep".into(),
                    hash: "aa".repeat(32),
                    num_files: 1,
                    size: 1,
                },
                DocEntry {
                    id: "gone".into(),
                    hash: "bb".repeat(32),
                    num_files: 1,
                    size: 1,
                },
                DocEntry {
                    id: "edit".into(),
                    hash: "cc".repeat(32),
                    num_files: 1,
                    size: 1,
                },
            ],
        );
        let b = snap(
            2,
            vec![
                DocEntry {
                    id: "keep".into(),
                    hash: "aa".repeat(32),
                    num_files: 1,
                    size: 1,
                },
                DocEntry {
                    id: "edit".into(),
                    hash: "dd".repeat(32),
                    num_files: 1,
                    size: 1,
                },
                DocEntry {
                    id: "new".into(),
                    hash: "ee".repeat(32),
                    num_files: 1,
                    size: 1,
                },
            ],
        );
        let d = a.diff(&b);
        assert_eq!(d.added, vec!["new"]);
        assert_eq!(d.removed, vec!["gone"]);
        assert_eq!(d.changed, vec!["edit"]);
    }

    #[test]
    fn lookup_by_id() {
        let a = snap(
            1,
            vec![DocEntry {
                id: "x".into(),
                hash: "aa".repeat(32),
                num_files: 1,
                size: 9,
            }],
        );
        assert_eq!(a.doc("x").unwrap().size, 9);
        assert!(a.doc("y").is_none());
    }
}
