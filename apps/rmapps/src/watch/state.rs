//! Persistent daemon state: diff baseline + scheduler last-run + failed-job retries.
//! Atomic save (temp+rename) and tolerant load (fresh on corrupt), mirroring sync.rs.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct WatchState {
    /// Compact diff baseline: doc id -> doc hash at last reconcile.
    #[serde(default)]
    pub baseline: BTreeMap<String, String>,
    /// Root generation matching `baseline` (None before first reconcile).
    #[serde(default)]
    pub baseline_generation: Option<i64>,
    /// Scheduler: task key -> last successful run (unix secs).
    #[serde(default)]
    pub last_run: BTreeMap<String, u64>,
    /// Failed reactive jobs awaiting retry.
    #[serde(default)]
    pub pending_jobs: Vec<PendingJob>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingJob {
    pub rule_path: String,
    pub doc_id: String,
    pub new_hash: String,
    pub attempts: u32,
}

pub const MAX_ATTEMPTS: u32 = 5;

pub fn state_path() -> PathBuf {
    let base = dirs::state_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local").join("state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"));
    base.join("rmapps").join("watch-state.json")
}

pub fn load() -> WatchState {
    let path = state_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            println!("[rmapps] watch: ignoring corrupt state {}: {e}", path.display());
            WatchState::default()
        }),
        Err(_) => WatchState::default(),
    }
}

pub fn save(state: &WatchState) -> Result<()> {
    let path = state_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(state).context("serializing watch state")?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("renaming -> {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_state_serde_round_trip() {
        let mut state = WatchState::default();
        state.baseline.insert("doc-a".to_string(), "hash-a".to_string());
        state.baseline.insert("doc-b".to_string(), "hash-b".to_string());
        state.baseline_generation = Some(42);
        state.last_run.insert("digest#0".to_string(), 1_700_000_000);
        state.pending_jobs.push(PendingJob {
            rule_path: "/Books".to_string(),
            doc_id: "doc-c".to_string(),
            new_hash: "hash-c".to_string(),
            attempts: 2,
        });

        let json = serde_json::to_string_pretty(&state).unwrap();
        let back: WatchState = serde_json::from_str(&json).unwrap();

        assert_eq!(back.baseline, state.baseline);
        assert_eq!(back.baseline_generation, Some(42));
        assert_eq!(back.last_run.get("digest#0"), Some(&1_700_000_000));
        assert_eq!(back.pending_jobs.len(), 1);
        assert_eq!(back.pending_jobs[0].doc_id, "doc-c");
        assert_eq!(back.pending_jobs[0].attempts, 2);
    }

    #[test]
    fn corrupt_json_yields_default() {
        let back: WatchState =
            serde_json::from_str("{ not valid json ").unwrap_or_default();
        assert!(back.baseline.is_empty());
        assert!(back.baseline_generation.is_none());
        assert!(back.pending_jobs.is_empty());
    }
}
