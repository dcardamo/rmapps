//! Targeted reactive actions over the shared Cloud. Best-effort: errors are
//! returned to the caller, which logs and continues (never crashes the daemon).
use anyhow::Result;

use rmdigest::deploy::CloudDoc;
use rmreader::readback;
use rmreader::readwise::http::UreqTransport;

use crate::cloud::Cloud;
use crate::cloud_adapters::{CloudBackend, CloudFetch};
use crate::config::{Config, WatchAction};
use crate::watch::reconcile::Job;

/// The parent folder slash-path of a full doc path (`/Books/Title` -> `/Books`; root -> "").
fn folder_of(path: &str) -> String {
    path.rsplit_once('/').map(|(f, _)| f).unwrap_or("").to_string()
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
            let folder = folder_of(&job.doc.path);
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
            let folder = folder_of(&job.doc.path);
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
