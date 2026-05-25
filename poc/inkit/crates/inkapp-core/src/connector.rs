//! The connector plugin seam. A `Connector` is an `Arc`-shared plugin the
//! framework drives: it `refresh`es its own cache (network reads live here) and
//! `flush`es a durable write queue (network writes, with retry). App-facing
//! typed methods (`queue()`, `archive()`, …) live on the concrete connector and
//! stay synchronous — reads hit the warm cache, writes only enqueue.
//!
//! `ConnectorSet` lets the framework enumerate the connectors an app registered
//! so it can refresh/flush them around the sync `view`/`update` core.

use std::sync::Arc;

/// An error from a connector's network-facing work (refresh/flush transport).
/// `Clone` so it can be a `SingleFlight` result (shared across joiners).
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConnectorError {
    #[error("connector transport failed: {0}")]
    Transport(String),
    #[error("connector auth failed: {0}")]
    Auth(String),
    #[error("connector rate limited")]
    RateLimited,
}

/// A connector plugin. Shared as `Arc<dyn Connector>`; methods take `&self` and
/// the connector uses interior mutability for its cache and write queue.
#[async_trait::async_trait]
pub trait Connector: Send + Sync {
    /// Stable name (e.g. "readwise") — diagnostics and credential lookup.
    fn name(&self) -> &str;

    /// Pull fresh data into the connector's own cache. All network reads live
    /// here; the framework calls this before `view`/`update` so they read warm.
    async fn refresh(&self) -> Result<(), ConnectorError>;

    /// Drain the durable write queue, pushing each write out with retry.
    /// Persistent failures are recorded internally (the concrete connector
    /// exposes them, e.g. `failed_writes()`); surfacing is the app's job.
    async fn flush(&self);
}

/// The set of connectors an app registered, so the framework can drive them.
/// Apps implement this with a one-liner over their `Connectors` struct.
pub trait ConnectorSet {
    fn connectors(&self) -> Vec<Arc<dyn Connector>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[derive(Default)]
    struct FakeConnector {
        refreshes: AtomicU32,
        flushes: AtomicU32,
    }

    #[async_trait::async_trait]
    impl Connector for FakeConnector {
        fn name(&self) -> &str {
            "fake"
        }
        async fn refresh(&self) -> Result<(), ConnectorError> {
            self.refreshes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn flush(&self) {
            self.flushes.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct Cx {
        conn: Arc<FakeConnector>,
    }

    impl ConnectorSet for Cx {
        fn connectors(&self) -> Vec<Arc<dyn Connector>> {
            vec![self.conn.clone()]
        }
    }

    #[tokio::test]
    async fn enumerates_and_drives_connectors() {
        let conn = Arc::new(FakeConnector::default());
        let cx = Cx { conn: conn.clone() };

        let set = cx.connectors();
        assert_eq!(set.len(), 1);
        assert_eq!(set[0].name(), "fake");

        set[0].refresh().await.unwrap();
        set[0].flush().await;

        assert_eq!(conn.refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(conn.flushes.load(Ordering::SeqCst), 1);
    }
}
