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

    /// Number of stored blobs (test helper).
    pub fn blob_count(&self) -> usize {
        self.state.lock().unwrap().blobs.len()
    }

    /// Read a stored blob by hash (test helper).
    pub fn blob(&self, hash: &str) -> Option<Vec<u8>> {
        self.state.lock().unwrap().blobs.get(hash).cloned()
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
