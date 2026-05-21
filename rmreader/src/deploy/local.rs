//! Local backend "none": PDFs are already on disk; deploy/refresh are no-ops.

use std::path::PathBuf;

use super::Deployer;

#[derive(Debug)]
pub struct LocalDeployer;

impl Deployer for LocalDeployer {
    fn deploy(&self, _targets: &[(PathBuf, String)]) -> anyhow::Result<()> {
        Ok(())
    }
    fn refresh(&self, _targets: &[(PathBuf, String)]) -> anyhow::Result<()> {
        Ok(())
    }
}
