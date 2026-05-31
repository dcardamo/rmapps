//! Local backend "none": PDFs are already on disk; deploy/refresh are no-ops.

use std::path::{Path, PathBuf};

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
    fn fetch(&self, _folder: &str, _name: &str) -> anyhow::Result<Option<PathBuf>> {
        Ok(None)
    }
    fn replace(&self, _pdf: &Path, _folder: &str) -> anyhow::Result<()> {
        Ok(())
    }
}
