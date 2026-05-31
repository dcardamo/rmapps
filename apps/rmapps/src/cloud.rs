//! Cloud deploy operations on top of `rm-cloud` — the native replacement for
//! the three apps' old rmapi backends.
//!
//! The domain crates are synchronous; `rm-cloud` is async. This wraps one
//! [`rm_cloud::Client`] behind a shared Tokio runtime and exposes the handful
//! of path-shaped operations the apps need:
//!
//! - [`Cloud::upsert`] — create, or content-only refresh (preserves on-device ink).
//! - [`Cloud::create_if_missing`] — create only when absent; leave existing docs alone.
//! - [`Cloud::replace`] — destructive remove-then-create (for write-only docs).
//! - [`Cloud::fetch_bundle`] — download a doc to a temp `.rmdoc`.
//! - [`Cloud::list_recursive`] — walk a folder subtree, excluding generated docs.
//!
//! reMarkable visible names carry no extension, so callers pass the bare doc
//! name (the PDF file stem), not a path.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use rm_cloud::{Client, Config, DocFiles};
use tokio::runtime::Runtime;

/// A document discovered by [`Cloud::list_recursive`].
#[derive(Debug, Clone)]
pub struct RemoteDoc {
    /// Document id (UUID).
    pub id: String,
    /// Visible name (leaf).
    pub name: String,
    /// Parent folder path, e.g. `/Books/Author`.
    pub folder: String,
    /// Full path, e.g. `/Books/Author/Title`.
    pub path: String,
}

/// A native cloud client with synchronous, path-shaped deploy helpers.
pub struct Cloud {
    rt: Runtime,
    client: Client,
}

impl Cloud {
    /// Build from the stored device token (`~/.config/rmapps/auth.json`).
    pub fn from_stored() -> Result<Self> {
        let token = crate::auth::load_device_token()?;
        Self::from_device_token(token)
    }

