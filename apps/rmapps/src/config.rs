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
    /// IANA timezone (e.g. "America/Halifax") for resolving clock-time `at` schedules.
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub sync: Vec<SyncTask>,
    /// Reactive `[[watch]]` rules: react to cloud changes under `path`.
    #[serde(default)]
    pub watch: Vec<WatchRule>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct SyncTask {
    /// Which app to run: "bujo" | "reader" | "digest".
    pub app: String,
    /// Interval schedule: `<N>s|m|h|d` (e.g. "12h", "1d"). Mutually exclusive with `at`.
    #[serde(default)]
    pub every: Option<String>,
    /// Clock-time schedule: list of `HH:MM` (24h). Mutually exclusive with `every`.
    #[serde(default)]
    pub at: Option<Vec<String>>,
    /// For `bujo`: when true, sync only the current calendar month.
    #[serde(default)]
    pub month_window: Option<bool>,
}

/// A reactive watch rule: when the cloud folder at `path` changes, run `action`
/// (after a quiet period of `debounce`).
#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct WatchRule {
    pub path: String,
    pub action: WatchAction,
    #[serde(default = "default_debounce")]
    pub debounce: String,
}

fn default_debounce() -> String {
    "30s".to_string()
}

/// The action a [`WatchRule`] fires. Unknown values are a load-time error.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum WatchAction {
    Digest,
    Readback,
}

impl Config {
    /// Validate the parsed config: timezone, `every`/`at` exclusivity, `HH:MM`
    /// times, watch paths and debounce durations. Cheap, pure, called by `load`.
    pub fn validate(&self) -> Result<()> {
        if let Some(tz) = &self.timezone {
            tz.parse::<chrono_tz::Tz>()
                .map_err(|_| anyhow::anyhow!("unknown timezone {tz:?}"))?;
        }
        for (i, t) in self.sync.iter().enumerate() {
            if t.every.is_some() && t.at.is_some() {
                anyhow::bail!("[[sync]] #{i} ({}): set either `every` or `at`, not both", t.app);
            }
            if t.every.is_none() && t.at.is_none() {
                anyhow::bail!(
                    "[[sync]] #{i} ({}): has neither `every` nor `at`, so it would never fire. \
                     A reactive (on-change) task belongs in a [[watch]] rule, not [[sync]].",
                    t.app
                );
            }
            if let Some(times) = &t.at {
                if times.is_empty() {
                    anyhow::bail!(
                        "[[sync]] #{i} ({}): `at` is empty; a task with no `every` and an empty `at` would never fire",
                        t.app
                    );
                }
                for s in times {
                    parse_hhmm(s).map_err(|e| anyhow::anyhow!("[[sync]] #{i}: {e}"))?;
                }
            }
        }
        for (i, r) in self.watch.iter().enumerate() {
            if r.path.trim().is_empty() {
                anyhow::bail!("[[watch]] #{i}: empty path");
            }
            let debounce = crate::watch::schedule::parse_duration(&r.debounce)
                .map_err(|e| anyhow::anyhow!("[[watch]] #{i} debounce: {e}"))?;
            if debounce.is_zero() {
                anyhow::bail!("[[watch]] #{i} debounce: must be > 0");
            }
        }
        Ok(())
    }
}

