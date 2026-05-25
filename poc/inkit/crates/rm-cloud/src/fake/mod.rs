//! An in-process fake reMarkable cloud for tests. Real HTTP (axum) over an ephemeral
//! port, so clients exercise the true reqwest/serialization path. Enabled by feature
//! `fake`; public so downstream crates can test their own code against it.

mod handlers;

use std::collections::HashMap;
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
}

impl Drop for FakeCloud {
    fn drop(&mut self) {
        self.handle.abort();
    }
}
