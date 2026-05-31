//! Open a bundle and decide which pages changed since last sync.
use crate::state::DocState;
use rmfiles::Bundle;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub struct Ingested {
    pub bundle: Bundle,
    /// 0-based indices of pages whose .rm changed (or are new).
    pub changed: Vec<usize>,
    /// fresh page-key -> hash map to store after a successful run.
    pub new_hashes: BTreeMap<String, String>,
}

fn page_hash(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

pub fn ingest(bundle_path: &std::path::Path, prev: &DocState) -> anyhow::Result<Ingested> {
    let bundle = Bundle::open(bundle_path)?;
    let mut changed = Vec::new();
    let mut new_hashes = BTreeMap::new();
    for pg in bundle.pages() {
        let key = format!("{}.rm", pg.id);
        // A page with no .rm (never annotated) hashes the empty marker so it is
        // stable across runs.
        let bytes = pg.scene_bytes().unwrap_or_default();
        let hash = page_hash(&bytes);
        if prev
            .page_hashes
            .get(&key)
            .map(|h| h != &hash)
            .unwrap_or(true)
        {
            changed.push(pg.index);
        }
        new_hashes.insert(key, hash);
    }
    Ok(Ingested {
        bundle,
        changed,
        new_hashes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::DocState;
    use std::fs;
    use tempfile::tempdir;

    /// Build a minimal bundle directory with two pages (p1, p2) under the given
    /// uuid and return the root dir path.
    fn make_bundle(root: &std::path::Path, uuid: &str, p1_bytes: &[u8], p2_bytes: &[u8]) {
        // .content — cPages form
        let content = r#"{"cPages":{"pages":[{"id":"p1"},{"id":"p2"}]},"customZoomPageWidth":954,"customZoomPageHeight":1696}"#;
        fs::write(root.join(format!("{uuid}.content")), content).unwrap();

        // .metadata — all fields have serde(default) so a minimal object is fine
        let metadata = r#"{"visibleName":"Sample","type":"DocumentType"}"#;
        fs::write(root.join(format!("{uuid}.metadata")), metadata).unwrap();

        // page sub-directory
        let page_dir = root.join(uuid);
        fs::create_dir_all(&page_dir).unwrap();
        fs::write(page_dir.join("p1.rm"), p1_bytes).unwrap();
        fs::write(page_dir.join("p2.rm"), p2_bytes).unwrap();
    }

    #[test]
    fn first_sight_all_changed() {
        let dir = tempdir().unwrap();
        make_bundle(dir.path(), "doc", b"page-one-data", b"page-two-data");
        let prev = DocState::default();
        let ingested = ingest(dir.path(), &prev).unwrap();
        assert_eq!(ingested.changed, vec![0, 1]);
    }

    #[test]
    fn unchanged_pages_not_reported() {
        let dir = tempdir().unwrap();
        make_bundle(dir.path(), "doc", b"page-one-data", b"page-two-data");

        // Run 1 — capture hashes
        let prev = DocState::default();
        let run1 = ingest(dir.path(), &prev).unwrap();
        assert_eq!(run1.changed, vec![0, 1]);

        // Run 2 — use run1's hashes as prev state; nothing changed
        let prev2 = DocState {
            cloud_version: None,
            page_hashes: run1.new_hashes,
            digest_uuids: vec![],
        };
        let run2 = ingest(dir.path(), &prev2).unwrap();
        assert!(
            run2.changed.is_empty(),
            "expected no changes, got {:?}",
            run2.changed
        );
    }

    #[test]
    fn mutated_page_detected() {
        let dir = tempdir().unwrap();
        make_bundle(dir.path(), "doc", b"page-one-data", b"page-two-data");

        // Run 1 — get baseline hashes
        let prev = DocState::default();
        let run1 = ingest(dir.path(), &prev).unwrap();

        // Mutate p2 on disk
        fs::write(dir.path().join("doc").join("p2.rm"), b"different-bytes").unwrap();

        // Run 2 — only index 1 should be reported as changed
        let prev2 = DocState {
            cloud_version: None,
            page_hashes: run1.new_hashes,
            digest_uuids: vec![],
        };
        let run2 = ingest(dir.path(), &prev2).unwrap();
        assert_eq!(run2.changed, vec![1]);
    }
}
