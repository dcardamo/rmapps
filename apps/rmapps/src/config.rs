//! Unified rmapps config: one TOML at `~/.config/rmapps/config.toml`.
//!
//! Each app gets an optional section that deserializes straight into that crate's
//! own `Config` (they all already derive `Deserialize`). A subcommand errors
//! clearly if its section is missing. `[[sync]]` tasks drive the `sync`
//! orchestrator.
//!
//! Deploy transport is ALWAYS the native cloud client now — the per-app
//! `deploy.backend` field no longer selects a transport. It is only consulted as
//! a "generate only" switch: `backend == "none"` means build the PDFs but skip
//! the upload. The folder fields (`base_folder`, `library_folder`,
//! `feed_folder`) still say WHERE to deploy.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize, Default)]
pub struct Config {
    pub bujo: Option<rmbujo::config::Config>,
    pub reader: Option<rmreader::config::Config>,
    pub digest: Option<rmdigest::config::Config>,
    #[serde(default)]
    pub sync: Vec<SyncTask>,
}

#[derive(Deserialize, Clone)]
pub struct SyncTask {
    /// Which app to run: "bujo" | "reader" | "digest".
    pub app: String,
    /// "schedule" (default) | "on-change".
    #[serde(default)]
    pub trigger: Option<String>,
    /// Interval for `schedule` triggers: `<N>s|m|h|d` (e.g. "12h", "1d").
    #[serde(default)]
    pub every: Option<String>,
    /// Folder to watch for `on-change` triggers (filtering not yet implemented; see
    /// the TODO in `sync.rs`).
    #[serde(default)]
    #[allow(dead_code)]
    pub watch: Option<String>,
    /// For `bujo`: when true, sync only the current calendar month.
    #[serde(default)]
    pub month_window: Option<bool>,
}

/// Resolve the config path: `explicit` if given, else
/// `<config_dir>/rmapps/config.toml`.
pub fn config_path(explicit: Option<&Path>) -> Result<PathBuf> {
    match explicit {
        Some(p) => Ok(p.to_path_buf()),
        None => {
            let base = dirs::config_dir().context("could not resolve a config directory")?;
            Ok(base.join("rmapps").join("config.toml"))
        }
    }
}

/// A stable per-app cache/generation directory under
/// `<cache_dir>/rmapps/<app>/`, created if missing. rmbujo's ICS feed cache and
/// rmreader's article cache persist here across runs.
pub fn cache_dir(app: &str) -> Result<PathBuf> {
    let base = dirs::cache_dir().context("could not resolve a cache directory")?;
    let dir = base.join("rmapps").join(app);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating cache dir {}", dir.display()))?;
    Ok(dir)
}

/// Load the unified config from `explicit` or the default path.
pub fn load(explicit: Option<&Path>) -> Result<Config> {
    let path = config_path(explicit)?;
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading rmapps config {}", path.display()))?;
    let cfg: Config =
        toml::from_str(&text).with_context(|| format!("parsing rmapps config {}", path.display()))?;
    Ok(cfg)
}
