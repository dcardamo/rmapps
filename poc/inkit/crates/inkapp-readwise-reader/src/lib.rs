//! Readwise Reader connector, as an inkapp `Connector` plugin. Reads come from a
//! warm `RwLock` cache that a single-flighted `refresh` fills via a pluggable
//! `FetchTransport` (the default is a cassette; a live build injects HTTP) and
//! optionally persists to a durable `inkapp_core::cache::Cache` for warm restart.
//! Writes (move / delete / add highlight) update an optimistic overlay AND
//! enqueue a durable write that `flush` pushes through a `WriteTransport` with
//! retry. The default transports are cassette/no-op (no live account); live
//! transports are wired by `from_config()` and exercised by a manual `#[ignore]` bar.

pub mod http;

use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};

use inkapp_core::connector::{Connector, ConnectorError};
use inkapp_core::single_flight::SingleFlight;

/// After this many failed flush attempts a write is treated as permanently
/// failed and moved to `failed_writes()`.
pub const MAX_ATTEMPTS: u32 = 3;

/// Durable-cache key for the refreshed article set.
const ARTICLES_KEY: &str = "articles/v1";

/// A Readwise article id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArticleId(pub String);

impl ArticleId {
    pub fn new(s: impl Into<String>) -> Self {
        ArticleId(s.into())
    }
}

/// Where an article sits in Readwise Reader.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Location {
    #[default]
    New,
    Later,
    Shortlist,
    Archive,
    Feed,
}

impl Location {
    /// The Reader API location string.
    pub fn as_str(self) -> &'static str {
        match self {
            Location::New => "new",
            Location::Later => "later",
            Location::Shortlist => "shortlist",
            Location::Archive => "archive",
            Location::Feed => "feed",
        }
    }
}

/// An article with its full metadata. All fields beyond `id` and `title` use
/// `#[serde(default)]` so the committed cassette JSON (which only has
/// `id/title/body/highlights`) still deserialises cleanly.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Article {
    pub id: ArticleId,
    pub title: String,
    /// Plain-text body — the worked-example/highlight source until the content
    /// pipeline lands. Rich source HTML rides in `html_content`.
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub highlights: Vec<String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub source_url: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub site_name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub location: Location,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub word_count: Option<u32>,
    #[serde(default)]
    pub reading_time: Option<String>,
    #[serde(default)]
    pub published_date: Option<String>,
    #[serde(default)]
    pub saved_at: String,
    #[serde(default)]
    pub html_content: Option<String>,
}

/// A pending outbound write — the user's intent, recorded durably until pushed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Write {
    Move(ArticleId, Location),
    Delete(ArticleId),
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

/// One page of a Reader list response.
pub struct Page {
    pub articles: Vec<Article>,
    pub next_cursor: Option<String>,
}

/// The read seam: how the connector fetches a location's articles. Mirrors
/// `WriteTransport`. Default is a cassette fetch; a live build injects HTTP;
/// tests inject canned pages. (Connectors may bring their own — escape hatch.)
#[async_trait::async_trait]
pub trait FetchTransport: Send + Sync {
    async fn list(&self, location: &str, cursor: Option<&str>) -> Result<Page, ConnectorError>;
}

/// Cassette fetch: returns the committed source as a single page per location.
struct CassetteFetch {
    source: Vec<Article>,
}

