//! The cloud client: holds credentials + config, refreshes the user token on demand,
//! and exposes snapshot/porcelain/sync entry points.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::auth::{refresh_user_token, Credentials};
use crate::cache::BlobCache;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::plumbing::blob::{get_blob, put_blob};
use crate::plumbing::commit::{apply, prepare_doc, root_blob, Mutation};
use crate::plumbing::index::serialize_root_index;
use crate::plumbing::snapshot::Snapshot;

/// A client for one reMarkable cloud account. Construct multiple clients for multiple
/// accounts — there is no shared global state.
#[derive(Clone)]
pub struct Client {
    pub(crate) http: reqwest::Client,
    pub(crate) config: Config,
    pub(crate) creds: Arc<RwLock<Credentials>>,
    pub(crate) cache: Option<Arc<BlobCache>>,
    /// Persistent local sync index: listing/path resolution route through it, so unchanged
    /// docs cost no metadata fetch. `None` => every resolve rebuilds from the cloud.
    pub(crate) sync_store: Option<Arc<crate::sync_store::SyncStore>>,
    /// Generation-keyed snapshot memo: avoids re-downloading + re-parsing the root index
    /// when the account's generation hasn't changed since the last call.
    pub(crate) snap_cache: Arc<RwLock<Option<Snapshot>>>,
}

#[derive(Deserialize)]
struct RootResp {
    hash: String,
    generation: i64,
}

#[derive(Serialize)]
struct RootPutReq<'a> {
    broadcast: bool,
    hash: &'a str,
    generation: i64,
}

#[derive(Deserialize)]
struct RootPutResp {
    generation: i64,
}

impl Client {
    /// Build a client from an explicit device token (user token refreshed lazily).
    pub fn from_device_token(config: Config, device_token: impl Into<String>) -> Self {
        Self::new(config, Credentials::from_device_token(device_token))
    }

    /// Build a client from an explicit user token only.
    ///
    /// **Cannot auto-refresh:** user tokens are short-lived, and without a device token
    /// this client cannot mint a new one — once the token expires, calls fail with
    /// [`Error::MissingCredential`]. Use this for tests or ephemeral, already-valid
    /// tokens; prefer [`from_device_token`](Self::from_device_token) for anything
    /// long-running.
    pub fn from_user_token(config: Config, user_token: impl Into<String>) -> Self {
        Self::new(
            config,
            Credentials {
                device_token: None,
                user_token: Some(user_token.into()),
            },
        )
    }

    /// Build a client from `RM_CLOUD_*` env vars and `Config::from_env()`.
    pub fn from_env() -> Result<Self> {
        let creds = Credentials::from_env();
        if creds.device_token.is_none() && creds.user_token.is_none() {
            return Err(Error::MissingCredential(
                "RM_CLOUD_DEVICE_TOKEN or RM_CLOUD_USER_TOKEN",
            ));
        }
        Ok(Self::new(Config::from_env(), creds))
    }

