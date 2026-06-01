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
use rm_cloud::{BlobCache, Client, Config, DocFiles};
use tokio::runtime::Runtime;

/// Default blob-cache directory: `$RMAPPS_CACHE_DIR`, else `$XDG_CACHE_HOME/rmapps/blobs`,
/// else `~/.cache/rmapps/blobs`.
pub fn default_cache_dir() -> PathBuf {
    if let Ok(d) = std::env::var("RMAPPS_CACHE_DIR") {
        return PathBuf::from(d);
    }
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache")
        });
    base.join("rmapps").join("blobs")
}

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
    /// Cloud content hash (used by the digest cheap-skip).
    pub hash: String,
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
        let client = Client::from_device_token(Config::from_env(), token)
            .with_cache(BlobCache::new(default_cache_dir()));
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

    /// Cheap root-generation poll: `Some(generation)`, or `None` if the account
    /// never synced. Used by the sync engine's `on-change` trigger to detect
    /// whether the cloud moved without downloading a full snapshot.
    pub fn current_generation(&self) -> Result<Option<i64>> {
        self.rt
            .block_on(self.client.current_generation())
            .map_err(|e| anyhow!("current_generation: {e}"))
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

    /// The ids of ALL (non-folder) documents named `name` directly under
    /// `folder_id`. Unlike [`Self::doc_id_in`], this returns every match — used by
    /// [`Self::replace`] to sweep duplicate docs that can accumulate from repeated
    /// pushes under eventual consistency.
    fn doc_ids_in(&self, folder_id: &str, name: &str) -> Result<Vec<String>> {
        let entries = self
            .rt
            .block_on(self.client.ls(folder_id))
            .map_err(|e| anyhow!("ls: {e}"))?;
        Ok(entries
            .into_iter()
            .filter(|e| !e.is_folder && e.name == name)
            .map(|e| e.id)
            .collect())
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

    /// Destructive replace: remove EVERY existing doc of this name, then create a
    /// fresh one. For write-only docs (reader PDFs, digests) with no ink to keep.
    ///
    /// We delete all same-named matches (not just the first) so this is idempotent
    /// against pre-existing duplicates: repeated pushes and the cloud's eventual
    /// consistency could otherwise leave several "Feed"/"Library" docs in a folder,
    /// and a one-doc remove would never converge back to a single copy.
    pub fn replace(&self, folder: &str, name: &str, pdf: Vec<u8>) -> Result<()> {
        let folder_id = self.ensure_folder(folder)?;
        for id in self.doc_ids_in(&folder_id, name)? {
            // Best-effort remove; individual failures surface on the create below.
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
    /// does not exist. Fetches the snapshot ONCE and reuses it across all folders.
    pub fn list_recursive(&self, root: &str, exclude_suffixes: &[String]) -> Result<Vec<RemoteDoc>> {
        let snap = self
            .rt
            .block_on(self.client.snapshot())
            .map_err(|e| anyhow!("snapshot: {e}"))?;
        let Some(root_id) = self.resolve_folder_in(&snap, root)? else {
            return Ok(Vec::new());
        };
        let root_path = normalize_path(root);
        let mut out = Vec::new();
        self.walk(&snap, &root_id, &root_path, exclude_suffixes, &mut out)?;
        Ok(out)
    }

    fn walk(
        &self,
        snap: &rm_cloud::Snapshot,
        folder_id: &str,
        folder_path: &str,
        exclude_suffixes: &[String],
        out: &mut Vec<RemoteDoc>,
    ) -> Result<()> {
        let entries = self
            .rt
            .block_on(self.client.ls_with(snap, folder_id))
            .map_err(|e| anyhow!("ls {folder_path}: {e}"))?;
        for e in entries {
            let child_path = if folder_path.ends_with('/') {
                format!("{folder_path}{}", e.name)
            } else {
                format!("{folder_path}/{}", e.name)
            };
            if e.is_folder {
                self.walk(snap, &e.id, &child_path, exclude_suffixes, out)?;
            } else if !exclude_suffixes.iter().any(|s| e.name.ends_with(s.as_str())) {
                out.push(RemoteDoc {
                    id: e.id,
                    name: e.name,
                    folder: folder_path.to_string(),
                    path: child_path,
                    hash: e.hash,
                });
            }
        }
        Ok(())
    }

    /// Resolve a slash path to a folder id against an existing snapshot (no extra root fetch).
    /// `Ok(None)` if any segment is missing. Root (`""`/`"/"`) resolves to `""`.
    fn resolve_folder_in(&self, snap: &rm_cloud::Snapshot, folder: &str) -> Result<Option<String>> {
        let mut parent = String::new();
        for seg in folder.split('/').filter(|s| !s.is_empty()) {
            let entries = self
                .rt
                .block_on(self.client.ls_with(snap, &parent))
                .map_err(|e| anyhow!("ls {parent:?}: {e}"))?;
            match entries.into_iter().find(|e| e.is_folder && e.name == seg) {
                Some(e) => parent = e.id,
                None => return Ok(None),
            }
        }
        Ok(Some(parent))
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

#[cfg(test)]
mod tests {
    use super::*;
    use rm_cloud::fake::FakeCloud;
    use rm_cloud::{Config as CloudConfig, Metadata};

    /// Build a `Cloud` wrapping an explicit client (test seam — avoids the
    /// process-global `RM_CLOUD_HOST` that `from_env`/`from_*_token` rely on, so
    /// this test can run in parallel with the reactor test against its own fake).
    fn cloud_from_client(client: Client) -> Cloud {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        Cloud { rt, client }
    }

    fn doc_with_pdf(id: &str, name: &str, parent: &str, pdf: &[u8]) -> DocFiles {
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
                (format!("{id}.pdf"), pdf.to_vec()),
            ],
        }
    }

    /// `replace()` must sweep EVERY same-named doc (the dedup invariant), leaving
    /// exactly one copy even when the folder already holds duplicates.
    #[test]
    fn replace_removes_all_same_named_docs() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let fake = rt.block_on(FakeCloud::spawn());
        let seed = Client::from_user_token(CloudConfig::single_host(&fake.base), "user-token");

        // Seed /Readwise with THREE docs all named "Feed" (the duplicate state).
        let folder = rt.block_on(seed.mkdir("Readwise", "")).unwrap();
        for id in [
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        ] {
            rt.block_on(seed.put(doc_with_pdf(id, "Feed", &folder, b"%PDF-old")))
                .unwrap();
        }

        let cloud = cloud_from_client(Client::from_user_token(
            CloudConfig::single_host(&fake.base),
            "user-token",
        ));
        // Pre-condition: doc_ids_in sees all three duplicates.
        assert_eq!(cloud.doc_ids_in(&folder, "Feed").unwrap().len(), 3);

        cloud.replace("/Readwise", "Feed", b"%PDF-new".to_vec()).unwrap();

        // Post-condition: exactly one "Feed" remains.
        assert_eq!(cloud.doc_ids_in(&folder, "Feed").unwrap().len(), 1);
    }
}