#[async_trait::async_trait]
impl FetchTransport for CassetteFetch {
    async fn list(&self, location: &str, _cursor: Option<&str>) -> Result<Page, ConnectorError> {
        let articles = self
            .source
            .iter()
            .filter(|a| a.location.as_str() == location)
            .cloned()
            .collect();
        Ok(Page {
            articles,
            next_cursor: None,
        })
    }
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

/// Which locations make up the Library view, per-collection caps, and the token.
#[derive(Debug, Clone, serde::Deserialize, inkapp_config::Config)]
#[serde(default)]
#[config(kind = "readwise", namespace = "connector")]
pub struct ReaderConfig {
    /// Reader API token (name in the secret store).
    pub token: inkapp_config::SecretRef,
    /// Reader locations that make up the Library view, in order.
    #[config(default = vec![Location::New, Location::Later, Location::Shortlist])]
    pub library_locations: Vec<Location>,
    /// Maximum number of Library articles to keep.
    #[config(default = 100)]
    pub library_max: usize,
    /// Whether to include the Feed collection.
    #[config(default = true)]
    pub feed_enabled: bool,
    /// Maximum number of Feed articles to keep.
    #[config(default = 100)]
    pub feed_max: usize,
}

/// The connector. Reads come from the warm `RwLock` cache (populated by
/// `refresh` through the read seam); writes mutate the overlay (optimistic) and
/// enqueue durable writes.
pub struct Readwise {
    /// Warm (in-memory) cache read by `queue()`. Shared as `Arc` so the
    /// single-flighted refresh closure can own a handle without borrowing `self`.
    cache_articles: Arc<RwLock<Vec<Article>>>,
    overlay: Mutex<Overlay>,
    persist_path: Option<PathBuf>,
    transport: Arc<dyn WriteTransport>,
    /// The read seam. Defaults to a cassette fetch over `source`.
    fetch: Arc<dyn FetchTransport>,
    /// Optional durable cache: persists the refreshed set so a restart can serve
    /// reads before the first refresh.
    cache: Option<Arc<inkapp_core::cache::Cache>>,
    /// Reader locations to page through on refresh (in order).
    locations: Vec<String>,
    refresh_flight: SingleFlight<Result<(), ConnectorError>>,
    pub(crate) config: ReaderConfig,
}

impl Readwise {
    /// Shared constructor: pre-populate the warm cache from `source` so `queue()`
    /// works before the first explicit `refresh`, and seed the default cassette
    /// fetch over the same source.
    fn build(source: Vec<Article>, overlay: Overlay, persist_path: Option<PathBuf>) -> Self {
        Self {
            cache_articles: Arc::new(RwLock::new(source.clone())),
            fetch: Arc::new(CassetteFetch { source }),
            cache: None,
            // Single source of truth: derive the default refresh locations from
            // the default config, so `build()` and `from_config()` can't drift.
            locations: Self::locations_from(&ReaderConfig::default()),
            overlay: Mutex::new(overlay),
            persist_path,
            transport: Arc::new(NoopTransport),
            refresh_flight: SingleFlight::new(),
            config: ReaderConfig::default(),
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
                ..Article::default()
            },
            Article {
                id: ArticleId::new("a2"),
                title: "Two".into(),
                body: "ink survives the round trip".into(),
                highlights: vec![],
                ..Article::default()
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

    /// Replace the read transport (builder). Tests inject canned pages; a live
    /// build injects the Readwise-API fetch.
    #[must_use]
    pub fn with_fetch(mut self, fetch: Arc<dyn FetchTransport>) -> Self {
        self.fetch = fetch;
        self
    }

    /// Set the Reader locations to page through on refresh, in order.
    #[must_use]
    pub fn with_locations(mut self, locations: Vec<String>) -> Self {
        self.locations = locations;
        self
    }

    /// Attach a durable cache. `refresh()` persists the refreshed set to it.
    #[must_use]
    pub fn with_cache(mut self, cache: Arc<inkapp_core::cache::Cache>) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Build over an existing durable cache, hydrating the warm cache from it so
    /// reads work before the first refresh (warm restart / offline).
    pub async fn with_cache_hydrated(mut self, cache: Arc<inkapp_core::cache::Cache>) -> Self {
        if let Ok(Some(stored)) = cache.get_json::<Vec<Article>>(ARTICLES_KEY).await {
            *self.cache_articles.write().unwrap() = stored;
        }
        self.cache = Some(cache);
        self
    }

    /// The current queue: cached articles minus archived, with overlay highlights
    /// merged in. Reads the warm cache under a read lock.
    pub fn queue(&self) -> Vec<Article> {
        let ov = self.overlay.lock().unwrap();
        let cache = self.cache_articles.read().unwrap();
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

    /// Articles in the configured Library locations (overlay applied), capped.
    pub fn library(&self) -> Vec<Article> {
        let locs = &self.config.library_locations;
        let mut v: Vec<Article> = self
            .queue()
            .into_iter()
            .filter(|a| locs.contains(&a.location))
            .collect();
        v.truncate(self.config.library_max);
        v
    }

    /// Feed articles (overlay applied), capped; empty if the feed is disabled.
    pub fn feed(&self) -> Vec<Article> {
        if !self.config.feed_enabled {
            return Vec::new();
        }
        let mut v: Vec<Article> = self
            .queue()
            .into_iter()
            .filter(|a| a.location == Location::Feed)
            .collect();
        v.truncate(self.config.feed_max);
        v
    }

    /// Move an article to a new location (optimistic + enqueued).
    /// Move an article to a new location (optimistic + enqueued for `flush`).
    ///
    /// LIMITATION (current): the optimistic overlay only *hides* the article from
    /// the current views until the next `refresh`. That is correct for moves OUT
    /// of every visible view (e.g. `Archive`), but a move to a still-visible
    /// Library location (`Later`/`Shortlist`/`New`) will make the item briefly
    /// vanish and then reappear at its new location after `refresh`. No caller
    /// exercises a visible-target move yet (the reading-queue app only archives);
    /// the reader UI will, so an optimistic location-override is a planned
    /// refinement — see the `#[ignore]`d `move_to_visible_location_keeps_it_visible`
    /// spec test. Use `archive`/`delete` for the hide-from-view cases today.
    pub fn move_to(&self, id: &ArticleId, loc: Location) {
        let mut ov = self.overlay.lock().unwrap();
        if !ov.archived.contains(id) {
            ov.archived.push(id.clone()); // overlay "removed from current view"
            ov.pending.push(PendingWrite {
                write: Write::Move(id.clone(), loc),
                attempts: 0,
            });
        }
        self.save(&ov);
    }

    /// Delete an article (optimistic + enqueued).
    pub fn delete(&self, id: &ArticleId) {
        let mut ov = self.overlay.lock().unwrap();
        if !ov.archived.contains(id) {
            ov.archived.push(id.clone());
            ov.pending.push(PendingWrite {
                write: Write::Delete(id.clone()),
                attempts: 0,
            });
        }
        self.save(&ov);
    }

    /// Archive an article — delegates to `move_to(id, Location::Archive)`.
    pub fn archive(&self, id: &ArticleId) {
        self.move_to(id, Location::Archive);
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

    /// Ids currently hidden from the queue by an optimistic write — moved,
    /// deleted, or archived (the overlay field is named `archived` for serde
    /// back-compat with overlays persisted before move/delete existed).
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

    /// Flush and close the durable cache (if any). Call on shutdown so in-memory
    /// cache entries are persisted to disk.
    pub async fn close(&self) -> Result<(), ConnectorError> {
        if let Some(cache) = &self.cache {
            cache
                .close()
                .await
                .map_err(|e| ConnectorError::Transport(e.to_string()))?;
        }
        Ok(())
    }

    /// Test accessor: the refresh locations currently configured on this connector.
    #[doc(hidden)]
    pub fn locations_for_test(&self) -> Vec<String> {
        self.locations.clone()
    }

    /// Refresh locations derived from config: library locations + "feed" when enabled.
    fn locations_from(config: &ReaderConfig) -> Vec<String> {
        let mut v: Vec<String> = config
            .library_locations
            .iter()
            .map(|l| l.as_str().to_string())
            .collect();
        if config.feed_enabled {
            v.push("feed".to_string());
        }
        v
    }

    /// Assemble a live connector from typed config: token resolved from the secret
    /// store by name, durable cache under `cache_dir`, retrying HTTP transports.
    ///
    /// Returns `ConnectorError::Auth` when the configured token name is absent.
    pub async fn from_config(
        cfg: ReaderConfig,
        secrets: &inkapp_core::secrets::SecretStore,
        cache_dir: impl Into<PathBuf>,
    ) -> Result<Self, ConnectorError> {
        use inkapp_core::secrets::Scope;

        if cfg.token.is_empty() {
            return Err(ConnectorError::Auth("no readwise token configured".into()));
        }
        let raw_token = secrets
            .get(Scope::ConnectorCred, cfg.token.name())
            .map_err(|e| ConnectorError::Auth(e.to_string()))?
            .ok_or_else(|| {
                ConnectorError::Auth(format!("secret '{}' not in store", cfg.token.name()))
            })?;
        let token =
            String::from_utf8(raw_token).map_err(|e| ConnectorError::Auth(e.to_string()))?;

        let cache = Arc::new(
            inkapp_core::cache::Cache::open(cache_dir.into(), 16 << 20, 512 << 20)
                .await
                .map_err(|e| ConnectorError::Transport(e.to_string()))?,
        );

        let client = Self::retrying_http_client();
        let fetch = Arc::new(crate::http::HttpFetch::new(client.clone(), token.clone()));

        let mut me = Readwise::build(Vec::new(), Overlay::default(), None);
        me.locations = Self::locations_from(&cfg);
        me.fetch = fetch;

        // Write transport needs to look up cached articles for highlight metadata.
        let warm = Arc::clone(&me.cache_articles);
        let lookup: crate::http::ArticleLookup = Arc::new(move |id: &ArticleId| {
            warm.read().unwrap().iter().find(|a| &a.id == id).cloned()
        });
        me.transport = Arc::new(crate::http::HttpWrite::new(client, token, lookup));
        me.config = cfg;

        Ok(me.with_cache_hydrated(cache).await)
    }

    /// Build a `reqwest-middleware` client with exponential-backoff retry on transient
    /// failures (429 / 5xx). Wraps a plain `reqwest::Client` so all live transports
    /// share one connection pool. NOTE: this is pure exponential backoff (5 retries)
    /// and does not read the `Retry-After` header; if Readwise's 20/min rate limit
    /// proves bursty in practice, switch to a policy that honors `Retry-After`.
    fn retrying_http_client() -> reqwest_middleware::ClientWithMiddleware {
        use reqwest_middleware::ClientBuilder;
        use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};

        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(5);
        let inner = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("reqwest client builds (TLS backend init)");
        ClientBuilder::new(inner)
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build()
    }
}

#[async_trait::async_trait]
impl Connector for Readwise {
    fn name(&self) -> &str {
        "readwise-reader"
    }

    async fn refresh(&self) -> Result<(), ConnectorError> {
        // Page through every configured location via the read seam, dedupe by id,
        // sort newest-first, persist to the durable cache (best effort) and swap
        // into the warm cache. A live build awaits the network inside the closure,
        // outside any lock. Single-flight collapses a refresh stampede into one
        // execution. The closure is `'static` — it must not borrow `self`, so
        // overlay reconciliation happens after the flight on `&self`.
        let fetch = Arc::clone(&self.fetch);
        let locations = self.locations.clone();
        let cache = self.cache.clone();
        let warm = Arc::clone(&self.cache_articles);
        self.refresh_flight
            .run(move || async move {
                let mut all: Vec<Article> = Vec::new();
                let mut seen = std::collections::HashSet::new();
                for loc in &locations {
                    let mut cursor: Option<String> = None;
                    loop {
                        let page = fetch.list(loc, cursor.as_deref()).await?;
                        for a in page.articles {
                            if seen.insert(a.id.0.clone()) {
                                all.push(a);
                            }
                        }
                        match page.next_cursor {
                            Some(c) => cursor = Some(c),
                            None => break,
                        }
                    }
                }
                all.sort_by(|a, b| b.saved_at.cmp(&a.saved_at)); // newest first
                if let Some(cache) = &cache {
                    let _ = cache.put_json(ARTICLES_KEY, &all).await; // best-effort
                }
                // Brief write lock, no await held across it (the doc's rule). On a
                // fetch error the `?` above returns Err before this, so the prior
                // warm cache is preserved.
                *warm.write().unwrap() = all;
                Ok(())
            })
            .await?;

        // Reconcile the optimistic overlay against new server truth: keep a
        // "hidden" id only while it's still present server-side (move/delete not
        // yet applied); drop added highlights the server now reflects.
        //
        // Load-bearing assumption: an applied move/archive/delete makes the item
        // absent from this refresh set — moves/archives land in `archive`, which
        // is NOT among the configured `locations`, and deletes remove the item
        // entirely. If `archive` were ever added to `locations`, an archived item
        // would still appear here and its overlay entry would never be pruned;
        // the reconciliation would then need to compare locations, not presence.
        {
            let warm = self.cache_articles.read().unwrap();
            let present: std::collections::HashSet<String> =
                warm.iter().map(|a| a.id.0.clone()).collect();
            let mut ov = self.overlay.lock().unwrap();
            ov.archived.retain(|id| present.contains(&id.0));
            ov.added.retain(|(id, text)| {
                warm.iter()
                    .find(|a| &a.id == id)
                    .is_none_or(|a| !a.highlights.contains(text))
            });
            self.save(&ov);
        }
        Ok(())
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
