//! Targeted reactive actions over the shared Cloud. Best-effort: errors are
//! returned to the caller, which logs and continues (never crashes the daemon).
use anyhow::Result;
use std::path::{Path, PathBuf};

use rmdigest::deploy::{Backend, CloudDoc};
use rmreader::deploy::BundleFetch;
use rmreader::readback;
use rmreader::readwise::http::UreqTransport;

use crate::cloud::Cloud;
use crate::config::{Config, WatchAction};
use crate::watch::reconcile::Job;

/// Adapts the native cloud client to rmdigest's `Backend` seam. Mirrors the
/// adapter in `crate::digest`.
struct CloudBackend<'a> {
    cloud: &'a Cloud,
}
impl Backend for CloudBackend<'_> {
    fn list(&self, root: &str, ex: &[String]) -> Result<Vec<CloudDoc>> {
        Ok(self
            .cloud
            .list_recursive(root, ex)?
            .into_iter()
            .map(|d| CloudDoc {
                path: d.path,
                name: d.name,
                folder: d.folder,
                version: None,
            })
            .collect())
    }
    fn fetch(&self, doc: &CloudDoc) -> Result<Option<PathBuf>> {
        self.cloud.fetch_bundle(&doc.folder, &doc.name)
    }
    fn put(&self, pdf: &Path, folder: &str, name: &str) -> Result<()> {
        self.cloud.replace(folder, name, std::fs::read(pdf)?)
    }
}

/// Adapts the native cloud client to rmreader's `BundleFetch` seam. Mirrors the
/// adapter in `crate::reader`.
struct CloudFetch<'a> {
    cloud: &'a Cloud,
}
impl BundleFetch for CloudFetch<'_> {
    fn fetch(&self, folder: &str, name: &str) -> Result<Option<PathBuf>> {
        self.cloud.fetch_bundle(folder, name)
    }
}

/// Run a single reactive job against the cloud.
#[allow(dead_code)] // wired by Task 8 (daemon).
pub fn run_job(cloud: &Cloud, cfg: &Config, job: &Job) -> Result<()> {
    match job.action {
        WatchAction::Digest => {
            let digest = cfg
                .digest
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("[[watch]] digest action but no [digest] config"))?;
            let backend = CloudBackend { cloud };
            let state_path = rmdigest::state::State::default_path();
            let opts = rmdigest::generate::Opts {
                dry_run: false,
                local_output: None,
            };
            let folder = job
                .doc
                .path
                .rsplit_once('/')
                .map(|(f, _)| f)
                .unwrap_or("")
                .to_string();
            let doc = CloudDoc {
                path: job.doc.path.clone(),
                name: job.doc.name.clone(),
                folder,
                version: None,
            };
            rmdigest::generate::run_one(digest, &backend, &state_path, &opts, &doc)
        }
        WatchAction::Readback => {
            let reader = cfg.reader.as_ref().ok_or_else(|| {
                anyhow::anyhow!("[[watch]] readback action but no [reader] config")
            })?;
            let bf = CloudFetch { cloud };
            let transport = UreqTransport;
            let folder = job
                .doc
                .path
                .rsplit_once('/')
                .map(|(f, _)| f)
                .unwrap_or("")
                .to_string();
            readback::sync_collection(
                &bf,
                &transport,
                &reader.readwise.token,
                &folder,
                &job.doc.name,
            )
            .map(|_| ())
        }
    }
}
