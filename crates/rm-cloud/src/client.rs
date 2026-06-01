//! The cloud client: holds credentials + config, refreshes the user token on demand,
//! and exposes snapshot/porcelain/sync entry points.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::auth::{refresh_user_token, Credentials};
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
        }
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

    /// Fetch the current account snapshot (empty if the account never synced).
    pub async fn snapshot(&self) -> Result<Snapshot> {
        // Fetch the root ref, with one transparent refresh on 401.
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
        Snapshot::from_root_index(root.generation, root.hash, &bytes)
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
        let resp = self
            .http
            .get(self.config.root_get())
            .bearer_auth(&token)
            .send()
            .await?;
        match resp.status() {
            reqwest::StatusCode::NOT_FOUND => Ok(None),
            s if s.is_success() => Ok(Some(resp.json::<RootResp>().await?)),
            reqwest::StatusCode::UNAUTHORIZED => Err(Error::Unauthorized),
            s => Err(Error::Http(format!("root get failed: {s}"))),
        }
    }

    // --- internal helpers reused by commit/porcelain (later tasks) ---

    /// GET a blob by hash and logical filename.
    pub(crate) async fn get_blob(&self, hash: &str, name: &str) -> Result<Vec<u8>> {
        let token = self.user_token().await?;
        get_blob(&self.http, &self.config.blob(hash), &token, name).await
    }

    /// PUT a blob under `hash` with the given logical filename.
    pub(crate) async fn put_blob(&self, hash: &str, name: &str, bytes: Vec<u8>) -> Result<()> {
        let token = self.user_token().await?;
        put_blob(&self.http, &self.config.blob(hash), &token, name, bytes).await
    }

    /// Apply `mutation` atomically: upload new blobs, then CAS-put the root, rebasing on
    /// a stale generation. Returns the post-commit snapshot.
    pub async fn commit(&self, mutation: Mutation) -> Result<Snapshot> {
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
            let resp = self
                .http
                .put(self.config.root_put())
                .bearer_auth(&token)
                .header(crate::plumbing::blob::RM_FILENAME, "roothash")
                .json(&RootPutReq {
                    broadcast: false,
                    hash: &rhash,
                    generation: snap.generation,
                })
                .send()
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
