//! rmapi deploy backend: upload PDFs to the reMarkable cloud and refresh their
//! content non-destructively (preserving on-device handwriting).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::Deployer;

/// Runs a single `rmapi` subcommand. Abstracted so the deploy/refresh command
/// sequences are unit-testable without shelling out to the real binary.
pub trait RmapiRunner: std::fmt::Debug {
    /// Run `rmapi <args...>`; `args` never includes the binary name.
    fn run(&self, args: &[&str]) -> anyhow::Result<()>;
}

/// Uploads / refreshes PDFs via an [`RmapiRunner`], using (pdf, folder) pairs.
#[derive(Debug)]
pub struct RmapiDeployer<R: RmapiRunner> {
    runner: R,
}

impl<R: RmapiRunner> RmapiDeployer<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    /// Build the `put` arg vector. `-ni` keeps rmapi non-interactive so it never
    /// blocks on (or clobbers its conf via) the pairing prompt.
    fn put_args<'a>(&self, pdf: &'a str, folder: &'a str, content_only: bool) -> Vec<&'a str> {
        let mut a = vec!["-ni", "put"];
        if content_only {
            a.push("--content-only");
        }
        a.push(pdf);
        a.push(folder);
        a
    }
}

impl<R: RmapiRunner> Deployer for RmapiDeployer<R> {
    fn deploy(&self, targets: &[(PathBuf, String)]) -> anyhow::Result<()> {
        // mkdir is idempotent: a pre-existing folder makes rmapi error, which we
        // ignore. A genuine auth/connectivity failure surfaces on the first `put`.
        for (pdf, folder) in targets {
            let _ = self.runner.run(&["-ni", "mkdir", folder.as_str()]);
            self.runner
                .run(&self.put_args(path_str(pdf)?, folder, false))?;
        }
        Ok(())
    }

    fn refresh(&self, targets: &[(PathBuf, String)]) -> anyhow::Result<()> {
        for (pdf, folder) in targets {
            self.runner
                .run(&self.put_args(path_str(pdf)?, folder, true))?;
        }
        Ok(())
    }
}

fn path_str(p: &Path) -> anyhow::Result<&str> {
    p.to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF-8 path: {}", p.display()))
}

/// Real runner: invokes the `rmapi` binary. Guards against rmapi's token-clobber
/// bug (it can zero its own conf on a transient failure, bricking later calls) by
/// snapshotting a good conf at construction and restoring it if a call empties it.
#[derive(Debug)]
pub struct ProcessRmapi {
    bin: PathBuf,
    conf_path: PathBuf,
    snapshot: Vec<u8>,
}

impl ProcessRmapi {
    /// Resolve the default rmapi binary (`rmapi` on PATH) and conf path.
    pub fn new() -> anyhow::Result<Self> {
        Self::with(PathBuf::from("rmapi"), default_conf_path())
    }

    /// Construct with explicit binary + conf paths (used by tests). Verifies both
    /// up front so misconfiguration fails before any upload begins.
    pub fn with(bin: PathBuf, conf_path: PathBuf) -> anyhow::Result<Self> {
        resolve_bin(&bin)?;
        let snapshot = std::fs::read(&conf_path).map_err(|_| {
            anyhow::anyhow!(
                "rmapi is not paired (no conf at {}). Pair once by running `rmapi` \
                 with a code from https://my.remarkable.com/device/desktop/connect",
                conf_path.display()
            )
        })?;
        if is_blank_conf(&snapshot) {
            anyhow::bail!(
                "rmapi conf at {} has blank tokens; re-pair by running `rmapi`",
                conf_path.display()
            );
        }
        Ok(Self {
            bin,
            conf_path,
            snapshot,
        })
    }

    fn attempt(&self, args: &[&str]) -> anyhow::Result<bool> {
        let status = Command::new(&self.bin)
            .args(args)
            .stdin(Stdio::null())
            .status()?;
        Ok(status.success())
    }

    fn conf_blanked(&self) -> bool {
        std::fs::read(&self.conf_path)
            .map(|b| is_blank_conf(&b))
            .unwrap_or(true)
    }
}

impl RmapiRunner for ProcessRmapi {
    fn run(&self, args: &[&str]) -> anyhow::Result<()> {
        if self.attempt(args)? {
            return Ok(());
        }
        // The call failed. If rmapi blanked its own conf, restore the snapshot
        // and retry once before giving up.
        if self.conf_blanked() {
            std::fs::write(&self.conf_path, &self.snapshot)?;
            if self.attempt(args)? {
                return Ok(());
            }
        }
        anyhow::bail!("rmapi {:?} failed", args);
    }
}

fn default_conf_path() -> PathBuf {
    // Mirror rmapi's own resolution: RMAPI_XDG_HOME, then XDG_CONFIG_HOME, then
    // ~/.config. (Confirm against the spike's recorded conf path.)
    if let Ok(p) = std::env::var("RMAPI_XDG_HOME") {
        return PathBuf::from(p).join("rmapi/rmapi.conf");
    }
    if let Ok(p) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(p).join("rmapi/rmapi.conf");
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/rmapi/rmapi.conf")
}

/// Verify the binary is runnable: an explicit path must be an existing file; a
/// bare name must be found on PATH.
fn resolve_bin(bin: &Path) -> anyhow::Result<()> {
    if bin.components().count() > 1 || bin.is_absolute() {
        if bin.is_file() {
            return Ok(());
        }
        anyhow::bail!("`{}` is not an executable file", bin.display());
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        if dir.join(bin).is_file() {
            return Ok(());
        }
    }
    anyhow::bail!(
        "`{}` not found on PATH; the flake dev shell provides it (run inside `nix develop`)",
        bin.display()
    )
}

/// A conf is "blank" unless it has a non-empty devicetoken AND usertoken.
/// rmapi's clobber bug writes empty-string values or truncates the file.
fn is_blank_conf(bytes: &[u8]) -> bool {
    let s = String::from_utf8_lossy(bytes);
    let token_ok = |key: &str| {
        s.lines().any(|l| {
            l.trim()
                .strip_prefix(key)
                .map(|rest| {
                    let v = rest.trim_start_matches(':').trim().trim_matches('"');
                    !v.is_empty()
                })
                .unwrap_or(false)
        })
    };
    !(token_ok("devicetoken") && token_ok("usertoken"))
}