/// Parse "HH:MM" (24h) into (hour, minute).
pub fn parse_hhmm(s: &str) -> Result<(u32, u32)> {
    let (h, m) = s
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("invalid time {s:?}: expected HH:MM"))?;
    let h: u32 = h.parse().map_err(|_| anyhow::anyhow!("invalid hour in {s:?}"))?;
    let m: u32 = m.parse().map_err(|_| anyhow::anyhow!("invalid minute in {s:?}"))?;
    if h > 23 || m > 59 {
        anyhow::bail!("time out of range: {s:?}");
    }
    Ok((h, m))
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
    let cfg = load_str(&text)
        .with_context(|| format!("parsing rmapps config {}", path.display()))?;
    cfg.validate()
        .with_context(|| format!("validating rmapps config {}", path.display()))?;
    Ok(cfg)
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
    fn parses_watch_rules() {
        let cfg = load_str(
            r#"
            [[watch]]
            path = "/Books"
            action = "digest"
            debounce = "30s"

            [[watch]]
            path = "/Read/Library"
            action = "readback"
        "#,
        )
        .unwrap();
        assert_eq!(cfg.watch.len(), 2);
        assert_eq!(cfg.watch[0].path, "/Books");
        assert!(matches!(cfg.watch[0].action, WatchAction::Digest));
        assert!(matches!(cfg.watch[1].action, WatchAction::Readback));
        assert_eq!(cfg.watch[1].debounce, "30s"); // default applied
    }

    #[test]
    fn rejects_unknown_action() {
        let err = match load_str(
            r#"
            [[watch]]
            path = "/X"
            action = "frobnicate"
        "#,
        ) {
            Ok(_) => panic!("expected unknown action to be a load-time error"),
            Err(e) => e,
        };
        assert!(
            format!("{err:#}").contains("frobnicate")
                || format!("{err:#}").to_lowercase().contains("action")
                || format!("{err:#}").contains("variant")
        );
    }

    #[test]
    fn parses_at_times_and_timezone() {
        let cfg = load_str(
            r#"
            timezone = "America/Halifax"
            [[sync]]
            app = "bujo"
            at = ["06:00", "18:00"]
        "#,
        )
        .unwrap();
        assert_eq!(
            cfg.sync[0].at.as_ref().unwrap(),
            &vec!["06:00".to_string(), "18:00".to_string()]
        );
        cfg.validate().unwrap();
    }

    #[test]
    fn rejects_every_and_at_together() {
        let cfg = load_str(
            r#"
            [[sync]]
            app = "bujo"
            every = "12h"
            at = ["06:00"]
        "#,
        )
        .unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn rejects_bad_time_and_timezone() {
        let bad_time = load_str(
            r#"
            [[sync]]
            app = "bujo"
            at = ["6am"]
        "#,
        )
        .unwrap();
        assert!(bad_time.validate().is_err());

        let bad_tz = load_str(
            r#"
            timezone = "Mars/Olympus"
            [[sync]]
            app = "bujo"
            at = ["06:00"]
        "#,
        )
        .unwrap();
        assert!(bad_tz.validate().is_err());
    }

    #[test]
    fn rejects_empty_at_list() {
        let cfg = load_str(
            r#"
            [[sync]]
            app = "bujo"
            at = []
        "#,
        )
        .unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn rejects_sync_task_with_no_schedule() {
        // A [[sync]] task with neither `every` nor `at` can never fire. This must be
        // a load-time error, not a silent skip — it's the digest-on-change footgun:
        // digest was written as a [[sync]] task and the daemon silently never ran it.
        let cfg = load_str(
            r#"
            [[sync]]
            app = "digest"
        "#,
        )
        .unwrap();
        assert!(
            cfg.validate().is_err(),
            "a sync task with no every/at must be rejected, not silently skipped"
        );
    }

    #[test]
    fn rejects_unknown_sync_fields() {
        // `trigger`/`watch` are not [[sync]] fields. serde must reject them loudly so a
        // mistaken on-change sync task fails at load instead of being silently ignored
        // (the reactive form belongs in a [[watch]] rule). `every` is present here so
        // the ONLY reason to fail is the unknown fields.
        let err = load_str(
            r#"
            [[sync]]
            app = "digest"
            trigger = "on-change"
            watch = "/Books"
            every = "1h"
        "#,
        );
        assert!(err.is_err(), "unknown [[sync]] fields must be rejected at load");
    }

    #[test]
    fn rejects_zero_debounce() {
        let cfg = load_str(
            r#"
            [[watch]]
            path = "/Books"
            action = "digest"
            debounce = "0s"
        "#,
        )
        .unwrap();
        assert!(cfg.validate().is_err());
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
