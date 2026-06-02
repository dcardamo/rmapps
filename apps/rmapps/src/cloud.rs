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

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use rm_cloud::{BlobCache, Client, Config, DocFiles, SyncStore};
use tokio::runtime::Runtime;

/// Per-user rmapps cache base: `$XDG_CACHE_HOME/rmapps`, else `~/.cache/rmapps`. The blob
/// cache and the sync index both live under here, so they stay siblings by construction
/// (the invariant `cache gc` relies on to never touch the index).
fn cache_base() -> PathBuf {
    std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache")
        })
        .join("rmapps")
}

/// Default blob-cache directory: `$RMAPPS_CACHE_DIR`, else `<cache-base>/blobs`
/// (`~/.cache/rmapps/blobs`).
pub fn default_cache_dir() -> PathBuf {
    if let Ok(d) = std::env::var("RMAPPS_CACHE_DIR") {
        return PathBuf::from(d);
    }
    cache_base().join("blobs")
}

/// Default sync-index path: `$RMAPPS_SYNC_INDEX`, else `<cache-base>/sync-index.json`
/// (a sibling of the `blobs/` dir, so `cache gc` never touches it).
pub fn default_sync_index_path() -> PathBuf {
    if let Ok(p) = std::env::var("RMAPPS_SYNC_INDEX") {
        return PathBuf::from(p);
    }
    cache_base().join("sync-index.json")
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
            .with_cache(BlobCache::new(default_cache_dir()))
            .with_sync_store(SyncStore::new(default_sync_index_path()));
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
        let tree = self
            .rt
            .block_on(self.client.resolved_snapshot())
            .map_err(|e| anyhow!("resolved_snapshot: {e}"))?;
        Ok(tree.resolve_folder(folder))
    }

    /// The id of the (non-folder) document named `name` directly under `folder_id`.
    ///
    /// Each call re-resolves via one cheap generation poll (a warm store returns the tree
    /// with zero blob fetches). This is intentional in a multi-step op like `replace` so a
    /// later read observes an earlier mutation rather than a stale cached tree.
    fn doc_id_in(&self, folder_id: &str, name: &str) -> Result<Option<String>> {
        let tree = self
            .rt
            .block_on(self.client.resolved_snapshot())
            .map_err(|e| anyhow!("resolved_snapshot: {e}"))?;
        Ok(tree
            .children(folder_id)
            .into_iter()
            .find(|e| !e.is_folder && e.name == name)
            .map(|e| e.id))
    }

    /// The ids of ALL (non-folder) documents named `name` directly under
    /// `folder_id`. Unlike [`Self::doc_id_in`], this returns every match — used by
    /// [`Self::replace`] to sweep duplicate docs that can accumulate from repeated
    /// pushes under eventual consistency.
    fn doc_ids_in(&self, folder_id: &str, name: &str) -> Result<Vec<String>> {
        let tree = self
            .rt
            .block_on(self.client.resolved_snapshot())
            .map_err(|e| anyhow!("resolved_snapshot: {e}"))?;
        Ok(tree
            .children(folder_id)
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
        self.upsert_in(&folder_id, name, pdf)
    }

    /// Create the doc only if it does not already exist; existing docs are left
    /// completely untouched (no upload), so on-device edits survive.
    pub fn create_if_missing(&self, folder: &str, name: &str, pdf: Vec<u8>) -> Result<()> {
        let folder_id = self.ensure_folder(folder)?;
        self.create_if_missing_in(&folder_id, name, pdf)
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
        self.replace_in(&folder_id, name, pdf)
    }

    /// `upsert` against an already-resolved folder id (no path resolution).
    pub fn upsert_in(&self, folder_id: &str, name: &str, pdf: Vec<u8>) -> Result<()> {
        match self.doc_id_in(folder_id, name)? {
            Some(id) => self
                .rt
                .block_on(self.client.put_content_only(&id, pdf))
                .map_err(|e| anyhow!("content-only update {name}: {e}")),
            None => self
                .rt
                .block_on(self.client.put(DocFiles::new_pdf(name, folder_id, pdf)))
                .map_err(|e| anyhow!("create {name}: {e}")),
        }
    }

    /// `create_if_missing` against an already-resolved folder id (no path resolution).
    pub fn create_if_missing_in(&self, folder_id: &str, name: &str, pdf: Vec<u8>) -> Result<()> {
        if self.doc_id_in(folder_id, name)?.is_some() {
            return Ok(());
        }
        self.rt
            .block_on(self.client.put(DocFiles::new_pdf(name, folder_id, pdf)))
            .map_err(|e| anyhow!("create {name}: {e}"))
    }

    /// `replace` against an already-resolved folder id (no path resolution). Sweeps
    /// EVERY same-named doc before creating, so it converges pre-existing duplicates.
    pub fn replace_in(&self, folder_id: &str, name: &str, pdf: Vec<u8>) -> Result<()> {
        for id in self.doc_ids_in(folder_id, name)? {
            // Best-effort remove; individual failures surface on the create below.
            let _ = self.rt.block_on(self.client.rm(&id));
        }
        self.rt
            .block_on(self.client.put(DocFiles::new_pdf(name, folder_id, pdf)))
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

    /// Recursively list documents under `root`, excluding generated suffixes. One resolved
    /// snapshot; the walk is pure (no per-folder IO).
    pub fn list_recursive(&self, root: &str, exclude_suffixes: &[String]) -> Result<Vec<RemoteDoc>> {
        let tree = self
            .rt
            .block_on(self.client.resolved_snapshot())
            .map_err(|e| anyhow!("resolved_snapshot: {e}"))?;
        let Some(root_id) = tree.resolve_folder(root) else {
            return Ok(Vec::new());
        };
        let root_path = normalize_path(root);
        let mut out = Vec::new();
        walk_tree(&tree, &root_id, &root_path, exclude_suffixes, &mut out);
        Ok(out)
    }
}

/// Run-scoped memo of folder path → resolved id. The first `get` for a path performs
/// the one `ensure_folder` (hence the one possible `mkdir`); later `get`s for the same
/// path return the cached id with no cloud call. Construct one per run/task — NOT per
/// `Cloud`, which the `watch` daemon keeps alive across tasks (a folder can be trashed
/// and recreated between tasks, so a `Cloud`-lifetime cache would deploy into a deleted
/// folder).
pub struct FolderIds<'a> {
    cloud: &'a Cloud,
    ids: HashMap<String, String>,
}