    /// Build from an explicit device token.
    pub fn from_device_token(token: String) -> Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("building Tokio runtime")?;
        let client = Client::from_device_token(Config::from_env(), token);
        Ok(Self { rt, client })
    }

    /// The underlying async client, for callers that need the full surface
    /// (e.g. the sync orchestrator sharing one snapshot).
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Run a future to completion on the shared runtime.
    pub fn block_on<F: std::future::Future>(&self, fut: F) -> F::Output {
        self.rt.block_on(fut)
    }

    /// Resolve a slash path to a folder id, creating missing segments (`mkdir -p`).
    pub fn ensure_folder(&self, folder: &str) -> Result<String> {
        self.rt
            .block_on(self.client.mkdir_p(folder))
            .map_err(|e| anyhow!("mkdir_p {folder}: {e}"))
    }

    /// Resolve a slash path to a folder id WITHOUT creating anything.
    /// `Ok(None)` if any segment is missing. Root (`""`/`"/"`) resolves to `""`.
    pub fn resolve_folder(&self, folder: &str) -> Result<Option<String>> {
        let mut parent = String::new();
        for seg in folder.split('/').filter(|s| !s.is_empty()) {
            let entries = self
                .rt
                .block_on(self.client.ls(&parent))
                .map_err(|e| anyhow!("ls {parent:?}: {e}"))?;
            match entries.into_iter().find(|e| e.is_folder && e.name == seg) {
                Some(e) => parent = e.id,
                None => return Ok(None),
            }
        }
        Ok(Some(parent))
    }

    /// The id of the (non-folder) document named `name` directly under `folder_id`.
    fn doc_id_in(&self, folder_id: &str, name: &str) -> Result<Option<String>> {
        let entries = self
            .rt
            .block_on(self.client.ls(folder_id))
            .map_err(|e| anyhow!("ls: {e}"))?;
        Ok(entries
            .into_iter()
            .find(|e| !e.is_folder && e.name == name)
            .map(|e| e.id))
    }

    /// Create the doc if absent, else replace only its PDF blob (content-only),
    /// preserving on-device handwriting (mechanics §3). `folder` is created if
    /// missing.
    pub fn upsert(&self, folder: &str, name: &str, pdf: Vec<u8>) -> Result<()> {
        let folder_id = self.ensure_folder(folder)?;
        match self.doc_id_in(&folder_id, name)? {
            Some(id) => self
                .rt
                .block_on(self.client.put_content_only(&id, pdf))
                .map_err(|e| anyhow!("content-only update {name}: {e}")),
            None => self
                .rt
                .block_on(self.client.put(DocFiles::new_pdf(name, &folder_id, pdf)))
                .map_err(|e| anyhow!("create {name}: {e}")),
        }
    }

    /// Create the doc only if it does not already exist; existing docs are left
    /// completely untouched (no upload), so on-device edits survive.
    pub fn create_if_missing(&self, folder: &str, name: &str, pdf: Vec<u8>) -> Result<()> {
        let folder_id = self.ensure_folder(folder)?;
        if self.doc_id_in(&folder_id, name)?.is_some() {
            return Ok(());
        }
        self.rt
            .block_on(self.client.put(DocFiles::new_pdf(name, &folder_id, pdf)))
            .map_err(|e| anyhow!("create {name}: {e}"))
    }

    /// Destructive replace: remove any existing doc of this name, then create a
    /// fresh one. For write-only docs (reader PDFs, digests) with no ink to keep.
    pub fn replace(&self, folder: &str, name: &str, pdf: Vec<u8>) -> Result<()> {
        let folder_id = self.ensure_folder(folder)?;
        if let Some(id) = self.doc_id_in(&folder_id, name)? {
            // Best-effort remove; a failure here surfaces on the create below.
            let _ = self.rt.block_on(self.client.rm(&id));
        }
        self.rt
            .block_on(self.client.put(DocFiles::new_pdf(name, &folder_id, pdf)))
            .map_err(|e| anyhow!("replace {name}: {e}"))
    }

    /// Download `<folder>/<name>` to a temp `.rmdoc`; `Ok(None)` if it doesn't
    /// exist. The folder is resolved read-only (not created).
    pub fn fetch_bundle(&self, folder: &str, name: &str) -> Result<Option<PathBuf>> {
        let Some(folder_id) = self.resolve_folder(folder)? else {
            return Ok(None);
        };
        let Some(id) = self.doc_id_in(&folder_id, name)? else {
            return Ok(None);
        };
        self.fetch_bundle_by_id(&id, name)
    }

    /// Download a doc by id to a stable temp `.rmdoc`.
    pub fn fetch_bundle_by_id(&self, id: &str, name: &str) -> Result<Option<PathBuf>> {
        let df = self
            .rt
            .block_on(self.client.get(id))
            .map_err(|e| anyhow!("get {id}: {e}"))?;
        let dest = std::env::temp_dir().join(format!("rmapps-{name}.rmdoc"));
        let _ = std::fs::remove_file(&dest);
        df.write_rmdoc(&dest)
            .map_err(|e| anyhow!("write rmdoc {}: {e}", dest.display()))?;
        Ok(Some(dest))
    }

    /// Recursively list documents under `root` (a slash path), excluding any
    /// whose visible name ends with one of `exclude_suffixes`. Empty if `root`
    /// does not exist.
    pub fn list_recursive(&self, root: &str, exclude_suffixes: &[String]) -> Result<Vec<RemoteDoc>> {
        let Some(root_id) = self.resolve_folder(root)? else {
            return Ok(Vec::new());
        };
        let root_path = normalize_path(root);
        let mut out = Vec::new();
        self.walk(&root_id, &root_path, exclude_suffixes, &mut out)?;
        Ok(out)
    }

    fn walk(
        &self,
        folder_id: &str,
        folder_path: &str,
        exclude_suffixes: &[String],
        out: &mut Vec<RemoteDoc>,
    ) -> Result<()> {
        let entries = self
            .rt
            .block_on(self.client.ls(folder_id))
            .map_err(|e| anyhow!("ls {folder_path}: {e}"))?;
        for e in entries {
            let child_path = if folder_path.ends_with('/') {
                format!("{folder_path}{}", e.name)
            } else {
                format!("{folder_path}/{}", e.name)
            };
            if e.is_folder {
                self.walk(&e.id, &child_path, exclude_suffixes, out)?;
            } else if !exclude_suffixes.iter().any(|s| e.name.ends_with(s.as_str())) {
                out.push(RemoteDoc {
                    id: e.id,
                    name: e.name,
                    folder: folder_path.to_string(),
                    path: child_path,
                });
            }
        }
        Ok(())
    }
}

/// Normalize a folder path to a single leading slash and no trailing slash
/// (`""`/`"/"` → `""`).
fn normalize_path(path: &str) -> String {
    let trimmed = path.trim().trim_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("/{trimmed}")
    }
}

/// Strip a `.pdf` extension to get the reMarkable visible name (the file stem).
pub fn doc_name(pdf: &Path) -> Result<String> {
    pdf.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("no file stem: {}", pdf.display()))
}
