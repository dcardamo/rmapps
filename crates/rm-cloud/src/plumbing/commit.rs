//! Atomic commit: turn a desired mutation into uploaded blobs + a CAS root-put,
//! retrying with a rebase when the generation is stale.

use crate::plumbing::index::{
    doc_hash, doc_size, serialize_doc_index, sha256_hex, DocEntry, FileEntry,
};

/// A document to upsert: its full set of files with raw bytes.
pub struct DocUpsert {
    /// Document id (UUID).
    pub id: String,
    /// Files: (logical id, bytes). The doc/file hashes are computed here.
    pub files: Vec<(String, Vec<u8>)>,
}

/// A desired change to the account tree.
#[derive(Default)]
pub struct Mutation {
    /// Documents to create or replace wholesale.
    pub upserts: Vec<DocUpsert>,
    /// Document ids to remove.
    pub removals: Vec<String>,
}

/// Blobs to upload for one upserted doc, plus the resulting root [`DocEntry`].
pub(crate) struct PreparedDoc {
    pub doc_entry: DocEntry,
    /// (hash, logical name, bytes) for each file blob + the doc-index blob.
    pub blobs: Vec<(String, String, Vec<u8>)>,
}

/// Compute hashes + serialized blobs for one upsert (pure; no IO).
pub(crate) fn prepare_doc(up: &DocUpsert) -> PreparedDoc {
    let file_entries: Vec<FileEntry> = up
        .files
        .iter()
        .map(|(id, bytes)| FileEntry {
            id: id.clone(),
            hash: sha256_hex(bytes),
            size: bytes.len() as u64,
        })
        .collect();

    let dhash = doc_hash(&file_entries);
    let size = doc_size(&file_entries);
    let index_bytes = serialize_doc_index(&file_entries);

    let mut blobs: Vec<(String, String, Vec<u8>)> = up
        .files
        .iter()
        .zip(file_entries.iter())
        .map(|((id, bytes), fe)| (fe.hash.clone(), id.clone(), bytes.clone()))
        .collect();
    // The doc-index blob is keyed by the doc hash, named "<id>.docSchema".
    blobs.push((dhash.clone(), format!("{}.docSchema", up.id), index_bytes));

    PreparedDoc {
        doc_entry: DocEntry {
            id: up.id.clone(),
            hash: dhash,
            num_files: file_entries.len() as u32,
            size,
        },
        blobs,
    }
}

/// Apply a mutation to the current doc set, returning the new root [`DocEntry`] list.
pub(crate) fn apply(
    current: &[DocEntry],
    mutation: &Mutation,
    prepared: &[PreparedDoc],
) -> Vec<DocEntry> {
    let mut by_id: std::collections::BTreeMap<String, DocEntry> =
        current.iter().map(|d| (d.id.clone(), d.clone())).collect();
    for id in &mutation.removals {
        by_id.remove(id);
    }
    for p in prepared {
        by_id.insert(p.doc_entry.id.clone(), p.doc_entry.clone());
    }
    by_id.into_values().collect()
}

/// Build the (root_hash, root_index_bytes) for a doc-entry list.
pub(crate) fn root_blob(docs: &[DocEntry]) -> (String, Vec<u8>) {
    let bytes = crate::plumbing::index::serialize_root_index(docs);
    (sha256_hex(&bytes), bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_upserts_and_removals() {
        let current = vec![
            DocEntry {
                id: "a".into(),
                hash: "11".repeat(32),
                num_files: 1,
                size: 1,
            },
            DocEntry {
                id: "b".into(),
                hash: "22".repeat(32),
                num_files: 1,
                size: 1,
            },
        ];
        let up = DocUpsert {
            id: "c".into(),
            files: vec![("c.metadata".into(), b"{}".to_vec())],
        };
        let prepared = vec![prepare_doc(&up)];
        let mutation = Mutation {
            upserts: vec![up],
            removals: vec!["a".into()],
        };
        let result = apply(&current, &mutation, &prepared);
        let ids: Vec<&str> = result.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "c"]); // a removed, c added, sorted by id
    }

    #[test]
    fn prepare_doc_keys_index_by_doc_hash() {
        let up = DocUpsert {
            id: "x".into(),
            files: vec![("x.metadata".into(), b"hi".to_vec())],
        };
        let p = prepare_doc(&up);
        // last blob is the doc index, keyed by the doc hash, named x.docSchema
        let (hash, name, _) = p.blobs.last().unwrap();
        assert_eq!(name, "x.docSchema");
        assert_eq!(*hash, p.doc_entry.hash);
    }
}
