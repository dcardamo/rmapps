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
//!
//! Secrets stay out of this file: it can be nix-managed and committed safely.
//! Any string value may instead reference a dotvault-deployed 0600 file with
//! `@file:~/.config/secrets/NAME` (a leading `~/` expands to the home dir, and
//! the file's contents are trimmed) or an env var with `@env:NAME`. References
//! are resolved at load time, before deserialization.

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
    load_str(&text).with_context(|| format!("parsing rmapps config {}", path.display()))
}

/// Parse TOML `text`, resolve any `@file:`/`@env:` secret references, then
/// deserialize into [`Config`].
fn load_str(text: &str) -> Result<Config> {
    let mut value: toml::Value = toml::from_str(text).context("parsing config TOML")?;
    resolve_secret_refs(&mut value)?;
    let cfg: Config = value.try_into().context("deserializing resolved config")?;
    Ok(cfg)
}

/// Recursively replace every string in `value` that is a secret reference:
/// `@file:<path>` → trimmed file contents (leading `~/` expands to home), and
/// `@env:<VARNAME>` → the env var's value. Any other string is left as-is.
fn resolve_secret_refs(value: &mut toml::Value) -> Result<()> {
    match value {
        toml::Value::String(s) => {
            if let Some(rest) = s.strip_prefix("@file:") {
                let path = expand_tilde(rest);
                let contents = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading secret file {}", path.display()))?;
                *s = contents.trim().to_string();
            } else if let Some(var) = s.strip_prefix("@env:") {
                *s = std::env::var(var)
                    .with_context(|| format!("reading secret env var {var}"))?;
            }
        }
        toml::Value::Table(table) => {
            for (_k, v) in table.iter_mut() {
                resolve_secret_refs(v)?;
            }
        }
        toml::Value::Array(array) => {
            for v in array.iter_mut() {
                resolve_secret_refs(v)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Expand a leading `~/` to the user's home directory; other paths pass through.
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_file_ref_to_trimmed_contents() {
        let path = std::env::temp_dir().join("rmapps_secret_token.txt");
        std::fs::write(&path, "  super-secret-token\n").unwrap();
        let mut value =
            toml::Value::String(format!("@file:{}", path.display()));
        resolve_secret_refs(&mut value).unwrap();
        assert_eq!(value.as_str(), Some("super-secret-token"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn resolves_env_ref() {
        std::env::set_var("RMAPPS_TEST_SECRET", "env-value");
        let mut value = toml::Value::String("@env:RMAPPS_TEST_SECRET".to_string());
        resolve_secret_refs(&mut value).unwrap();
        assert_eq!(value.as_str(), Some("env-value"));
        std::env::remove_var("RMAPPS_TEST_SECRET");
    }

    #[test]
    fn leaves_plain_string_unchanged() {
        let mut value =
            toml::from_str::<toml::Value>(r#"token = "plain-value""#).unwrap();
        resolve_secret_refs(&mut value).unwrap();
        assert_eq!(value["token"].as_str(), Some("plain-value"));
    }

    #[test]
    fn missing_file_ref_errors() {
        let mut value = toml::Value::String(
            "@file:/no/such/rmapps/secret/file".to_string(),
        );
        let err = resolve_secret_refs(&mut value).unwrap_err();
        assert!(err.to_string().contains("/no/such/rmapps/secret/file"));
    }
}
