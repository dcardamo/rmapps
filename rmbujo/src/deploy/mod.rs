//! Deploy seam (Phase 2 fills in the rmapi backend).

pub mod local;
pub mod rmapi;

use std::path::PathBuf;

use crate::config::Config;

pub trait Deployer {
    fn deploy(&self, paths: &[PathBuf]) -> anyhow::Result<()>;
    fn refresh(&self, paths: &[PathBuf]) -> anyhow::Result<()>;
}

pub fn get_deployer(config: &Config) -> anyhow::Result<Box<dyn Deployer>> {
    match config.deploy.backend.as_str() {
        "none" => Ok(Box::new(local::LocalDeployer)),
        other => anyhow::bail!("unsupported deploy backend: {other:?}"),
    }
}
