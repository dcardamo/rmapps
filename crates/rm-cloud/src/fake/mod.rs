//! An in-process fake reMarkable cloud for tests. Real HTTP (axum) over an ephemeral
//! port, so clients exercise the true reqwest/serialization path. Enabled by feature
//! `fake`; public so downstream crates can test their own code against it.

mod handlers;

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use tokio::net::TcpListener;

/// Shared server state.
#[derive(Default)]
pub struct State {
    /// hash -> blob bytes.
    pub blobs: HashMap<String, Vec<u8>>,
    /// Current root hash (empty before first PUT).
    pub root_hash: String,
    /// Current generation (0 before first PUT).
    pub generation: i64,
    /// Number of upcoming root PUTs to reject with 412 (decremented each time).
    pub conflicts_remaining: u32,
    /// If true, the next root GET returns 401 then clears the flag.
    pub unauthorized_once: bool,
    /// Number of upcoming sync requests (root get/put, blob get/put) to reject with
    /// `429 Too Many Requests` + `Retry-After: 0` (decremented each time). Exercises the
    /// client's automatic 429 backoff/retry.
    pub rate_limited_remaining: u32,
    /// Per-hash count of blob GETs served (test assertion of cache effectiveness).
    pub blob_gets: HashMap<String, u32>,
    /// Count of root-ref GETs served (test assertion of generation-poll cost).
    pub root_gets: u32,
    /// Count of root PUTs received with `broadcast: true` (test assertion of notify).
    pub broadcast_commits: u32,
    /// Reads to keep stale once the next commit arms the lag (0 = unarmed).
    pub arm_lag: u32,
    /// Remaining root GETs currently serving the pre-commit index (0 = none).
    pub active_lag: u32,
    /// Root hash to serve while a lag window is active (the pre-commit index).
    pub lagged_hash: String,
}

impl State {
    /// If a rate-limit injection is pending, consume one and report it (the handler then
    /// returns 429). Returns `false` once the budget is spent.
    pub(crate) fn take_rate_limit(&mut self) -> bool {
        if self.rate_limited_remaining > 0 {
            self.rate_limited_remaining -= 1;
            true
        } else {
            false
        }
    }
}

/// A running fake cloud. Drop to stop it.
pub struct FakeCloud {
    /// Base URL, e.g. `http://127.0.0.1:54321`.
    pub base: String,
    /// Shared state for assertions and fault injection.
    pub state: Arc<Mutex<State>>,
    handle: tokio::task::JoinHandle<()>,
}