impl<'a> FolderIds<'a> {
    /// A fresh, empty resolver bound to `cloud`.
    pub fn new(cloud: &'a Cloud) -> Self {
        Self {
            cloud,
            ids: HashMap::new(),
        }
    }

    /// Resolve `path` to a folder id, creating it on first miss; cached thereafter.
    pub fn get(&mut self, path: &str) -> Result<String> {
        if let Some(id) = self.ids.get(path) {
            return Ok(id.clone());
        }
        let id = self.cloud.ensure_folder(path)?;
        self.ids.insert(path.to_string(), id.clone());
        Ok(id)
    }
}

/// Pure recursive walk over a resolved tree (no IO). Collects non-folder docs,
/// skipping any whose name ends with one of `exclude_suffixes`.
fn walk_tree(
    tree: &rm_cloud::ResolvedTree,
    folder_id: &str,
    folder_path: &str,
    exclude_suffixes: &[String],
    out: &mut Vec<RemoteDoc>,
) {
    for e in tree.children(folder_id) {
        let child_path = if folder_path.ends_with('/') {
            format!("{folder_path}{}", e.name)
        } else {
            format!("{folder_path}/{}", e.name)
        };
        if e.is_folder {
            walk_tree(tree, &e.id, &child_path, exclude_suffixes, out);
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

    /// A warm `replace` (sync store already populated) fetches a small CONSTANT
    /// number of metadata/blob GETs, independent of how many docs the account
    /// holds. The metric that actually captures this is blob/metadata GETs, NOT
    /// root polls: root polls are a small constant in both the new (store-backed)
    /// and the old (store-less) code, so they would have passed even under the
    /// old broken behavior. The N-scaling regression lived entirely in the old
    /// `ls`, which did 2*N metadata fetches per call (~209 GETs at N=50). The
    /// store-backed path resolves unrelated siblings as store hits and only
    /// fetches metadata for the genuinely changed doc.
    #[test]
    fn warm_replace_is_account_size_independent() {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let fake = rt.block_on(FakeCloud::spawn());
        let dir = tempfile::tempdir().unwrap();
        let client = Client::from_user_token(CloudConfig::single_host(&fake.base), "user-token")
            .with_sync_store(rm_cloud::SyncStore::new(dir.path().join("idx.json")));
        let cloud = cloud_from_client(client);

        cloud.ensure_folder("/Readwise").unwrap();
        cloud.replace("/Readwise", "Feed", b"feed-v1".to_vec()).unwrap();
        cloud.replace("/Readwise", "Library", b"lib-v1".to_vec()).unwrap();
        // Seed several unrelated sibling docs — this is the "account size" the old
        // store-less `ls` would have re-scanned (2*N metadata GETs) on every replace.
        for i in 0..8 {
            cloud
                .create_if_missing("/Readwise", &format!("Extra{i}"), format!("extra-{i}").into_bytes())
                .unwrap();
        }
        let _ = cloud.list_recursive("/Readwise", &[]).unwrap(); // warm the store

        let blobs_before = fake.blob_count_total();
        cloud.replace("/Readwise", "Feed", b"feed-v2".to_vec()).unwrap();
        let blob_delta = fake.blob_count_total() - blobs_before;

        // Observed: blob_delta == 1 (only Feed's own new content blob is fetched;
        // the 8 unrelated Extra docs + Library are resolved as store hits, never
        // refetched). The old 2*N behavior would fetch metadata for all ~11 docs
        // (~22 GETs, possibly multiple times per replace) — far above this bound,
        // which is exactly what makes it account-size-independent.
        assert!(
            blob_delta <= 3,
            "warm replace refetched unrelated docs' metadata: {blob_delta} blob GETs"
        );
    }

    /// `replace_in` deploys into an already-resolved folder id and sweeps duplicates,
    /// without doing any path resolution itself.
    #[test]
    fn replace_in_targets_resolved_folder_id() {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let fake = rt.block_on(FakeCloud::spawn());
        let client = Client::from_user_token(CloudConfig::single_host(&fake.base), "user-token");
        let cloud = cloud_from_client(client);

        let folder_id = cloud.ensure_folder("/Readwise").unwrap();
        cloud.replace_in(&folder_id, "Feed", b"%PDF-1".to_vec()).unwrap();
        cloud.replace_in(&folder_id, "Feed", b"%PDF-2".to_vec()).unwrap();

        // Exactly one "Feed" doc remains under the resolved folder id.
        assert_eq!(cloud.doc_ids_in(&folder_id, "Feed").unwrap().len(), 1);
    }

    /// Companion to the `mkdir_p` bug-lock (`lagging_index_after_mkdir_duplicates_folder`):
    /// under the SAME eventual-consistency lag that makes a naive double-resolve mint a
    /// duplicate folder, a run-scoped `FolderIds` resolves "/Readwise" once and reuses the
    /// id (the second `get` is a memo hit that never re-queries the cloud), so no second
    /// `mkdir` can happen. `/Seed` is created first so the lag window serves a valid stale
    /// index rather than an empty one.
    #[test]
    fn resolver_prevents_duplicate_folder_under_lag() {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let fake = rt.block_on(FakeCloud::spawn());
        let dir = tempfile::tempdir().unwrap();
        let client = Client::from_user_token(CloudConfig::single_host(&fake.base), "user-token")
            .with_sync_store(rm_cloud::SyncStore::new(dir.path().join("idx.json")));
        let cloud = cloud_from_client(client);

        cloud.ensure_folder("/Seed").unwrap();
        fake.lag_next_commit(4);

        let mut folders = FolderIds::new(&cloud);
        let id_a = folders.get("/Readwise").unwrap();
        let id_b = folders.get("/Readwise").unwrap();

        assert_eq!(id_a, id_b);
    }

    /// bujo deploys many PDFs to ONE target folder; a single resolver must yield one
    /// stable id for all of them (so only one folder is created).
    #[test]
    fn resolver_single_target_one_folder_for_many_docs() {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let fake = rt.block_on(FakeCloud::spawn());
        let client = Client::from_user_token(CloudConfig::single_host(&fake.base), "user-token");
        let cloud = cloud_from_client(client);

        let mut folders = FolderIds::new(&cloud);
        let mut ids = std::collections::HashSet::new();
        for i in 0..14 {
            let id = folders.get("/2026").unwrap();
            cloud.upsert_in(&id, &format!("2026.{i:02} Doc"), b"%PDF".to_vec()).unwrap();
            ids.insert(id);
        }
        assert_eq!(ids.len(), 1, "all 14 docs resolved the same /2026 folder id");
    }
}
