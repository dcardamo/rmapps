//! `rmapps sync` — the config-driven trigger engine.
//!
//! Iterates `config.sync` and decides, per task, whether it is *due* this run:
//!
//! - `trigger = "schedule"` (default): due when `every` is unset, or when at least
//!   `every` has elapsed since this task last ran (tracked in persistent state).
//! - `trigger = "on-change"`: due when the cloud account's root *generation* has
//!   moved since the last sync (or on the very first run).
//!
//! Per-task last-run times and the last-seen generation are persisted to
//! `~/.local/state/rmapps/sync-state.json` so scheduling survives across invocations.
//! A single [`Cloud`] is built once and shared by every due task.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use chrono::Datelike;
use serde::{Deserialize, Serialize};

use crate::bujo::{self, BujoArgs};
use crate::cloud::Cloud;
use crate::config::Config;
use crate::digest::{self, DigestArgs};
use crate::reader;

/// Persistent sync state: the last-seen cloud root generation (for `on-change`)
/// and per-task last-run unix timestamps (for `schedule`).
#[derive(Debug, Default, Serialize, Deserialize)]
struct SyncState {
    last_generation: Option<i64>,
    #[serde(default)]
    last_run: BTreeMap<String, u64>,
}

/// `<state_dir>/rmapps/sync-state.json` (falling back to `~/.local/state`).
fn state_path() -> PathBuf {
    let base = dirs::state_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local").join("state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"));
    base.join("rmapps").join("sync-state.json")
}

/// Load persistent state, or start fresh (logging) if it is missing or corrupt —
/// a bad state file must never abort the run.
fn load_state() -> SyncState {
    let path = state_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(state) => state,
            Err(e) => {
                println!("[rmapps] sync: ignoring corrupt state {}: {e}", path.display());
                SyncState::default()
            }
        },
        Err(_) => SyncState::default(),
    }
}

/// Save state atomically (write to a temp sibling, then rename).
fn save_state(state: &SyncState) -> Result<()> {
    let path = state_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating state dir {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(state).context("serializing sync state")?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

/// Parse a duration string of the form `<N><unit>` where unit is `s|m|h|d`.
fn parse_every(s: &str) -> Result<Duration> {
    let s = s.trim();
    let (num, unit) = s.split_at(
        s.find(|c: char| !c.is_ascii_digit())
            .with_context(|| format!("invalid duration {s:?}: expected <N>s|m|h|d"))?,
    );
    if num.is_empty() {
        anyhow::bail!("invalid duration {s:?}: missing number");
    }
    let n: u64 = num
        .parse()
        .with_context(|| format!("invalid duration {s:?}: bad number"))?;
    let secs = match unit {
        "s" => n,
        "m" => n * 60,
        "h" => n * 60 * 60,
        "d" => n * 60 * 60 * 24,
        other => anyhow::bail!("invalid duration {s:?}: unknown unit {other:?} (use s|m|h|d)"),
    };
    Ok(Duration::from_secs(secs))
}

/// Current unix time in seconds.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn run(cfg: &Config) -> Result<()> {
    if cfg.sync.is_empty() {
        println!("No [[sync]] tasks configured.");
        return Ok(());
    }

    let mut state = load_state();
    let cloud = Cloud::from_stored()?;
    let cur_gen = cloud.current_generation()?;

    let now = now_secs();
    let mut ran = 0usize;
    let mut skipped = 0usize;

    for (i, task) in cfg.sync.iter().enumerate() {
        let key = format!("{}#{}", task.app, i);
        let trigger = task.trigger.as_deref().unwrap_or("schedule");

        let (due, reason) = match trigger {
            "on-change" => {
                // TODO: filter on-change by `watch` via snapshot diff — for now ANY
                // account-generation change (or the first run) triggers the task.
                let changed = cur_gen != state.last_generation;
                (changed, if changed { "generation changed" } else { "generation unchanged" })
            }
            "schedule" => match &task.every {
                None => (true, "no interval (always due)"),
                Some(every) => {
                    let interval = parse_every(every)?;
                    let last = state.last_run.get(&key).copied().unwrap_or(0);
                    let elapsed = now.saturating_sub(last);
                    let due = elapsed >= interval.as_secs();
                    (
                        due,
                        if due {
                            "interval elapsed"
                        } else {
                            "interval not elapsed"
                        },
                    )
                }
            },
            other => anyhow::bail!(
                "task {key}: unknown trigger {other:?} (expected \"schedule\" or \"on-change\")"
            ),
        };

        if !due {
            println!("[rmapps] sync: {key} skipped (not due: {reason})");
            skipped += 1;
            continue;
        }

        println!("[rmapps] sync: {key} running ({reason})");
        match task.app.as_str() {
            "bujo" => {
                let only_month = if task.month_window == Some(true) {
                    // chrono gives the current calendar month; the year is taken from
                    // the bujo config inside bujo::run.
                    Some(chrono::Local::now().month())
                } else {
                    None
                };
                bujo::run(BujoArgs::for_sync(only_month), cfg)?;
            }
            "reader" => reader::run(cfg)?,
            "digest" => digest::run(DigestArgs::default(), cfg)?,
            other => anyhow::bail!("task {key}: unknown app {other:?} (expected bujo|reader|digest)"),
        }
        state.last_run.insert(key, now);
        ran += 1;
    }

    state.last_generation = cur_gen;
    if let Err(e) = save_state(&state) {
        println!("[rmapps] sync: warning: could not save state: {e}");
    }

    println!("[rmapps] sync: done — {ran} ran, {skipped} skipped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_every_units() {
        assert_eq!(parse_every("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_every("30m").unwrap(), Duration::from_secs(30 * 60));
        assert_eq!(parse_every("12h").unwrap(), Duration::from_secs(12 * 3600));
        assert_eq!(parse_every("1d").unwrap(), Duration::from_secs(86400));
        // Surrounding whitespace is tolerated.
        assert_eq!(parse_every(" 5m ").unwrap(), Duration::from_secs(300));
    }

    #[test]
    fn parse_every_rejects_bad_input() {
        assert!(parse_every("").is_err());
        assert!(parse_every("10").is_err()); // no unit
        assert!(parse_every("h").is_err()); // no number
        assert!(parse_every("10x").is_err()); // unknown unit
        assert!(parse_every("abc").is_err());
    }
}
