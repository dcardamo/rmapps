//! Local backend "none": PDFs are already on disk; deploy/refresh are no-ops.

use std::path::PathBuf;

use super::Deployer;

pub struct LocalDeployer;

impl Deployer for LocalDeployer {
    fn deploy(&self, _paths: &[PathBuf]) -> anyhow::Result<()> {
        Ok(())
    }
    fn refresh(&self, _paths: &[PathBuf]) -> anyhow::Result<()> {
        Ok(())
    }
}
