//! rmapi deploy backend: upload PDFs to the reMarkable cloud and refresh their
//! content non-destructively (preserving on-device handwriting).

use std::path::{Path, PathBuf};

use super::Deployer;

/// Runs a single `rmapi` subcommand. Abstracted so the deploy/refresh command
/// sequences are unit-testable without shelling out to the real binary.
pub trait RmapiRunner {
    /// Run `rmapi <args...>`; `args` never includes the binary name.
    fn run(&self, args: &[&str]) -> anyhow::Result<()>;
}

/// Uploads / refreshes a year of PDFs via an [`RmapiRunner`].
pub struct RmapiDeployer<R: RmapiRunner> {
    target_folder: String,
    runner: R,
}

impl<R: RmapiRunner> RmapiDeployer<R> {
    pub fn new(target_folder: String, runner: R) -> Self {
        Self {
            target_folder,
            runner,
        }
    }

    /// Build the `put` arg vector. `-ni` keeps rmapi non-interactive so it never
    /// blocks on (or clobbers its conf via) the pairing prompt.
    fn put_args<'a>(&'a self, pdf: &'a str, content_only: bool) -> Vec<&'a str> {
        let mut a = vec!["-ni", "put"];
        if content_only {
            a.push("--content-only");
        }
        a.push(pdf);
        a.push(self.target_folder.as_str());
        a
    }
}

impl<R: RmapiRunner> Deployer for RmapiDeployer<R> {
    fn deploy(&self, paths: &[PathBuf]) -> anyhow::Result<()> {
        // mkdir is idempotent: a pre-existing folder makes rmapi error, which we
        // ignore (established rmapi practice). A genuine auth/connectivity
        // failure surfaces on the first `put` below.
        let _ = self
            .runner
            .run(&["-ni", "mkdir", self.target_folder.as_str()]);
        for p in paths {
            self.runner.run(&self.put_args(path_str(p)?, false))?;
        }
        Ok(())
    }

    fn refresh(&self, paths: &[PathBuf]) -> anyhow::Result<()> {
        for p in paths {
            self.runner.run(&self.put_args(path_str(p)?, true))?;
        }
        Ok(())
    }
}

fn path_str(p: &Path) -> anyhow::Result<&str> {
    p.to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF-8 path: {}", p.display()))
}
