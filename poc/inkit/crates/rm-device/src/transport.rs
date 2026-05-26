//! reMarkable on-device transport, backed by the native `rm-cloud` client (the
//! pure-Rust reMarkable Cloud sync protocol) — no `rmapi` CLI, no shelling out.
//!
//! The mapping from the framework's device-neutral key/PDF/ink model onto cloud
//! documents:
//!
//! - The deploy **folder** is a slash path (e.g. `/ReadingQueue`); `mkdir_p`
//!   resolves it to a folder id, creating missing levels.
//! - A document's **app key is its `visibleName`** under that folder. `push`
//!   finds the doc by name and swaps only the PDF blob (content-only, so
//!   on-device ink survives — mechanics §3), creating the doc on first push.
//! - `pull` lists the folder and decodes each document's `.rm` ink, keyed by its
//!   `visibleName`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use inkapp_core::device::Device;
use inkapp_core::error::{Error, Result};
use inkapp_core::ink::Stroke;
use inkapp_core::sync::DeviceTransport;
use rm_cloud::{Client, DocFiles};
use rm_files::Bundle;

use crate::Remarkable;

/// Decode every page of a bundle into PDF-space strokes, indexed by the bundle's
/// `.content` page order: slot `p` aligns with the manifest's `region.page == p`.
/// An un-inked page occupies its slot as an empty `Vec`, so it never shifts later
/// pages. All pages share one `page_h` (Typst `#set page` fixes every page to the
/// same height).
pub fn strokes_from_bundle(device: &Remarkable, bundle: &Bundle, page_h: f64) -> Vec<Vec<Stroke>> {
    bundle
        .pages()
        .into_iter()
        .map(|pg| match pg.scene_bytes() {
            Some(bytes) => device.read_ink(bytes, page_h).unwrap_or_default(),
            None => Vec::new(),
        })
        .collect()
}

/// Open an `.rmdoc` at `path` and decode its per-page ink (see
/// [`strokes_from_bundle`]). Empty if the bundle won't open.
pub fn strokes_by_page(device: &Remarkable, path: &Path, page_h: f64) -> Vec<Vec<Stroke>> {
    match Bundle::open(path) {
        Ok(bundle) => strokes_from_bundle(device, &bundle, page_h),
        Err(_) => Vec::new(),
    }
}

/// reMarkable transport over the reMarkable cloud. Maps keys/PDFs/ink onto cloud
/// documents via `rm-cloud`; the deploy folder's id is resolved once and cached.
pub struct CloudTransport {
    client: Client,
    folder: String,
    device: Remarkable,
    folder_id: OnceLock<String>,
}

impl CloudTransport {
    /// A transport talking to the reMarkable cloud with credentials from the
    /// environment (`RM_CLOUD_DEVICE_TOKEN` / `RM_CLOUD_USER_TOKEN`), deploying
    /// under `folder`.
    pub fn from_env(folder: impl Into<String>) -> Result<Self> {
        let client = Client::from_env().map_err(|e| Error::Transport(format!("rm-cloud: {e}")))?;
        Ok(Self::with_client(client, folder))
    }

    /// A transport with credentials resolved from `SecretStore` (preferred) or
    /// `RM_CLOUD_*` env (fallback). The endpoint config comes from
    /// `Config::from_env()` like [`from_env`]; tests that need a fake host can
    /// still set `RM_CLOUD_HOST`, or construct via [`with_client`] directly.
    pub fn from_secrets(
        secrets: &inkapp_core::secrets::SecretStore,
        folder: impl Into<String>,
    ) -> Result<Self> {
        use rm_cloud::{Client, Config};
        let creds = crate::auth::resolve_credentials(secrets)
            .map_err(|e| Error::Transport(format!("rm-cloud: {e}")))?;
        let config = Config::from_env();
        let client = match (creds.device_token, creds.user_token) {
            (Some(d), _) => Client::from_device_token(config, d),
            (None, Some(u)) => Client::from_user_token(config, u),
            // resolve_credentials returns MissingCredential before reaching here.
            (None, None) => unreachable!("resolve_credentials guarantees at least one token"),
        };
        Ok(Self::with_client(client, folder))
    }

    /// A transport over an explicit client (tests pass a fake-cloud client).
    pub fn with_client(client: Client, folder: impl Into<String>) -> Self {
        Self {
            client,
            folder: folder.into(),
            device: Remarkable::new(),
            folder_id: OnceLock::new(),
        }
    }

    /// Resolve (and cache) the deploy folder's id, creating the path if missing.
    async fn folder_id(&self) -> Result<String> {
        if let Some(id) = self.folder_id.get() {
            return Ok(id.clone());
        }
        let id = self
            .client
            .mkdir_p(&self.folder)
            .await
            .map_err(|e| Error::Transport(format!("rm-cloud mkdir_p {}: {e}", self.folder)))?;
        // Race-tolerant: a concurrent resolve may have set it first — keep that.
        let _ = self.folder_id.set(id.clone());
        Ok(self.folder_id.get().cloned().unwrap_or(id))
    }

    /// The id of the document under `folder_id` whose `visibleName` is `key`, if any.
    async fn doc_id_for(&self, folder_id: &str, key: &str) -> Result<Option<String>> {
        let listing = self
            .client
            .ls(folder_id)
            .await
            .map_err(|e| Error::Transport(format!("rm-cloud ls: {e}")))?;
        Ok(listing
            .into_iter()
            .find(|e| !e.is_folder && e.name == key)
            .map(|e| e.id))
    }
}

#[async_trait::async_trait]
impl DeviceTransport for CloudTransport {
    async fn push(&self, key: &str, pdf: &[u8]) -> Result<()> {
        let folder_id = self.folder_id().await?;
        match self.doc_id_for(&folder_id, key).await? {
            // Existing doc: swap only the PDF blob so on-device ink survives (mechanics §3).
            Some(id) => self
                .client
                .put_content_only(&id, pdf.to_vec())
                .await
                .map_err(|e| Error::Transport(format!("rm-cloud put_content_only {key}: {e}"))),
            // New doc: create a fresh PDF document named after the key.
            None => self
                .client
                .put(DocFiles::new_pdf(key, &folder_id, pdf.to_vec()))
                .await
                .map_err(|e| Error::Transport(format!("rm-cloud put {key}: {e}"))),
        }
    }

    async fn delete(&self, key: &str) {
        // Best-effort: a missing folder/doc is not an error.
        let Ok(folder_id) = self.folder_id().await else {
            return;
        };
        if let Ok(Some(id)) = self.doc_id_for(&folder_id, key).await {
            let _ = self.client.rm(&id).await;
        }
    }

    async fn pull(
        &self,
        page_h_by_key: &HashMap<String, f64>,
    ) -> HashMap<String, Vec<Vec<Stroke>>> {
        let mut out = HashMap::new();
        let Ok(folder_id) = self.folder_id().await else {
            return out;
        };
        let Ok(listing) = self.client.ls(&folder_id).await else {
            return out;
        };
        for entry in listing {
            if entry.is_folder {
                continue;
            }
            // The app key is the document's visibleName; decode at that key's page height.
            let key = entry.name;
            let page_h = page_h_by_key.get(&key).copied().unwrap_or(0.0);
            let Ok(bundle) = self.client.get_bundle(&entry.id).await else {
                continue;
            };
            let pages = strokes_from_bundle(&self.device, &bundle, page_h);
            // Insert only when the document carries ink on some page.
            if pages.iter().any(|pg| !pg.is_empty()) {
                out.insert(key, pages);
            }
        }
        out
    }
}
