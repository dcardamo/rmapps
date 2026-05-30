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
    /// Probe whether a document exists at the given cloud path. Default returns
    /// `false` so test recorders need no changes; `ProcessRmapi` overrides to
    /// shell out to `rmapi stat`. Used by `upsert` to decide between a fresh
    /// `put` and a content-only refresh.
    fn exists(&self, _path: &str) -> anyhow::Result<bool> {
        Ok(false)
    }
}

/// Uploads / refreshes a year of PDFs via an [`RmapiRunner`].
#[derive(Debug)]
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
        // rmapi's mkdir is not recursive, so create each ancestor folder in turn
        // (e.g. /rmbujo, then /rmbujo/2027). mkdir is idempotent: a pre-existing
        // folder makes rmapi error, which we ignore. A genuine auth/connectivity
        // failure surfaces on the first `put` below.
        for dir in folder_chain(&self.target_folder) {
            let _ = self.runner.run(&["-ni", "mkdir", dir.as_str()]);
        }
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

impl<R: RmapiRunner> RmapiDeployer<R> {
    /// Upload each PDF, choosing per-file between "create" (`put`) and "replace
    /// content" (`put --content-only`) so existing on-device handwriting is
    /// preserved when the doc already exists. Used by the `month` subcommand
    /// where the same monthly PDF is re-pushed hourly: the first run creates
    /// it; every subsequent run updates the background without touching ink.
    ///
    /// rmapi names the cloud doc after the PDF's file stem (no `.pdf`), so the
    /// existence probe checks `<target_folder>/<stem>`. mkdir-chain runs lazily
    /// on the first miss; if every doc already exists it never fires.
    pub fn upsert(&self, paths: &[PathBuf]) -> anyhow::Result<()> {
        let target = self.target_folder.as_str();
        let mut mkdir_done = false;
        for p in paths {
            let pdf = path_str(p)?;
            let stem = p
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| anyhow::anyhow!("no file stem: {}", p.display()))?;
            let probe = if target == "/" {
                format!("/{stem}")
            } else {
                format!("{}/{}", target.trim_end_matches('/'), stem)
            };
            if self.runner.exists(&probe)? {
                self.runner.run(&self.put_args(pdf, true))?;
            } else {
                if !mkdir_done {
                    for dir in folder_chain(target) {
                        let _ = self.runner.run(&["-ni", "mkdir", dir.as_str()]);
                    }
                    mkdir_done = true;
                }
                self.runner.run(&self.put_args(pdf, false))?;
            }
        }
        Ok(())
    }
}

fn path_str(p: &Path) -> anyhow::Result<&str> {
    p.to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF-8 path: {}", p.display()))
}

/// Resolve the cloud folder for a given year under a configured base folder.
/// The base is normalized to a single leading slash and no trailing slash, so
/// `"rmbujo"`, `"/rmbujo"`, and `"rmbujo/"` all map to `/rmbujo/<year>`. An empty
/// or `"/"` base yields `/<year>` at the cloud root.
pub fn cloud_target(base_folder: &str, year: i32) -> String {
    let base = base_folder.trim().trim_matches('/');
    if base.is_empty() {
        format!("/{year}")
    } else {
        format!("/{base}/{year}")
    }
}

/// The chain of folders to create for `path`, parents first, so a non-recursive
/// `mkdir` can build the whole hierarchy: "/rmbujo/2027" -> ["/rmbujo", "/rmbujo/2027"].
fn folder_chain(path: &str) -> Vec<String> {
    let mut acc = String::new();
    let mut chain = Vec::new();
    for part in path.split('/').filter(|s| !s.is_empty()) {
        acc.push('/');
        acc.push_str(part);
        chain.push(acc.clone());
    }
    chain
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

    fn exists(&self, path: &str) -> anyhow::Result<bool> {
        // `rmapi stat <path>` exits 0 when the doc exists, non-zero otherwise.
        // We treat any non-zero as "missing" (creates a fresh doc) — a transient
        // auth failure here would just cause the subsequent `put` to fail too,
        // surfacing the real error.
        self.attempt(&["-ni", "stat", path])
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
