//! Local persistent state: per-doc page hashes + cached marks.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    /// keyed by source doc cloud path, e.g. "/Books/Purchased/kobo/Author/Title"
    pub docs: BTreeMap<String, DocState>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DocState {
    /// reMarkable cloud version/hash from `rmapi stat`; used for the cheap skip.
    pub cloud_version: Option<String>,
    /// page key ("<uuid>.rm") -> sha256 hex of its .rm bytes
    pub page_hashes: BTreeMap<String, String>,
    /// the digest docs we created (so a later run can replace them in place)
    pub digest_uuids: Vec<String>,
}

impl State {
    pub fn load(path: &Path) -> anyhow::Result<State> {
        match std::fs::read(path) {
            Ok(b) => Ok(serde_json::from_slice(&b)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
    pub fn default_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home).join(".local/state/rmdigest/state.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_missing_returns_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let state = State::load(&path).expect("load should succeed on missing file");
        assert!(state.docs.is_empty());
    }

    #[test]
    fn save_and_load_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");

        let mut state = State::default();
        let doc = DocState {
            cloud_version: Some("v42".into()),
            page_hashes: BTreeMap::from([("p1.rm".into(), "abcdef".into())]),
            digest_uuids: vec!["uuid-digest-1".into()],
        };
        state.docs.insert("/Books/SomeBook".into(), doc);

        state.save(&path).expect("save should succeed");
        let loaded = State::load(&path).expect("load should succeed");

        let loaded_doc = loaded
            .docs
            .get("/Books/SomeBook")
            .expect("doc should be present");
        assert_eq!(loaded_doc.cloud_version, Some("v42".into()));
        assert_eq!(
            loaded_doc.page_hashes.get("p1.rm").map(|s| s.as_str()),
            Some("abcdef")
        );
        assert_eq!(loaded_doc.digest_uuids, vec!["uuid-digest-1".to_string()]);
    }
}
