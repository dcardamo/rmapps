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
            if config.deploy.target_folder.trim().is_empty() {
                anyhow::bail!("deploy.target_folder is required for the rmapi backend");
            }
            let runner = rmapi::ProcessRmapi::new()?;
            Ok(Box::new(rmapi::RmapiDeployer::new(
                config.deploy.target_folder.clone(),
                runner,
            )))
        }
        other => anyhow::bail!("unsupported deploy backend: {other:?}"),
    }
}
