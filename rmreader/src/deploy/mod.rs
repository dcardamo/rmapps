//! Deploy seam. `none` is a no-op; `rmapi` uploads (pdf, folder) pairs.
pub mod local;
pub mod rmapi;

use std::path::PathBuf;

use crate::config::Config;

pub trait Deployer: std::fmt::Debug {
    fn deploy(&self, targets: &[(PathBuf, String)]) -> anyhow::Result<()>;
    fn refresh(&self, targets: &[(PathBuf, String)]) -> anyhow::Result<()>;
}

pub fn get_deployer(config: &Config) -> anyhow::Result<Box<dyn Deployer>> {
    match config.deploy.backend.as_str() {
        "none" => Ok(Box::new(local::LocalDeployer)),
        "rmapi" => Ok(Box::new(rmapi::RmapiDeployer::new(
            rmapi::ProcessRmapi::new()?,
        ))),
        other => anyhow::bail!("unsupported deploy backend: {other:?}"),
    }
}