    fn new(config: Config, creds: Credentials) -> Self {
        Self {
            http: reqwest::Client::new(),
            config,
            creds: Arc::new(RwLock::new(creds)),
            cache: None,
            sync_store: None,
            snap_cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Attach a content-addressed disk cache. All blob reads/writes route through it.
    pub fn with_cache(mut self, cache: BlobCache) -> Self {
        self.cache = Some(Arc::new(cache));
        self
    }

    /// Attach a persistent local sync index. Listing/path resolution route through it,
    /// so unchanged docs cost no metadata fetch.
    pub fn with_sync_store(mut self, store: crate::sync_store::SyncStore) -> Self {
        self.sync_store = Some(Arc::new(store));
        self
    }

    /// Current user token, refreshing from the device token if absent.
    async fn user_token(&self) -> Result<String> {
        if let Some(t) = self.creds.read().await.user_token.clone() {
            return Ok(t);
        }
        self.force_refresh().await
    }

    /// Refresh the user token from the device token and store it.
    pub(crate) async fn force_refresh(&self) -> Result<String> {
        let device = self
            .creds
            .read()
            .await
            .device_token
            .clone()
            .ok_or(Error::MissingCredential("device_token"))?;
        let token = refresh_user_token(&self.http, &self.config, &device).await?;
        self.creds.write().await.user_token = Some(token.clone());
        Ok(token)
    }

    /// Fetch the current account snapshot, reusing a cached one when the account's
    /// generation is unchanged (cheap root-ref poll), else rebuilding it.
    pub async fn snapshot(&self) -> Result<Snapshot> {
        let current_gen = self.current_generation().await?;

        // Reuse the cached snapshot when the generation matches.
        if let Some(gen) = current_gen {
            if let Some(snap) = self.snap_cache.read().await.as_ref() {
                if snap.generation == gen {
                    return Ok(snap.clone());
                }
            }
        }

        // Rebuild from the root ref. `None` => account never synced.
        let root = match self.get_root_ref().await {
            Err(Error::Unauthorized) => {
                self.force_refresh().await?;
                self.get_root_ref().await?
            }
            other => other?,
        };
        let Some(root) = root else {
            return Ok(Snapshot::empty());
        };
        let bytes = self.get_blob(&root.hash, "root.docSchema").await?;
        let snap = Snapshot::from_root_index(root.generation, root.hash, &bytes)?;
        *self.snap_cache.write().await = Some(snap.clone());
        Ok(snap)
    }

    /// Resolve the account to ids + paths, reusing the persistent sync index. Polls the
    /// root generation (one request); if the store is current, returns it verbatim with no
    /// metadata fetches. Otherwise diffs the root index by doc hash and fetches `.metadata`
    /// only for added/changed docs (skipping deleted docs), then persists the updated index.
    pub async fn resolved_snapshot(&self) -> Result<crate::sync_store::ResolvedTree> {
        use crate::sync_store::{ResolvedDoc, ResolvedTree};

        let Some(gen) = self.current_generation().await? else {
            return Ok(ResolvedTree::default());
        };

        let prev = self
            .sync_store
            .as_ref()
            .map(|s| s.tree())
            .unwrap_or_default();

        // Fast path: store already built at this generation.
        if self.sync_store.is_some() && prev.generation == gen && !prev.docs.is_empty() {
            return Ok(prev);
        }

        // Rebuild: fetch the root index (blob-cache served when its hash is known) and diff.
        // `snapshot()` re-polls the root ref internally, so the non-fast path costs a second
        // (cheap) root GET on top of the one above — acceptable next to the metadata fetches
        // it precedes, and never paid on the warm fast path. The stored tree takes its
        // generation from the snapshot, not the earlier poll, so it reflects the bytes read.
        let snap = self.snapshot().await?;
        let mut docs = std::collections::BTreeMap::new();
        for d in snap.docs() {
            if let Some(p) = prev.docs.get(&d.id) {
                if p.hash == d.hash {
                    docs.insert(d.id.clone(), p.clone()); // unchanged -> no fetch
                    continue;
                }
            }
            // Added or changed -> read just this doc's metadata.
            let meta = self.metadata_by(&d.hash, &d.id).await?;
            if meta.deleted {
                continue; // trashed/deleted docs never enter the resolved tree
            }
            docs.insert(
                d.id.clone(),
                ResolvedDoc {
                    hash: d.hash.clone(),
                    parent: meta.parent,
                    name: meta.visible_name,
                    is_folder: meta.doc_type == "CollectionType",
                },
            );
        }
        let tree = ResolvedTree {
            generation: snap.generation,
            docs,
        };
        if let Some(store) = &self.sync_store {
            store.store(&tree);
        }
        Ok(tree)
    }

    /// Cheap root-generation poll: fetch just the root ref and return its
    /// `generation`, or `None` if the account never synced (404). Mirrors
    /// [`snapshot`](Self::snapshot)'s single transparent token-refresh on 401, but
    /// skips the (potentially large) root-index blob download.
    pub async fn current_generation(&self) -> Result<Option<i64>> {
        let root = match self.get_root_ref().await {
            Err(Error::Unauthorized) => {
                self.force_refresh().await?;
                self.get_root_ref().await?
            }
            other => other?,
        };
        Ok(root.map(|r| r.generation))
    }

    /// GET the root ref; `Ok(None)` if the account has never synced (404).
    async fn get_root_ref(&self) -> Result<Option<RootResp>> {
        let token = self.user_token().await?;
        let resp = crate::transport::send_retrying(
            self.http.get(self.config.root_get()).bearer_auth(&token),
        )
        .await?;
        match resp.status() {
            reqwest::StatusCode::NOT_FOUND => Ok(None),
            s if s.is_success() => Ok(Some(resp.json::<RootResp>().await?)),
            reqwest::StatusCode::UNAUTHORIZED => Err(Error::Unauthorized),
            reqwest::StatusCode::TOO_MANY_REQUESTS => Err(Error::RateLimited),
            s => Err(Error::Http(format!("root get failed: {s}"))),
        }
    }

    // --- internal helpers reused by commit/porcelain (later tasks) ---

    /// GET a content-addressed blob (key == sha256(bytes)); cache reads are re-hash-verified.
    pub(crate) async fn get_blob(&self, hash: &str, name: &str) -> Result<Vec<u8>> {
        self.get_blob_inner(hash, name, true).await
    }

    /// GET an opaque-keyed blob (key is a stable id but NOT sha256(bytes), e.g. the per-doc
    /// index blob keyed by the Merkle doc hash); cache reads are NOT re-hash-verified.
    pub(crate) async fn get_blob_unverified(&self, hash: &str, name: &str) -> Result<Vec<u8>> {
        self.get_blob_inner(hash, name, false).await
    }

    async fn get_blob_inner(&self, hash: &str, name: &str, verify: bool) -> Result<Vec<u8>> {
        if let Some(cache) = &self.cache {
            let hit = if verify { cache.get(hash) } else { cache.get_unverified(hash) };
            if let Some(bytes) = hit {
                return Ok(bytes);
            }
        }
        let token = self.user_token().await?;
        let bytes = get_blob(&self.http, &self.config.blob(hash), &token, name).await?;
        // Best-effort write-through; cache failures must not fail the read.
        if let Some(cache) = &self.cache {
            let _ = cache.put(hash, &bytes);
        }
        Ok(bytes)
    }

    /// PUT a blob under `hash` with the given logical filename.
    #[tracing::instrument(name = "cloud.put_blob", skip_all, fields(name = %name))]
    pub(crate) async fn put_blob(&self, hash: &str, name: &str, bytes: Vec<u8>) -> Result<()> {
        let token = self.user_token().await?;
        if let Some(cache) = &self.cache {
            put_blob(&self.http, &self.config.blob(hash), &token, name, bytes.clone()).await?;
            // Best-effort write-through after a successful upload.
            let _ = cache.put(hash, &bytes);
        } else {
            put_blob(&self.http, &self.config.blob(hash), &token, name, bytes).await?;
        }
        Ok(())
    }

    /// Apply `mutation` atomically: upload new blobs, then CAS-put the root, rebasing on
    /// a stale generation. Returns the post-commit snapshot. Does NOT broadcast a change
    /// notification — see [`commit_broadcast`](Self::commit_broadcast).
    pub async fn commit(&self, mutation: Mutation) -> Result<Snapshot> {
        self.commit_with(mutation, false).await
    }

    /// Commit AND broadcast a change notification to the account's other subscribers (the
    /// reMarkable notification websocket). Normal sync uses [`commit`](Self::commit), which
    /// does NOT broadcast. This exists for clients that want to actively notify other devices
    /// (and for end-to-end push tests). The `rmapps watch` daemon never calls this — it must
    /// not self-notify.
    pub async fn commit_broadcast(&self, mutation: Mutation) -> Result<Snapshot> {
        self.commit_with(mutation, true).await
    }

    /// Shared commit implementation. `broadcast` controls the root PUT `broadcast` flag, which
    /// determines whether the reMarkable cloud pushes a wakeup frame to the account's other
    /// notification-socket subscribers. All normal callers pass `false`.
    #[tracing::instrument(name = "cloud.commit", skip_all)]
    async fn commit_with(&self, mutation: Mutation, broadcast: bool) -> Result<Snapshot> {
        // Prepare + upload doc blobs once (content-addressed → safe across retries).
        let prepared: Vec<_> = mutation.upserts.iter().map(prepare_doc).collect();
        for p in &prepared {
            for (hash, name, bytes) in &p.blobs {
                self.put_blob(hash, name, bytes.clone()).await?;
            }
        }

        const MAX_ATTEMPTS: u32 = 10;
        for _ in 0..MAX_ATTEMPTS {
            let snap = self.snapshot().await?;
            let current: Vec<_> = snap.docs().cloned().collect();
            let new_docs = apply(&current, &mutation, &prepared);
            let (rhash, rbytes) = root_blob(&new_docs);
            self.put_blob(&rhash, "root.docSchema", rbytes).await?;

            let token = self.user_token().await?;
            let resp = crate::transport::send_retrying(
                self.http
                    .put(self.config.root_put())
                    .bearer_auth(&token)
                    .header(crate::plumbing::blob::RM_FILENAME, "roothash")
                    .json(&RootPutReq {
                        broadcast,
                        hash: &rhash,
                        generation: snap.generation,
                    }),
            )
            .await?;
            match resp.status() {
                s if s.is_success() => {
                    let gen = resp.json::<RootPutResp>().await?.generation;
                    return Snapshot::from_root_index(gen, rhash, &serialize_root_index(&new_docs));
                }
                reqwest::StatusCode::PRECONDITION_FAILED => continue, // rebase: loop re-snapshots
                reqwest::StatusCode::UNAUTHORIZED => {
                    self.force_refresh().await?;
                    continue;
                }
                reqwest::StatusCode::TOO_MANY_REQUESTS => return Err(Error::RateLimited),
                s => return Err(Error::Http(format!("root put failed: {s}"))),
            }
        }
        Err(Error::CommitExhausted(MAX_ATTEMPTS))
    }

    /// Connect to the notification websocket, authenticated with the user token.
    /// Returns the message stream; the caller maps each message to a wakeup.
    ///
    /// One-way (receive-only): the server pushes a frame whenever the account may have
    /// changed (some other client committed with `broadcast: true`). We never send.
    pub async fn notifications_subscribe(&self) -> Result<NotifyStream> {
        match self.try_ws_connect().await {
            Ok(s) => Ok(s),
            Err(_first) => {
                // The cached user token may have expired (long-running daemon).
                // Mint a fresh one and retry the connect exactly once.
                self.force_refresh().await?;
                self.try_ws_connect().await
            }
        }
    }

    /// Single websocket connect attempt using the current user token.
    async fn try_ws_connect(&self) -> Result<NotifyStream> {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
        let token = self.user_token().await?;
        let mut req = self
            .config
            .notifications_ws()
            .into_client_request()
            .map_err(|e| Error::Http(format!("ws request: {e}")))?;
        req.headers_mut().insert(
            AUTHORIZATION,
            format!("Bearer {token}")
                .parse()
                .map_err(|e| Error::Http(format!("ws auth header: {e}")))?,
        );
        let (stream, _resp) = tokio_tungstenite::connect_async(req)
            .await
            .map_err(|e| Error::Http(format!("ws connect: {e}")))?;
        Ok(stream)
    }
}

/// The notification websocket stream returned by [`Client::notifications_subscribe`].
pub type NotifyStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

#[cfg(all(test, feature = "fake"))]
mod snapshot_memo {
    use super::*;
    use crate::fake::FakeCloud;
    use crate::porcelain::docfiles::DocFiles;

    #[tokio::test]
    async fn unchanged_generation_reuses_snapshot() {
        let fake = FakeCloud::spawn().await;
        // No BlobCache: only the generation memo can prevent the second root-index fetch.
        let client = Client::from_user_token(Config::single_host(&fake.base), "user-token");

        // Create one doc so a non-empty root index exists.
        client
            .put(DocFiles::new_pdf("Doc", "", b"%PDF-1.4\n".to_vec()))
            .await
            .unwrap();

        let s1 = client.snapshot().await.unwrap();
        let root_hash = s1.root_hash.clone();
        let gets_after_first = fake.blob_get_count(&root_hash);

        let s2 = client.snapshot().await.unwrap();
        assert_eq!(s1.generation, s2.generation);
        assert_eq!(
            fake.blob_get_count(&root_hash),
            gets_after_first,
            "unchanged generation must not refetch the root-index blob"
        );
    }
}

#[cfg(all(test, feature = "fake"))]
mod cache_integration {
    use super::*;
    use crate::cache::BlobCache;
    use crate::config::Config;
    use crate::fake::FakeCloud;

    #[tokio::test]
    async fn second_read_hits_cache_not_network() {
        let fake = FakeCloud::spawn().await;
        let dir = tempfile::tempdir().unwrap();
        let client = Client::from_user_token(Config::single_host(&fake.base), "user-token")
            .with_cache(BlobCache::new(dir.path()));

        let bytes = b"blobby".to_vec();
        let hash = crate::plumbing::index::sha256_hex(&bytes);
        fake.state.lock().unwrap().blobs.insert(hash.clone(), bytes.clone());

        let a = client.get_blob(&hash, "x").await.unwrap();
        let b = client.get_blob(&hash, "x").await.unwrap();
        assert_eq!(a, bytes);
        assert_eq!(b, bytes);
        assert_eq!(
            fake.blob_get_count(&hash),
            1,
            "second read must be served from cache"
        );
    }
}

#[cfg(all(test, feature = "fake"))]
mod resolved_tests {
    use super::*;
    use crate::fake::FakeCloud;
    use crate::porcelain::docfiles::DocFiles;
    use crate::sync_store::SyncStore;
    use crate::Metadata;

    fn pdf_doc(id: &str, name: &str, parent: &str) -> DocFiles {
        let meta = Metadata {
            visible_name: name.into(),
            doc_type: "DocumentType".into(),
            parent: parent.into(),
            last_modified: "0".into(),
            deleted: false,
            extra: Default::default(),
        };
        DocFiles {
            id: id.into(),
            files: vec![
                (format!("{id}.metadata"), serde_json::to_vec(&meta).unwrap()),
                (format!("{id}.content"), b"{}".to_vec()),
                (format!("{id}.pdf"), format!("pdf-{name}").into_bytes()),
            ],
        }
    }

    fn client_with_store(base: &str, dir: &std::path::Path) -> Client {
        Client::from_user_token(Config::single_host(base), "user-token")
            .with_sync_store(SyncStore::new(dir.join("sync-index.json")))
    }

    #[tokio::test]
    async fn current_generation_returns_store_without_metadata_fetches() {
        let fake = FakeCloud::spawn().await;
        let dir = tempfile::tempdir().unwrap();
        let client = client_with_store(&fake.base, dir.path());

        client.put(pdf_doc("a", "Alpha", "")).await.unwrap();
        client.put(pdf_doc("b", "Beta", "")).await.unwrap();
        let _ = client.resolved_snapshot().await.unwrap();

        let a_hash = client.snapshot().await.unwrap().doc("a").unwrap().hash.clone();
        let a_gets_before = fake.blob_get_count(&a_hash);
        // Capture AFTER the intervening snapshot() (which itself polls the root) so the
        // delta measures only resolved_snapshot()'s generation poll.
        let roots_before = fake.root_get_count();

        let tree = client.resolved_snapshot().await.unwrap();
        assert_eq!(tree.docs.len(), 2);
        assert_eq!(fake.root_get_count() - roots_before, 1, "exactly one generation poll");
        assert_eq!(
            fake.blob_get_count(&a_hash),
            a_gets_before,
            "no doc-index refetch when generation unchanged"
        );
    }

    #[tokio::test]
    async fn only_changed_doc_is_refetched() {
        let fake = FakeCloud::spawn().await;
        let dir = tempfile::tempdir().unwrap();
        let client = client_with_store(&fake.base, dir.path());
        client.put(pdf_doc("a", "Alpha", "")).await.unwrap();
        client.put(pdf_doc("b", "Beta", "")).await.unwrap();
        let _ = client.resolved_snapshot().await.unwrap();

        let a_hash = client.snapshot().await.unwrap().doc("a").unwrap().hash.clone();
        let a_gets_before = fake.blob_get_count(&a_hash);

        client.put(pdf_doc("b", "Beta v2", "")).await.unwrap();
        let tree = client.resolved_snapshot().await.unwrap();

        assert_eq!(tree.docs.get("b").unwrap().name, "Beta v2");
        assert_eq!(
            fake.blob_get_count(&a_hash),
            a_gets_before,
            "unchanged doc a must not be refetched"
        );
    }

    #[tokio::test]
    async fn removed_doc_drops_from_tree() {
        let fake = FakeCloud::spawn().await;
        let dir = tempfile::tempdir().unwrap();
        let client = client_with_store(&fake.base, dir.path());
        client.put(pdf_doc("a", "Alpha", "")).await.unwrap();
        client.put(pdf_doc("b", "Beta", "")).await.unwrap();
        let _ = client.resolved_snapshot().await.unwrap();

        client.rm("a").await.unwrap();
        let tree = client.resolved_snapshot().await.unwrap();
        assert!(tree.docs.get("a").is_none());
        assert!(tree.docs.get("b").is_some());
    }

    #[tokio::test]
    async fn deleted_doc_excluded_from_tree() {
        let fake = FakeCloud::spawn().await;
        let dir = tempfile::tempdir().unwrap();
        let client = client_with_store(&fake.base, dir.path());
        // A doc whose metadata is marked deleted must not surface in the resolved tree.
        let meta = crate::Metadata {
            visible_name: "Ghost".into(),
            doc_type: "DocumentType".into(),
            parent: "".into(),
            last_modified: "0".into(),
            deleted: true,
            extra: Default::default(),
        };
        let df = DocFiles {
            id: "ghost".into(),
            files: vec![
                ("ghost.metadata".into(), serde_json::to_vec(&meta).unwrap()),
                ("ghost.content".into(), b"{}".to_vec()),
                ("ghost.pdf".into(), b"x".to_vec()),
            ],
        };
        client.put(df).await.unwrap();
        client.put(pdf_doc("real", "Real", "")).await.unwrap();
        let tree = client.resolved_snapshot().await.unwrap();
        assert!(tree.docs.get("ghost").is_none(), "deleted doc must be excluded");
        assert!(tree.docs.get("real").is_some());
    }
}
