//! Deploy seam (Phase 2 fills in the rmapi backend).

pub mod local;
pub mod rmapi;

use std::path::PathBuf;

use crate::config::Config;

pub trait Deployer: std::fmt::Debug {
    fn deploy(&self, paths: &[PathBuf]) -> anyhow::Result<()>;
    fn refresh(&self, paths: &[PathBuf]) -> anyhow::Result<()>;
}

pub fn get_deployer(config: &Config) -> anyhow::Result<Box<dyn Deployer>> {
    match config.deploy.backend.as_str() {
        "none" => Ok(Box::new(local::LocalDeployer)),
        "rmapi" => {
            // Validate config before touching rmapi, so a misconfig fails the same
            // way regardless of whether rmapi is installed/paired.
            let base = config.deploy.base_folder.trim();
            if base.is_empty() {
                anyhow::bail!("deploy.base_folder is required for the rmapi backend");
            }
            // A year's PDFs go in a per-year subfolder under the base, e.g. base
            // "/rmbujo" + year 2026 -> "/rmbujo/2026".
            let runner = rmapi::ProcessRmapi::new()?;
            Ok(Box::new(rmapi::RmapiDeployer::new(
                rmapi::cloud_target(base, config.year),
                runner,
            )))
        }
        other => anyhow::bail!("unsupported deploy backend: {other:?}"),
    }
}
