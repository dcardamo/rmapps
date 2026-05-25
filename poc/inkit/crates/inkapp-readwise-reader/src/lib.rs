//! Cassette-backed Readwise connector, as an inkapp `Connector` plugin. Reads
//! come from an `RwLock` cache (populated from the committed cassette by a
//! single-flighted `refresh`); writes (archive / add highlight) update an
//! optimistic overlay AND enqueue a durable write that `flush` pushes through a
//! `WriteTransport` with retry. The default transport is a no-op (cassette mode,
//! no live account); the live transport is a manual `#[ignore]` bar.

use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};

use inkapp_core::connector::{Connector, ConnectorError};
use inkapp_core::single_flight::SingleFlight;

/// After this many failed flush attempts a write is treated as permanently
/// failed and moved to `failed_writes()`.
pub const MAX_ATTEMPTS: u32 = 3;

/// A Readwise article id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArticleId(pub String);

impl ArticleId {
    pub fn new(s: impl Into<String>) -> Self {
        ArticleId(s.into())
    }
}

/// An article: its id, title, body text, and highlighted spans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Article {
    pub id: ArticleId,
    pub title: String,
    pub body: String,
    pub highlights: Vec<String>,
}

/// A pending outbound write — the user's intent, recorded durably until pushed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Write {
    Archive(ArticleId),
    Highlight(ArticleId, String),
}

/// A queued write plus how many flush attempts it has survived.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingWrite {
    write: Write,
    attempts: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct Cassette {
    articles: Vec<Article>,
}

#[derive(Default, Serialize, Deserialize)]
struct Overlay {
    archived: Vec<ArticleId>,
    added: Vec<(ArticleId, String)>,
    /// Outbound writes not yet delivered. `#[serde(default)]` so overlay files
    /// written before this field existed still load.
    #[serde(default)]
    pending: Vec<PendingWrite>,
    /// Writes that exhausted their retries.
    #[serde(default)]
    failed: Vec<Write>,
}

/// How a write reaches the remote. The default is a no-op (cassette mode); tests
/// inject a scripted transport; a live build pushes to the Readwise API.
#[async_trait::async_trait]
pub trait WriteTransport: Send + Sync {
    async fn push(&self, write: &Write) -> Result<(), ConnectorError>;
}

/// Cassette-mode transport: every write "succeeds" against nothing.
struct NoopTransport;

#[async_trait::async_trait]
impl WriteTransport for NoopTransport {
    async fn push(&self, _write: &Write) -> Result<(), ConnectorError> {
        Ok(())
    }
}

/// A deterministic transport for tests. `remaining` > 0 fails that many pushes
/// then succeeds; `remaining` < 0 fails forever; counts successful deliveries.
pub struct ScriptedTransport {
    remaining: AtomicI64,
    delivered: AtomicU32,
}

impl ScriptedTransport {
    /// Fail the first `n` pushes, then succeed.
    pub fn failing(n: u32) -> Self {
        Self {
            remaining: AtomicI64::new(n as i64),
            delivered: AtomicU32::new(0),
        }
    }

    /// Never succeed.
    pub fn always_failing() -> Self {
        Self {
            remaining: AtomicI64::new(-1),
            delivered: AtomicU32::new(0),
        }
    }

