//! `rmapps sync` — interval-driven trigger engine (stopgap).
//!
//! Iterates `config.sync` and runs each task that is *due* this run: a task with
//! no `every` is always due, otherwise it is due once `every` has elapsed since it
//! last ran. Per-task last-run timestamps are persisted to
//! `~/.local/state/rmapps/sync-state.json` so intervals survive across invocations.
//!
//! This is intentionally minimal: there is no reactivity here. Clock-time `at`
//! scheduling and change-driven triggers are owned by the `rmapps watch` daemon
//! (Task 8); this command just fires intervals when invoked (e.g. from cron).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use chrono::Datelike;
use serde::{Deserialize, Serialize};

use crate::bujo::{self, BujoArgs};
use crate::config::Config;
use crate::digest::{self, DigestArgs};
use crate::reader;

/// Persistent sync state: per-task last-run unix timestamps, keyed by task key.
#[derive(Debug, Default, Serialize, Deserialize)]
struct SyncState {
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

    let now = now_secs();
    let mut ran = 0usize;
    let mut skipped = 0usize;
    // Keys of tasks that failed this run, so we can summarize and exit non-zero
    // at the end — but only after every due task has had its turn.
    let mut failed: Vec<String> = Vec::new();

    for (i, task) in cfg.sync.iter().enumerate() {
        let key = format!("{}#{}", task.app, i);

        // Resolve whether this task is due. Trigger-resolution errors (bad
        // `every`, unknown trigger) are per-task failures too — record them and
        // move on rather than aborting the whole run.
        let (due, reason) = match resolve_due(task, &key, &state, now) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[rmapps] sync: task {key} failed: {e:#}");
                failed.push(key);
                continue;
            }
        };

        if !due {
            println!("[rmapps] sync: {key} skipped (not due: {reason})");
            skipped += 1;
            continue;
        }

        println!("[rmapps] sync: {key} running ({reason})");
        match run_task(task, &key, cfg) {
            Ok(()) => {
                // Only advance last_run on success, so a failed task retries on
                // its next fire instead of being marked as having run.
                state.last_run.insert(key, now);
                ran += 1;
            }
            Err(e) => {
                eprintln!("[rmapps] sync: task {key} failed: {e:#}");
                failed.push(key);
            }
        }
    }

    // Persist updated last-run timestamps regardless of task outcomes.
    if let Err(e) = save_state(&state) {
        println!("[rmapps] sync: warning: could not save state: {e}");
    }

    println!(
        "[rmapps] sync: {ran} ran, {} failed, {skipped} skipped",
        failed.len()
    );

    if !failed.is_empty() {
        anyhow::bail!("sync: {} task(s) failed: {}", failed.len(), failed.join(", "));
    }
    Ok(())
}

/// Decide whether `task` is due this run, returning `(due, reason)`.
///
/// NOTE: this is a stopgap. The `on-change` trigger and clock-time `at`
/// scheduling are handled by the reactive daemon (Task 8); here every task is
/// treated as a simple interval schedule.
fn resolve_due(
    task: &crate::config::SyncTask,
    key: &str,
    state: &SyncState,
    now: u64,
) -> Result<(bool, &'static str)> {
    Ok(match &task.every {
        None => (true, "no interval (always due)"),
        Some(every) => {
            let interval = parse_every(every)?;
            let last = state.last_run.get(key).copied().unwrap_or(0);
            let elapsed = now.saturating_sub(last);
            let due = elapsed >= interval.as_secs();
            (due, if due { "interval elapsed" } else { "interval not elapsed" })
        }
    })
}

/// Run a single due task's underlying app.
fn run_task(task: &crate::config::SyncTask, key: &str, cfg: &Config) -> Result<()> {
    match task.app.as_str() {
        "bujo" => {
            let only_month = if task.month_window == Some(true) {
                // chrono gives the current calendar month; the year is taken from
                // the bujo config inside bujo::run.
                Some(chrono::Local::now().month())
            } else {
                None
            };
            bujo::run(BujoArgs::for_sync(only_month), cfg)
        }
        "reader" => reader::run(cfg),
        "digest" => digest::run(DigestArgs::default(), cfg),
        other => anyhow::bail!("task {key}: unknown app {other:?} (expected bujo|reader|digest)"),
    }
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

    /// Mirrors `run`'s per-task loop: one failing task must not stop later ones,
    /// every failure is recorded, and the run reports a non-empty failure set.
    #[test]
    fn one_task_failure_does_not_stop_others() {
        let tasks: Vec<Result<()>> = vec![
            Ok(()),
            Err(anyhow::anyhow!("reader 401")),
            Ok(()),
            Err(anyhow::anyhow!("boom")),
        ];

        let mut ran = 0usize;
        let mut failed: Vec<usize> = Vec::new();
        for (i, t) in tasks.into_iter().enumerate() {
            match t {
                Ok(()) => ran += 1,
                Err(_) => failed.push(i),
            }
        }

        // Both successful tasks after the first failure still ran.
        assert_eq!(ran, 2);
        assert_eq!(failed, vec![1, 3]);
        assert!(!failed.is_empty(), "failures aggregate into a non-empty set");
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