impl FakeCloud {
    /// Bind an ephemeral port and start serving.
    pub async fn spawn() -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let app = handlers::router(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            base: format!("http://{addr}"),
            state,
            handle,
        }
    }

    /// Force the next root PUT to fail with 412 (simulate a competing writer).
    pub fn inject_conflict_once(&self) {
        self.state.lock().unwrap().conflicts_remaining = 1;
    }

    /// Reject the next `n` root PUTs with 412 (to exercise the commit retry bound).
    pub fn inject_conflicts(&self, n: u32) {
        self.state.lock().unwrap().conflicts_remaining = n;
    }

    /// Force the next root GET to return 401 once (to exercise token-refresh retry).
    pub fn inject_unauthorized_once(&self) {
        self.state.lock().unwrap().unauthorized_once = true;
    }

    /// Reject the next `n` sync requests (root get/put, blob get/put) with `429 Too Many
    /// Requests` + `Retry-After: 0` (to exercise the client's 429 backoff/retry).
    pub fn inject_rate_limited(&self, n: u32) {
        self.state.lock().unwrap().rate_limited_remaining = n;
    }

    /// Arm the NEXT root PUT so that the following `reads` root GETs report the new
    /// generation but serve the PRE-commit root index — modelling reMarkable's
    /// eventual consistency (commit accepted, read replica lags). Used to reproduce
    /// the duplicate-folder race deterministically.
    pub fn lag_next_commit(&self, reads: u32) {
        self.state.lock().unwrap().arm_lag = reads;
    }

    /// Number of stored blobs (test helper).
    pub fn blob_count(&self) -> usize {
        self.state.lock().unwrap().blobs.len()
    }

    /// Read a stored blob by hash (test helper).
    pub fn blob(&self, hash: &str) -> Option<Vec<u8>> {
        self.state.lock().unwrap().blobs.get(hash).cloned()
    }

    /// Number of blob GETs served for `hash` (test helper).
    pub fn blob_get_count(&self, hash: &str) -> u32 {
        self.state.lock().unwrap().blob_gets.get(hash).copied().unwrap_or(0)
    }

    /// Total blob GETs served across all hashes (test helper).
    pub fn blob_count_total(&self) -> u32 {
        self.state.lock().unwrap().blob_gets.values().sum()
    }

    /// Number of root-ref GETs served (test helper).
    pub fn root_get_count(&self) -> u32 {
        self.state.lock().unwrap().root_gets
    }

    /// Number of commits that requested a broadcast notification (test helper).
    pub fn broadcast_count(&self) -> u32 {
        self.state.lock().unwrap().broadcast_commits
    }

    /// Spawn a new fake cloud, hydrating its state from `dir/state.json` if present.
    /// If `dir` does not exist or contains no `state.json`, starts empty.
    pub async fn from_dir(dir: &Path) -> std::io::Result<Self> {
        let state_path = dir.join("state.json");
        let on_disk: Option<StateOnDisk> = if state_path.exists() {
            let bytes = fs::read(&state_path)?;
            Some(
                serde_json::from_slice(&bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?,
            )
        } else {
            None
        };
        let cloud = Self::spawn().await;
        if let Some(on_disk) = on_disk {
            let mut s = cloud.state.lock().unwrap();
            s.root_hash = on_disk.root_hash;
            s.generation = on_disk.generation;
            s.blobs = on_disk.blobs;
        }
        Ok(cloud)
    }

    /// Atomically write current state to `dir/state.json` (creates `dir` if missing).
    pub fn dump_to_dir(&self, dir: &Path) -> std::io::Result<()> {
        fs::create_dir_all(dir)?;
        let on_disk = {
            let s = self.state.lock().unwrap();
            StateOnDisk {
                root_hash: s.root_hash.clone(),
                generation: s.generation,
                blobs: s.blobs.clone(),
            }
        };
        let bytes = serde_json::to_vec_pretty(&on_disk)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let tmp = dir.join(format!("state.json.tmp.{}", std::process::id()));
        let dest = dir.join("state.json");
        fs::write(&tmp, &bytes)?;
        fs::rename(&tmp, &dest)?;
        Ok(())
    }
}

/// On-disk projection of `State` — only the durable fields (blobs + root pointer);
/// fault-injection knobs like `conflicts_remaining` are runtime-only and not persisted.
#[derive(serde::Serialize, serde::Deserialize)]
struct StateOnDisk {
    root_hash: String,
    generation: i64,
    blobs: HashMap<String, Vec<u8>>,
}

impl Drop for FakeCloud {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

#[cfg(test)]
mod broadcast_count_tests {
    use super::*;
    use crate::client::Client;
    use crate::config::Config;
    use crate::porcelain::docfiles::DocFiles;

    #[tokio::test]
    async fn broadcast_count_tracks_only_broadcasting_commits() {
        let fake = FakeCloud::spawn().await;
        let client = Client::from_user_token(Config::single_host(&fake.base), "user-token");

        client.put(DocFiles::new_pdf("A", "", b"%PDF\n".to_vec())).await.unwrap();
        assert_eq!(fake.broadcast_count(), 0, "put must not broadcast");

        client.put_broadcast(DocFiles::new_pdf("B", "", b"%PDF\n".to_vec())).await.unwrap();
        assert_eq!(fake.broadcast_count(), 1, "put_broadcast must broadcast once");
    }
}