    /// How many pushes have succeeded.
    pub fn delivered(&self) -> u32 {
        self.delivered.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl WriteTransport for ScriptedTransport {
    async fn push(&self, _write: &Write) -> Result<(), ConnectorError> {
        let r = self.remaining.load(Ordering::SeqCst);
        if r == 0 {
            self.delivered.fetch_add(1, Ordering::SeqCst);
            return Ok(());
        }
        if r > 0 {
            self.remaining.fetch_sub(1, Ordering::SeqCst);
        }
        Err(ConnectorError::Transport("scripted failure".into()))
    }
}

/// The connector. Reads come from the `RwLock` cache (populated from `source` by
/// `refresh`); writes mutate the overlay (optimistic) and enqueue durable writes.
pub struct Readwise {
    /// Immutable fetch source (the committed cassette). A live build would fetch
    /// from the network instead.
    source: Vec<Article>,
    /// Warm cache read by `queue()`. Shared as `Arc` so the single-flighted
    /// refresh closure can own a handle without borrowing `self`.
    cache: Arc<RwLock<Vec<Article>>>,
    overlay: Mutex<Overlay>,
    persist_path: Option<PathBuf>,
    transport: Arc<dyn WriteTransport>,
    refresh_flight: SingleFlight<Result<(), ConnectorError>>,
}

impl Readwise {
    /// Shared constructor: pre-populate the cache from `source` so `queue()`
    /// works before the first explicit `refresh`.
    fn build(source: Vec<Article>, overlay: Overlay, persist_path: Option<PathBuf>) -> Self {
        Self {
            cache: Arc::new(RwLock::new(source.clone())),
            source,
            overlay: Mutex::new(overlay),
            persist_path,
            transport: Arc::new(NoopTransport),
            refresh_flight: SingleFlight::new(),
        }
    }

    /// Load from the committed cassette JSON.
    pub fn from_cassette() -> Self {
        let raw = include_str!("../fixtures/cassette/articles.json");
        let c: Cassette = serde_json::from_str(raw).expect("valid committed cassette");
        Self::build(c.articles, Overlay::default(), None)
    }

    /// A tiny inline cassette for unit tests (no committed file dependency).
    pub fn fake() -> Self {
        let articles = vec![
            Article {
                id: ArticleId::new("a1"),
                title: "One".into(),
                body: "the slow web rewards patience".into(),
                highlights: vec![],
            },
            Article {
                id: ArticleId::new("a2"),
                title: "Two".into(),
                body: "ink survives the round trip".into(),
                highlights: vec![],
            },
        ];
        Self::build(articles, Overlay::default(), None)
    }

    /// Like `from_cassette`, but the overlay is loaded from `path` (if present)
    /// and saved on every write — so manual on-device use survives restarts.
    pub fn persisted(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let raw = include_str!("../fixtures/cassette/articles.json");
        let source: Vec<Article> = serde_json::from_str::<Cassette>(raw)
            .expect("valid committed cassette")
            .articles;
        let overlay = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self::build(source, overlay, Some(path))
    }

    /// Replace the write transport (builder). Tests inject a `ScriptedTransport`;
    /// a live build injects the Readwise-API transport.
    #[must_use]
    pub fn with_transport(mut self, transport: Arc<dyn WriteTransport>) -> Self {
        self.transport = transport;
        self
    }

    /// The current queue: cached articles minus archived, with overlay highlights
    /// merged in. Reads the warm cache under a read lock.
    pub fn queue(&self) -> Vec<Article> {
        let ov = self.overlay.lock().unwrap();
        let cache = self.cache.read().unwrap();
        cache
            .iter()
            .filter(|a| !ov.archived.contains(&a.id))
            .map(|a| {
                let mut a = a.clone();
                for (id, text) in &ov.added {
                    if id == &a.id && !a.highlights.contains(text) {
                        a.highlights.push(text.clone());
                    }
                }
                a
            })
            .collect()
    }

    /// Persist the overlay to `persist_path` if set (no-op for in-memory connectors).
    fn save(&self, overlay: &Overlay) {
        if let Some(path) = &self.persist_path {
            if let Ok(json) = serde_json::to_string_pretty(overlay) {
                let _ = std::fs::write(path, json);
            }
        }
    }

    /// Record an archive: optimistic (leaves the queue now) and enqueued for push.
    pub fn archive(&self, id: &ArticleId) {
        let mut ov = self.overlay.lock().unwrap();
        if !ov.archived.contains(id) {
            ov.archived.push(id.clone());
            ov.pending.push(PendingWrite {
                write: Write::Archive(id.clone()),
                attempts: 0,
            });
        }
        self.save(&ov);
    }

    /// Record a highlight (idempotent on (id, text)); enqueued for push.
    pub fn add_highlight(&self, id: &ArticleId, text: &str) {
        let mut ov = self.overlay.lock().unwrap();
        if !ov.added.iter().any(|(i, t)| i == id && t == text) {
            ov.added.push((id.clone(), text.to_string()));
            ov.pending.push(PendingWrite {
                write: Write::Highlight(id.clone(), text.to_string()),
                attempts: 0,
            });
        }
        self.save(&ov);
    }

    /// The archived ids (for assertions / surfacing).
    pub fn archived(&self) -> Vec<ArticleId> {
        self.overlay.lock().unwrap().archived.clone()
    }

    /// The recorded highlight texts for one article.
    pub fn highlights(&self, id: &ArticleId) -> Vec<String> {
        self.overlay
            .lock()
            .unwrap()
            .added
            .iter()
            .filter(|(i, _)| i == id)
            .map(|(_, t)| t.clone())
            .collect()
    }

    /// Writes that exhausted their retries — the app's `view` reads this to
    /// render a "couldn't sync" banner.
    pub fn failed_writes(&self) -> Vec<Write> {
        self.overlay.lock().unwrap().failed.clone()
    }
}

#[async_trait::async_trait]
impl Connector for Readwise {
    fn name(&self) -> &str {
        "readwise-reader"
    }

    async fn refresh(&self) -> Result<(), ConnectorError> {
        // Cassette mode: the "fetch" is the committed data. A live build would
        // await the network inside this closure, outside any lock. Single-flight
        // collapses a refresh stampede into one execution.
        let source = self.source.clone();
        let cache = Arc::clone(&self.cache);
        self.refresh_flight
            .run(move || async move {
                // Brief write lock, no await held across it (the doc's rule).
                *cache.write().unwrap() = source;
                Ok(())
            })
            .await
    }

    async fn flush(&self) {
        // Take the queue out under the lock, then release it before any await.
        let pending = {
            let mut ov = self.overlay.lock().unwrap();
            std::mem::take(&mut ov.pending)
        };

        let mut still_pending = Vec::new();
        let mut newly_failed = Vec::new();
        for mut p in pending {
            match self.transport.push(&p.write).await {
                Ok(()) => {} // delivered — drop it
                Err(_) => {
                    p.attempts += 1;
                    if p.attempts >= MAX_ATTEMPTS {
                        newly_failed.push(p.write);
                    } else {
                        still_pending.push(p);
                    }
                }
            }
        }

        let mut ov = self.overlay.lock().unwrap();
        ov.pending.extend(still_pending);
        ov.failed.extend(newly_failed);
        self.save(&ov);
    }
}
