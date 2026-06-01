//! Persistent daemon state: diff baseline + scheduler last-run/last-attempt + retry counters.
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
    /// Scheduler: task key -> last attempt (unix secs), recorded regardless of outcome.
    /// Used to pace `every` tasks so a failing task waits its full interval before retrying
    /// (avoids a hot busy-loop), while `last_run` remains the "last success" record.
    #[serde(default)]
    pub last_attempt: BTreeMap<String, u64>,
    /// Reactive-action retry counters: doc id -> consecutive failed attempts. Retry itself is
    /// driven by removing the doc from `baseline` so the next reconcile re-detects it; this
    /// just bounds the number of retries.
    #[serde(default)]
    pub failed_attempts: BTreeMap<String, u32>,
}

pub fn state_path() -> PathBuf {
    // Test/override hook (mirrors rmdigest's RMDIGEST_STATE): point the daemon at an
    // explicit state file. Used by the reactor integration test to stay hermetic so it
    // never clobbers the real ~/.local/state/rmapps/watch-state.json on a host running
    // the watch daemon.
    if let Ok(p) = std::env::var("RMAPPS_WATCH_STATE") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
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
        state.last_attempt.insert("digest#0".to_string(), 1_700_000_500);
        state.failed_attempts.insert("doc-c".to_string(), 2);

        let json = serde_json::to_string_pretty(&state).unwrap();
        let back: WatchState = serde_json::from_str(&json).unwrap();

        assert_eq!(back.baseline, state.baseline);
        assert_eq!(back.baseline_generation, Some(42));
        assert_eq!(back.last_run.get("digest#0"), Some(&1_700_000_000));
        assert_eq!(back.last_attempt.get("digest#0"), Some(&1_700_000_500));
        assert_eq!(back.failed_attempts.get("doc-c"), Some(&2));
    }

    #[test]
    fn corrupt_json_yields_default() {
        let back: WatchState =
            serde_json::from_str("{ not valid json ").unwrap_or_default();
        assert!(back.baseline.is_empty());
        assert!(back.baseline_generation.is_none());
        assert!(back.failed_attempts.is_empty());
    }

    #[test]
    fn state_path_honors_env_override() {
        // Single test that sets + restores the env var to avoid cross-test interference
        // (these run in the same process). An empty value is ignored (falls back to default).
        let prev = std::env::var("RMAPPS_WATCH_STATE").ok();

        std::env::set_var("RMAPPS_WATCH_STATE", "/tmp/rmapps-test-override.json");
        assert_eq!(state_path(), PathBuf::from("/tmp/rmapps-test-override.json"));

        // Empty => ignored, so the default path is used (ends with the well-known filename).
        std::env::set_var("RMAPPS_WATCH_STATE", "");
        assert!(state_path().ends_with("rmapps/watch-state.json"));

        match prev {
            Some(v) => std::env::set_var("RMAPPS_WATCH_STATE", v),
            None => std::env::remove_var("RMAPPS_WATCH_STATE"),
        }
    }

    #[test]
    fn old_state_without_new_fields_loads() {
        // Old state files predate last_attempt/failed_attempts; serde default fills them.
        let back: WatchState = serde_json::from_str(
            r#"{"baseline":{"d":"h"},"baseline_generation":1,"last_run":{"k":5}}"#,
        )
        .unwrap();
        assert_eq!(back.baseline.get("d"), Some(&"h".to_string()));
        assert!(back.last_attempt.is_empty());
        assert!(back.failed_attempts.is_empty());
    }
}
