# Event-driven sync (`rmapps watch`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a resident `rmapps watch` daemon that runs scheduled tasks on clock-times/intervals and reacts to reMarkable-cloud changes via push, running targeted `digest`/`readback` actions on the specific document that changed.

**Architecture:** One unified daemon owns a shared async `rm_cloud::Client` and runs three subsystems in a `tokio::select!` loop — a scheduler (fires `[[sync]]` tasks), a notification source (websocket push, with a safety-net poll fallback), and a reactor (snapshot → pure diff → self-write filter → route to `[[watch]]` rules → debounce → targeted action). The interesting logic (diff, routing, debounce, scheduling) is pure and unit-tested; cloud I/O and the websocket sit behind thin seams with fakes.

**Tech Stack:** Rust, tokio, `rm-cloud` (native reMarkable client), `rmreader`/`rmdigest` action crates, `chrono`/`chrono-tz` for clock-time scheduling, `tokio-tungstenite` for the websocket.

**User Verification:** YES — the spec requires a one-time manual real-device check (Tier B): Dan highlights a real book on the Paper Pro, lets it sync, and confirms a digest appears beside it. Encoded as Task 12 with `requiresUserVerification: true`.

**Reference spec:** `docs/superpowers/specs/2026-06-01-event-driven-sync-design.md`

---

## File Structure

New (in `apps/rmapps/src/watch/`):
- `apps/rmapps/src/watch/mod.rs` — `rmapps watch` command entry, the `select!` loop, wiring.
- `apps/rmapps/src/watch/schedule.rs` — pure next-fire computation for `[[sync]]` (`every`/`at`/timezone/catch-up).
- `apps/rmapps/src/watch/reconcile.rs` — pure diff → `ChangedDoc`, folder-subtree routing, self-write filter.
- `apps/rmapps/src/watch/debounce.rs` — pure per-`(rule,doc)` coalescing with injected clock.
- `apps/rmapps/src/watch/actions.rs` — targeted `digest`/`readback` dispatch over the shared `Cloud`.
- `apps/rmapps/src/watch/notify.rs` — `NotificationSource` trait + safety-net poll source + fake.
- `apps/rmapps/src/watch/state.rs` — persistent baseline (`{id->hash}`) + `last_run` + `pending_jobs`.

Modified:
- `apps/rmapps/src/config.rs` — add `[[watch]]` rules; add `at`/`timezone` to `SyncTask`; drop `on-change`.
- `apps/rmapps/src/main.rs` — register the `Watch` subcommand.
- `apps/rmapps/src/sync.rs` — drop the `on-change` trigger (now the reactor's job); keep schedule semantics referenced by the daemon scheduler.
- `apps/rmapps/Cargo.toml` — add `chrono-tz`, `tokio-tungstenite`, `futures-util`.
- `crates/rmdigest/src/generate.rs` — expose a doc-scoped `run_one`.
- `crates/rm-cloud/src/config.rs` — add a `notifications` host + URL builder + env override.
- `crates/rm-cloud/src/client.rs` — add a `notifications_subscribe` websocket connector.

New tests:
- Unit tests colocated in each `watch/*.rs` module.
- `apps/rmapps/tests/watch_reactor.rs` — reactor integration against the fake cloud.
- `apps/rmapps/tests/watch_live.rs` — Tier A gated live e2e.

---

## Task 0: Config — `[[watch]]` rules, clock-time `[[sync]]`, drop `on-change`

**Goal:** Parse and validate the new config shape: `[[watch]]` reactive rules and `at`/`timezone` scheduled tasks; remove the `on-change` trigger and the unused `watch` field from `SyncTask`.

**Files:**
- Modify: `apps/rmapps/src/config.rs`
- Add dep: `apps/rmapps/Cargo.toml` (`chrono`, `chrono-tz`)

**Acceptance Criteria:**
- [ ] `[[watch]]` rules deserialize into `WatchRule { path, action, debounce }`.
- [ ] `action` is a closed enum (`digest` | `readback`); unknown values are a load-time error.
- [ ] `SyncTask` gains `at: Option<Vec<String>>` and the config gains top-level `timezone: Option<String>`; `every` and `at` are mutually exclusive (error if both set).
- [ ] `trigger`/`watch` fields removed from `SyncTask`.
- [ ] A `validate()` on `Config` rejects: both `every`+`at`, malformed `HH:MM`, unknown timezone, empty `path`, equal/empty debounce parse failure.

**Verify:** `cargo test -p rmapps config` → all config tests pass.

**Steps:**

- [ ] **Step 1: Add deps.** In `apps/rmapps/Cargo.toml` under `[dependencies]`:

```toml
chrono = { version = "0.4", default-features = false, features = ["clock"] }
chrono-tz = "0.9"
```

- [ ] **Step 2: Write failing tests** in `apps/rmapps/src/config.rs` `mod tests`:

```rust
#[test]
fn parses_watch_rules() {
    let cfg = load_str(r#"
        [[watch]]
        path = "/Books"
        action = "digest"
        debounce = "30s"

        [[watch]]
        path = "/Read/Library"
        action = "readback"
    "#).unwrap();
    assert_eq!(cfg.watch.len(), 2);
    assert_eq!(cfg.watch[0].path, "/Books");
    assert!(matches!(cfg.watch[0].action, WatchAction::Digest));
    assert!(matches!(cfg.watch[1].action, WatchAction::Readback));
}

#[test]
fn rejects_unknown_action() {
    let err = load_str(r#"
        [[watch]]
        path = "/X"
        action = "frobnicate"
    "#).unwrap_err();
    assert!(err.to_string().contains("frobnicate") || err.to_string().contains("action"));
}

#[test]
fn parses_at_times_and_timezone() {
    let cfg = load_str(r#"
        timezone = "America/Halifax"
        [[sync]]
        app = "bujo"
        at = ["06:00", "18:00"]
    "#).unwrap();
    assert_eq!(cfg.sync[0].at.as_ref().unwrap(), &vec!["06:00".to_string(), "18:00".to_string()]);
}

#[test]
fn rejects_every_and_at_together() {
    let cfg = load_str(r#"
        [[sync]]
        app = "bujo"
        every = "12h"
        at = ["06:00"]
    "#).unwrap();
    assert!(cfg.validate().is_err());
}

#[test]
fn rejects_bad_time_and_timezone() {
    let bad_time = load_str(r#"
        [[sync]]
        app = "bujo"
        at = ["6am"]
    "#).unwrap();
    assert!(bad_time.validate().is_err());

    let bad_tz = load_str(r#"
        timezone = "Mars/Olympus"
        [[sync]]
        app = "bujo"
        at = ["06:00"]
    "#).unwrap();
    assert!(bad_tz.validate().is_err());
}
```

- [ ] **Step 3: Run tests, expect failures** (`WatchAction`, `Config.watch`, `validate` undefined):
`cargo test -p rmapps config` → FAIL to compile.

- [ ] **Step 4: Implement.** In `apps/rmapps/src/config.rs`:

```rust
use serde::Deserialize;

#[derive(Deserialize, Default)]
pub struct Config {
    pub bujo: Option<rmbujo::config::Config>,
    pub reader: Option<rmreader::config::Config>,
    pub digest: Option<rmdigest::config::Config>,
    /// IANA timezone for `[[sync]]` `at` times (default: system local).
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub sync: Vec<SyncTask>,
    #[serde(default)]
    pub watch: Vec<WatchRule>,
}

#[derive(Deserialize, Clone)]
pub struct SyncTask {
    pub app: String,
    /// Interval form: `<N>s|m|h|d`. Mutually exclusive with `at`.
    #[serde(default)]
    pub every: Option<String>,
    /// Clock-time form: list of `HH:MM`. Mutually exclusive with `every`.
    #[serde(default)]
    pub at: Option<Vec<String>>,
    /// For `bujo`: when true, sync only the current calendar month.
    #[serde(default)]
    pub month_window: Option<bool>,
}

#[derive(Deserialize, Clone)]
pub struct WatchRule {
    /// Folder prefix to watch (matches docs at or under this path).
    pub path: String,
    /// What to run on a matching change.
    pub action: WatchAction,
    /// Per-(rule, doc) coalescing window; default 30s.
    #[serde(default = "default_debounce")]
    pub debounce: String,
}

fn default_debounce() -> String { "30s".to_string() }

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum WatchAction { Digest, Readback }

impl Config {
    /// Validate cross-field invariants. Call right after `load`.
    pub fn validate(&self) -> anyhow::Result<()> {
        // timezone resolves (or is unset).
        if let Some(tz) = &self.timezone {
            tz.parse::<chrono_tz::Tz>()
                .map_err(|_| anyhow::anyhow!("unknown timezone {tz:?}"))?;
        }
        for (i, t) in self.sync.iter().enumerate() {
            if t.every.is_some() && t.at.is_some() {
                anyhow::bail!("[[sync]] #{i} ({}): set either `every` or `at`, not both", t.app);
            }
            if let Some(times) = &t.at {
                for s in times {
                    parse_hhmm(s).map_err(|e| anyhow::anyhow!("[[sync]] #{i}: {e}"))?;
                }
            }
        }
        for (i, r) in self.watch.iter().enumerate() {
            if r.path.trim().is_empty() {
                anyhow::bail!("[[watch]] #{i}: empty path");
            }
            crate::watch::schedule::parse_duration(&r.debounce)
                .map_err(|e| anyhow::anyhow!("[[watch]] #{i} debounce: {e}"))?;
        }
        Ok(())
    }
}

/// Parse "HH:MM" (24h) into (hour, minute).
pub fn parse_hhmm(s: &str) -> anyhow::Result<(u32, u32)> {
    let (h, m) = s.split_once(':')
        .ok_or_else(|| anyhow::anyhow!("invalid time {s:?}: expected HH:MM"))?;
    let h: u32 = h.parse().map_err(|_| anyhow::anyhow!("invalid hour in {s:?}"))?;
    let m: u32 = m.parse().map_err(|_| anyhow::anyhow!("invalid minute in {s:?}"))?;
    if h > 23 || m > 59 { anyhow::bail!("time out of range: {s:?}"); }
    Ok((h, m))
}
```

> Note: `validate` references `crate::watch::schedule::parse_duration` (Task 1). If implementing Task 0 first, temporarily inline a duration parse or land Task 1's `parse_duration` first; the `.tasks.json` orders Task 1 to unblock this reference. Simplest: implement `parse_duration` in Task 1 before wiring it here.

- [ ] **Step 5: Wire `validate()` into `load`.** At the end of `config::load`, after deserialize, call `cfg.validate()?` before returning. Update the existing `Sync` command path in `main.rs` only if it relied on removed fields (it does not).

- [ ] **Step 6: Run tests, expect pass.** `cargo test -p rmapps config` → PASS.

- [ ] **Step 7: Commit.**

```bash
git add apps/rmapps/src/config.rs apps/rmapps/Cargo.toml Cargo.lock
git commit -m "feat(rmapps): config for [[watch]] rules + clock-time [[sync]]; drop on-change"
```

---

## Task 1: Scheduler — pure next-fire computation

**Goal:** A pure module computing when each `[[sync]]` task next fires, supporting `every`, `at` (in a timezone), and startup catch-up, with an injectable "now" so it is fully unit-testable.

**Files:**
- Create: `apps/rmapps/src/watch/mod.rs` (module declaration only for now), `apps/rmapps/src/watch/schedule.rs`
- Modify: `apps/rmapps/src/main.rs` (add `mod watch;`)

**Acceptance Criteria:**
- [ ] `parse_duration("30s"|"5m"|"12h"|"1d")` works (moved/shared from `sync.rs::parse_every`).
- [ ] `next_fire(task, tz, last_run, now)` returns the next `DateTime<Utc>` for both forms.
- [ ] For `at`, the next fire is the soonest listed time strictly after `now` in `tz`, rolling to tomorrow when all today's times have passed.
- [ ] `due_on_startup(task, tz, last_run, now)` returns true when an `at` time for today has passed and `last_run` is before it (catch-up), firing once regardless of how many were missed.

**Verify:** `cargo test -p rmapps schedule` → all pass.

**Steps:**

- [ ] **Step 1: Declare module.** In `main.rs` add `mod watch;`. Create `apps/rmapps/src/watch/mod.rs`:

```rust
//! `rmapps watch` — the resident daemon (scheduler + push reactor).
pub mod schedule;
```

- [ ] **Step 2: Write failing tests** in `apps/rmapps/src/watch/schedule.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use chrono_tz::America::Halifax;

    fn task_at(times: &[&str]) -> Sched {
        Sched::At(times.iter().map(|s| crate::config::parse_hhmm(s).unwrap()).collect())
    }

    #[test]
    fn parse_duration_units() {
        assert_eq!(parse_duration("30s").unwrap(), std::time::Duration::from_secs(30));
        assert_eq!(parse_duration("12h").unwrap(), std::time::Duration::from_secs(12 * 3600));
        assert!(parse_duration("10").is_err());
    }

    #[test]
    fn at_picks_next_time_today() {
        // now = 2026-06-01 05:00 Halifax; times 06:00, 18:00 → next is 06:00 today.
        let now = Halifax.with_ymd_and_hms(2026, 6, 1, 5, 0, 0).unwrap().with_timezone(&Utc);
        let next = next_fire(&task_at(&["06:00", "18:00"]), Halifax, None, now);
        let local = next.with_timezone(&Halifax);
        assert_eq!((local.hour(), local.minute()), (6, 0));
        assert_eq!(local.day(), 1);
    }

    #[test]
    fn at_rolls_to_tomorrow_after_last_time() {
        let now = Halifax.with_ymd_and_hms(2026, 6, 1, 19, 0, 0).unwrap().with_timezone(&Utc);
        let next = next_fire(&task_at(&["06:00", "18:00"]), Halifax, None, now);
        let local = next.with_timezone(&Halifax);
        assert_eq!((local.hour(), local.day()), (6, 2));
    }

    #[test]
    fn catchup_fires_once_for_missed_morning() {
        // Down across 06:00, started 07:00, never ran today → due.
        let now = Halifax.with_ymd_and_hms(2026, 6, 1, 7, 0, 0).unwrap().with_timezone(&Utc);
        assert!(due_on_startup(&task_at(&["06:00", "18:00"]), Halifax, None, now));
        // Already ran at 06:30 today → not due again until 18:00.
        let last = Halifax.with_ymd_and_hms(2026, 6, 1, 6, 30, 0).unwrap().with_timezone(&Utc);
        assert!(!due_on_startup(&task_at(&["06:00", "18:00"]), Halifax, Some(last), now));
    }
}
```

- [ ] **Step 3: Run, expect compile failure** (`Sched`, `parse_duration`, `next_fire`, `due_on_startup` undefined).

- [ ] **Step 4: Implement** `apps/rmapps/src/watch/schedule.rs`:

```rust
//! Pure scheduling math for `[[sync]]` tasks. No I/O; `now` is always injected.
use chrono::{DateTime, Datelike, Duration as ChDuration, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use std::time::Duration;

/// A task's schedule form, resolved from config.
pub enum Sched {
    Every(Duration),
    At(Vec<(u32, u32)>), // (hour, minute), sorted-or-not; we sort internally
}

/// Parse `<N>s|m|h|d`.
pub fn parse_duration(s: &str) -> anyhow::Result<Duration> {
    let s = s.trim();
    let idx = s.find(|c: char| !c.is_ascii_digit())
        .ok_or_else(|| anyhow::anyhow!("invalid duration {s:?}: expected <N>s|m|h|d"))?;
    let (num, unit) = s.split_at(idx);
    let n: u64 = num.parse().map_err(|_| anyhow::anyhow!("invalid duration {s:?}: bad number"))?;
    let secs = match unit {
        "s" => n, "m" => n * 60, "h" => n * 3600, "d" => n * 86400,
        other => anyhow::bail!("invalid duration {s:?}: unknown unit {other:?}"),
    };
    Ok(Duration::from_secs(secs))
}

/// Next fire instant (UTC) strictly after `now`.
pub fn next_fire(sched: &Sched, tz: Tz, last_run: Option<DateTime<Utc>>, now: DateTime<Utc>) -> DateTime<Utc> {
    match sched {
        Sched::Every(d) => {
            let base = last_run.unwrap_or(now);
            let cand = base + ChDuration::from_std(*d).unwrap_or(ChDuration::zero());
            if cand > now { cand } else { now }
        }
        Sched::At(times) => {
            let mut times = times.clone();
            times.sort_unstable();
            let local_now = now.with_timezone(&tz);
            for day in 0..=1 {
                let date = (local_now + ChDuration::days(day)).date_naive();
                for &(h, m) in &times {
                    if let Some(dt) = tz.from_local_datetime(
                        &date.and_hms_opt(h, m, 0).unwrap()).single()
                    {
                        let utc = dt.with_timezone(&Utc);
                        if utc > now { return utc; }
                    }
                }
            }
            now // unreachable in practice
        }
    }
}

/// True if an `at` time for *today* has already passed and the task has not run since it.
pub fn due_on_startup(sched: &Sched, tz: Tz, last_run: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    let Sched::At(times) = sched else { return false; };
    let local_now = now.with_timezone(&tz);
    let today = local_now.date_naive();
    // Most recent listed time at or before now today.
    let mut latest_passed: Option<DateTime<Utc>> = None;
    for &(h, m) in times {
        if let Some(dt) = tz.from_local_datetime(&today.and_hms_opt(h, m, 0).unwrap()).single() {
            let utc = dt.with_timezone(&Utc);
            if utc <= now { latest_passed = Some(latest_passed.map_or(utc, |p| p.max(utc))); }
        }
    }
    match (latest_passed, last_run) {
        (Some(passed), Some(last)) => last < passed,
        (Some(_), None) => true,
        _ => false,
    }
}
```

- [ ] **Step 5: Run, expect pass.** `cargo test -p rmapps schedule` → PASS.

- [ ] **Step 6: Commit.**

```bash
git add apps/rmapps/src/watch/ apps/rmapps/src/main.rs
git commit -m "feat(rmapps): pure scheduler (every/at/timezone/catch-up)"
```

---

## Task 2: Reconcile — pure diff → `ChangedDoc` + folder-subtree routing

**Goal:** Pure functions that, given a baseline `{id->hash}`, a new snapshot's `{id->hash}`, per-doc metadata, and resolved watch rules, produce the set of `(rule, doc)` jobs — independent of any network.

**Files:**
- Create: `apps/rmapps/src/watch/reconcile.rs`
- Modify: `apps/rmapps/src/watch/mod.rs` (`pub mod reconcile;`)

**Acceptance Criteria:**
- [ ] `diff_ids(baseline, current)` returns added+changed ids (removed are ignored — nothing to process).
- [ ] `ChangedDoc { id, name, parent, path }` is built from injected metadata.
- [ ] `route(changed, rules)` matches a doc to a rule when `doc.path == rule.path` or starts with `rule.path + "/"`.
- [ ] Multiple matching rules each yield a job; non-matching docs yield none.

**Verify:** `cargo test -p rmapps reconcile` → all pass.

**Steps:**

- [ ] **Step 1: Write failing tests** in `reconcile.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WatchAction;
    use std::collections::BTreeMap;

    fn rule(path: &str, action: WatchAction) -> ResolvedRule {
        ResolvedRule { path: path.into(), action, debounce: std::time::Duration::from_secs(30) }
    }

    #[test]
    fn diff_reports_added_and_changed_not_removed() {
        let base = BTreeMap::from([("a".into(), "h1".into()), ("b".into(), "h2".into())]);
        let cur  = BTreeMap::from([("a".into(), "h1".into()), ("b".into(), "h2b".into()), ("c".into(), "h3".into())]);
        let mut ids = diff_ids(&base, &cur);
        ids.sort();
        assert_eq!(ids, vec!["b".to_string(), "c".to_string()]);
    }

    #[test]
    fn routes_under_path_prefix() {
        let docs = vec![
            ChangedDoc { id: "1".into(), name: "Book".into(), parent: "p".into(), path: "/Books/Book".into() },
            ChangedDoc { id: "2".into(), name: "Art".into(),  parent: "q".into(), path: "/Read/Library/Art".into() },
            ChangedDoc { id: "3".into(), name: "Misc".into(), parent: "r".into(), path: "/Other/Misc".into() },
        ];
        let rules = vec![rule("/Books", WatchAction::Digest), rule("/Read/Library", WatchAction::Readback)];
        let jobs = route(&docs, &rules);
        assert_eq!(jobs.len(), 2);
        assert!(jobs.iter().any(|j| j.doc.id == "1" && j.action == WatchAction::Digest));
        assert!(jobs.iter().any(|j| j.doc.id == "2" && j.action == WatchAction::Readback));
    }

    #[test]
    fn prefix_does_not_match_sibling_with_shared_stem() {
        let docs = vec![ChangedDoc { id: "1".into(), name: "x".into(), parent: "p".into(), path: "/BooksClub/x".into() }];
        let rules = vec![rule("/Books", WatchAction::Digest)];
        assert!(route(&docs, &rules).is_empty());
    }
}
```

- [ ] **Step 2: Run, expect compile failure.**

- [ ] **Step 3: Implement** `reconcile.rs`:

```rust
//! Pure reconcile: snapshot-hash diff and folder-prefix routing. No I/O.
use crate::config::WatchAction;
use std::collections::BTreeMap;
use std::time::Duration;

/// A watch rule with its debounce parsed. (Path is resolved by prefix string match;
/// folder-id resolution happens in the impure shell that builds `ChangedDoc.path`.)
#[derive(Clone)]
pub struct ResolvedRule {
    pub path: String,
    pub action: WatchAction,
    pub debounce: Duration,
}

/// A changed document with enough identity to route and act on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedDoc {
    pub id: String,
    pub name: String,
    pub parent: String, // parent folder id
    pub path: String,   // full slash path, e.g. /Books/Author/Title
}

/// A unit of reactive work.
#[derive(Clone, Debug)]
pub struct Job {
    pub action: WatchAction,
    pub rule_path: String,
    pub debounce: Duration,
    pub doc: ChangedDoc,
}

/// Ids present-and-changed or newly-added between baseline and current hash maps.
/// Removed ids are intentionally excluded — there is nothing to process for a gone doc.
pub fn diff_ids(baseline: &BTreeMap<String, String>, current: &BTreeMap<String, String>) -> Vec<String> {
    current.iter()
        .filter(|(id, h)| baseline.get(*id).map_or(true, |b| b != *h))
        .map(|(id, _)| id.clone())
        .collect()
}

/// Match each changed doc against rule path-prefixes; one job per (rule, doc) match.
pub fn route(docs: &[ChangedDoc], rules: &[ResolvedRule]) -> Vec<Job> {
    let mut jobs = Vec::new();
    for d in docs {
        for r in rules {
            let under = d.path == r.path || d.path.starts_with(&format!("{}/", r.path));
            if under {
                jobs.push(Job {
                    action: r.action,
                    rule_path: r.path.clone(),
                    debounce: r.debounce,
                    doc: d.clone(),
                });
            }
        }
    }
    jobs
}
```

- [ ] **Step 4: Run, expect pass.** `cargo test -p rmapps reconcile` → PASS.

- [ ] **Step 5: Commit.**

```bash
git add apps/rmapps/src/watch/reconcile.rs apps/rmapps/src/watch/mod.rs
git commit -m "feat(rmapps): pure reconcile diff + folder-prefix routing"
```

---

## Task 3: Self-write filter

**Goal:** Drop changed docs that the daemon itself produced — digest/annotated outputs (by visible-name suffix) and any doc carrying the `rmCloudKey` app-key marker — so a deploy never re-triggers a reaction.

**Files:**
- Modify: `apps/rmapps/src/watch/reconcile.rs`

**Acceptance Criteria:**
- [ ] `is_self_write(name, has_app_key, suffixes)` returns true for a name ending in any suffix, or when `has_app_key` is true.
- [ ] `filter_self_writes(docs, suffixes, app_key_ids)` removes matching docs.

**Verify:** `cargo test -p rmapps reconcile::tests::self_write` → pass.

**Steps:**

- [ ] **Step 1: Failing tests** appended to `reconcile.rs` tests:

```rust
#[test]
fn self_write_filtered_by_suffix_and_appkey() {
    let suffixes = vec![" — Digest".to_string(), " — Annotated".to_string()];
    let docs = vec![
        ChangedDoc { id: "1".into(), name: "Book".into(), parent: "p".into(), path: "/Books/Book".into() },
        ChangedDoc { id: "2".into(), name: "Book — Digest".into(), parent: "p".into(), path: "/Books/Book — Digest".into() },
        ChangedDoc { id: "3".into(), name: "Reader Index".into(), parent: "q".into(), path: "/Read/Reader Index".into() },
    ];
    let app_key_ids: std::collections::BTreeSet<String> = ["3".to_string()].into_iter().collect();
    let kept = filter_self_writes(docs, &suffixes, &app_key_ids);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].id, "1");
}
```

- [ ] **Step 2: Run, expect failure.**

- [ ] **Step 3: Implement** in `reconcile.rs`:

```rust
use std::collections::BTreeSet;

/// True if a doc looks like daemon-generated output.
pub fn is_self_write(name: &str, has_app_key: bool, suffixes: &[String]) -> bool {
    has_app_key || suffixes.iter().any(|s| name.ends_with(s.as_str()))
}

/// Remove daemon-generated docs (`app_key_ids` carry the `rmCloudKey` marker).
pub fn filter_self_writes(docs: Vec<ChangedDoc>, suffixes: &[String], app_key_ids: &BTreeSet<String>) -> Vec<ChangedDoc> {
    docs.into_iter()
        .filter(|d| !is_self_write(&d.name, app_key_ids.contains(&d.id), suffixes))
        .collect()
}
```

- [ ] **Step 4: Run, expect pass.** `cargo test -p rmapps reconcile` → PASS.

- [ ] **Step 5: Commit.**

```bash
git add apps/rmapps/src/watch/reconcile.rs
git commit -m "feat(rmapps): self-write filter (suffix + rmCloudKey) to prevent reaction loops"
```

---

## Task 4: Debounce — pure per-`(rule,doc)` coalescing

**Goal:** Coalesce repeated jobs for the same `(rule_path, doc.id)` within the rule's debounce window, with an injected clock so it is deterministic in tests.

**Files:**
- Create: `apps/rmapps/src/watch/debounce.rs`
- Modify: `apps/rmapps/src/watch/mod.rs` (`pub mod debounce;`)

**Acceptance Criteria:**
- [ ] `Debouncer::offer(job, now)` records/refreshes a pending job keyed by `(rule_path, doc.id)`.
- [ ] `Debouncer::ready(now)` returns and removes jobs whose window has elapsed since the last `offer`.
- [ ] A second `offer` for the same key before the window elapses resets the timer and keeps the latest doc state.

**Verify:** `cargo test -p rmapps debounce` → pass.

**Steps:**

- [ ] **Step 1: Failing tests** in `debounce.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WatchAction;
    use crate::watch::reconcile::{ChangedDoc, Job};
    use std::time::{Duration, Instant};

    fn job(id: &str) -> Job {
        Job { action: WatchAction::Digest, rule_path: "/Books".into(), debounce: Duration::from_secs(30),
              doc: ChangedDoc { id: id.into(), name: "B".into(), parent: "p".into(), path: format!("/Books/{id}") } }
    }

    #[test]
    fn fires_after_window() {
        let t0 = Instant::now();
        let mut d = Debouncer::default();
        d.offer(job("1"), t0);
        assert!(d.ready(t0).is_empty());
        assert!(d.ready(t0 + Duration::from_secs(29)).is_empty());
        let ready = d.ready(t0 + Duration::from_secs(31));
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].doc.id, "1");
    }

    #[test]
    fn repeated_offer_resets_window_and_collapses() {
        let t0 = Instant::now();
        let mut d = Debouncer::default();
        d.offer(job("1"), t0);
        d.offer(job("1"), t0 + Duration::from_secs(20)); // reset
        assert!(d.ready(t0 + Duration::from_secs(31)).is_empty()); // window now ends at t0+50
        assert_eq!(d.ready(t0 + Duration::from_secs(51)).len(), 1);
    }
}
```

- [ ] **Step 2: Run, expect failure.**

- [ ] **Step 3: Implement** `debounce.rs`:

```rust
//! Pure debounce keyed by (rule_path, doc id). Clock is injected (`Instant`).
use crate::watch::reconcile::Job;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Default)]
pub struct Debouncer {
    pending: HashMap<(String, String), (Job, Instant)>, // key -> (latest job, fire-at)
}

impl Debouncer {
    /// Record/refresh a job; its fire time becomes `now + job.debounce`.
    pub fn offer(&mut self, job: Job, now: Instant) {
        let key = (job.rule_path.clone(), job.doc.id.clone());
        let fire_at = now + job.debounce;
        self.pending.insert(key, (job, fire_at));
    }

    /// Remove and return all jobs whose window has elapsed.
    pub fn ready(&mut self, now: Instant) -> Vec<Job> {
        let ready_keys: Vec<_> = self.pending.iter()
            .filter(|(_, (_, fire_at))| *fire_at <= now)
            .map(|(k, _)| k.clone())
            .collect();
        ready_keys.into_iter().filter_map(|k| self.pending.remove(&k).map(|(j, _)| j)).collect()
    }

    /// Earliest pending fire time, for computing the next select! sleep.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.pending.values().map(|(_, t)| *t).min()
    }
}
```

- [ ] **Step 4: Run, expect pass.** `cargo test -p rmapps debounce` → PASS.

- [ ] **Step 5: Commit.**

```bash
git add apps/rmapps/src/watch/debounce.rs apps/rmapps/src/watch/mod.rs
git commit -m "feat(rmapps): pure debounce for reactive jobs"
```

---

## Task 5: Targeted actions — `digest run_one` + reactive dispatch

**Goal:** Expose a doc-scoped digest entrypoint and a thin reactive-action dispatcher that runs `digest`/`readback` on a single `ChangedDoc` using the existing `Cloud` seams.

**Files:**
- Modify: `crates/rmdigest/src/generate.rs` (expose `run_one`)
- Create: `apps/rmapps/src/watch/actions.rs`
- Modify: `apps/rmapps/src/watch/mod.rs` (`pub mod actions;`)

**Acceptance Criteria:**
- [ ] `rmdigest::generate::run_one(cfg, backend, state_path, opts, doc)` runs the existing `process_doc` for one `CloudDoc` and is unit-tested with the existing fake backend in `generate.rs` tests.
- [ ] `actions::run_job(&Cloud, cfg, &Job)` maps `Digest`→`run_one` and `Readback`→`readback::sync_collection` for the changed doc's `(folder, name)`, returning `Result<()>`, logging and swallowing per-action errors at the call site (daemon stays alive).

**Verify:** `cargo test -p rmdigest run_one` and `cargo build -p rmapps` → pass.

**Steps:**

- [ ] **Step 1: Expose `run_one` in `crates/rmdigest/src/generate.rs`.** Add below `run`:

```rust
/// Run the digest pipeline for exactly one document (reactive, targeted path).
/// Reuses the same `state_path` as [`run`], so reactive and scheduled digests
/// never double-process the same doc.
pub fn run_one(
    cfg: &Config,
    backend: &dyn Backend,
    state_path: &Path,
    opts: &Opts,
    doc: &CloudDoc,
) -> anyhow::Result<()> {
    let mut state = State::load(state_path)?;
    process_doc(cfg, backend, doc, &mut state, state_path, opts)
}
```

- [ ] **Step 2: Failing test in `generate.rs` tests** (mirror the existing fake `Backend` there):

```rust
#[test]
fn run_one_processes_single_doc() {
    // Use the existing test fake Backend in this module (see `impl Backend for ...`).
    // Assert run_one fetches + (dry-run) generates for the given CloudDoc without error.
    // (Fill using the same fixture the existing run() test uses.)
}
```

> The existing `generate.rs` tests already define a fake `Backend` (around line 187). Reuse it: construct a `CloudDoc { path, name, folder, version: None }` pointing at the fixture the existing test serves, call `run_one(&cfg, &fake, &state_path, &Opts{dry_run:true, local_output:None}, &doc)`, assert `Ok`.

- [ ] **Step 3: Run, expect failure, then pass after Step 1.** `cargo test -p rmdigest run_one`.

- [ ] **Step 4: Implement** `apps/rmapps/src/watch/actions.rs`:

```rust
//! Targeted reactive actions over the shared Cloud. Best-effort: errors are
//! returned to the caller, which logs and continues (never crashes the daemon).
use anyhow::Result;
use std::path::{Path, PathBuf};

use rmdigest::deploy::{Backend, CloudDoc};
use rmreader::deploy::BundleFetch;
use rmreader::readback;
use rmreader::readwise::http::UreqTransport;

use crate::cloud::Cloud;
use crate::config::{Config, WatchAction};
use crate::watch::reconcile::Job;

struct CloudBackend<'a> { cloud: &'a Cloud }
impl Backend for CloudBackend<'_> {
    fn list(&self, root: &str, ex: &[String]) -> Result<Vec<CloudDoc>> {
        Ok(self.cloud.list_recursive(root, ex)?.into_iter()
            .map(|d| CloudDoc { path: d.path, name: d.name, folder: d.folder, version: None }).collect())
    }
    fn fetch(&self, doc: &CloudDoc) -> Result<Option<PathBuf>> { self.cloud.fetch_bundle(&doc.folder, &doc.name) }
    fn put(&self, pdf: &Path, folder: &str, name: &str) -> Result<()> { self.cloud.replace(folder, name, std::fs::read(pdf)?) }
}

struct CloudFetch<'a> { cloud: &'a Cloud }
impl BundleFetch for CloudFetch<'_> {
    fn fetch(&self, folder: &str, name: &str) -> Result<Option<PathBuf>> { self.cloud.fetch_bundle(folder, name) }
}

/// Run a single reactive job against the cloud.
pub fn run_job(cloud: &Cloud, cfg: &Config, job: &Job) -> Result<()> {
    match job.action {
        WatchAction::Digest => {
            let digest = cfg.digest.as_ref()
                .ok_or_else(|| anyhow::anyhow!("[[watch]] digest action but no [digest] config"))?;
            let backend = CloudBackend { cloud };
            let state_path = rmdigest::state::State::default_path();
            let opts = rmdigest::generate::Opts { dry_run: false, local_output: None };
            let folder = job.doc.path.rsplit_once('/').map(|(f, _)| f).unwrap_or("").to_string();
            let doc = CloudDoc { path: job.doc.path.clone(), name: job.doc.name.clone(), folder, version: None };
            rmdigest::generate::run_one(digest, &backend, &state_path, &opts, &doc)
        }
        WatchAction::Readback => {
            let reader = cfg.reader.as_ref()
                .ok_or_else(|| anyhow::anyhow!("[[watch]] readback action but no [reader] config"))?;
            let bf = CloudFetch { cloud };
            let transport = UreqTransport;
            let folder = job.doc.path.rsplit_once('/').map(|(f, _)| f).unwrap_or("").to_string();
            readback::sync_collection(&bf, &transport, &reader.readwise.token, &folder, &job.doc.name)
                .map(|_| ())
        }
    }
}
```

- [ ] **Step 5: Build.** `cargo build -p rmapps` → success.

- [ ] **Step 6: Commit.**

```bash
git add crates/rmdigest/src/generate.rs apps/rmapps/src/watch/actions.rs apps/rmapps/src/watch/mod.rs
git commit -m "feat: doc-scoped digest run_one + reactive action dispatch"
```

---

## Task 6: NotificationSource trait + safety-net poll source + fake

**Goal:** Define the wakeup abstraction and ship a poll-only source (the day-one mode and fallback) plus a controllable fake for reactor tests.

**Files:**
- Create: `apps/rmapps/src/watch/notify.rs`
- Modify: `apps/rmapps/src/watch/mod.rs` (`pub mod notify;`)

**Acceptance Criteria:**
- [ ] `Wakeup` is a unit signal (carries no payload).
- [ ] `trait NotificationSource { async fn next_wakeup(&mut self) -> Wakeup; }`.
- [ ] `PollSource::new(interval)` yields a `Wakeup` every `interval` (uses `tokio::time::interval`).
- [ ] `FakeSource` yields wakeups pushed onto an `mpsc` channel — used by Task 9's reactor test.

**Verify:** `cargo test -p rmapps notify` → pass.

**Steps:**

- [ ] **Step 1: Failing test** in `notify.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn fake_source_delivers_pushed_wakeups() {
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let mut src = FakeSource::new(rx);
        tx.send(Wakeup).await.unwrap();
        // next_wakeup resolves once a wakeup is pushed.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), src.next_wakeup()).await.unwrap();
    }
}
```

- [ ] **Step 2: Run, expect failure.**

- [ ] **Step 3: Implement** `notify.rs`:

```rust
//! Wakeup sources for the reactor. A Wakeup is only a signal; the diff is the
//! source of truth for what changed, so all sources are interchangeable.
use async_trait::async_trait;
use std::time::Duration;
use tokio::sync::mpsc::Receiver;

#[derive(Clone, Copy, Debug)]
pub struct Wakeup;

#[async_trait]
pub trait NotificationSource: Send {
    /// Resolve when the account may have changed. Never returns an error: a source
    /// that dies should reconnect internally and keep yielding (poll fallback covers gaps).
    async fn next_wakeup(&mut self) -> Wakeup;
}

/// Periodic safety-net source (also the `--poll-only` mode).
pub struct PollSource { interval: tokio::time::Interval }
impl PollSource {
    pub fn new(period: Duration) -> Self {
        let mut interval = tokio::time::interval(period);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        Self { interval }
    }
}
#[async_trait]
impl NotificationSource for PollSource {
    async fn next_wakeup(&mut self) -> Wakeup { self.interval.tick().await; Wakeup }
}

/// Test source driven by a channel.
pub struct FakeSource { rx: Receiver<Wakeup> }
impl FakeSource { pub fn new(rx: Receiver<Wakeup>) -> Self { Self { rx } } }
#[async_trait]
impl NotificationSource for FakeSource {
    async fn next_wakeup(&mut self) -> Wakeup { self.rx.recv().await.unwrap_or(Wakeup) }
}
```

- [ ] **Step 4: Add deps** to `apps/rmapps/Cargo.toml`: `async-trait = "0.1"` (tokio already present via `cloud.rs`; ensure `features = ["rt-multi-thread","macros","time","sync"]`).

- [ ] **Step 5: Run, expect pass.** `cargo test -p rmapps notify` → PASS.

- [ ] **Step 6: Commit.**

```bash
git add apps/rmapps/src/watch/notify.rs apps/rmapps/src/watch/mod.rs apps/rmapps/Cargo.toml Cargo.lock
git commit -m "feat(rmapps): NotificationSource trait + poll + fake sources"
```

---

## Task 7: Websocket NotificationSource (discovery spike + impl)

**Goal:** Implement a real push source against reMarkable's notification websocket, behind the `NotificationSource` trait, with exponential-backoff reconnect. Because the protocol is undocumented, begin with a bounded discovery spike; if it cannot be confirmed, the feature still ships in poll-only mode.

**Files:**
- Modify: `crates/rm-cloud/src/config.rs` (notifications host + URL builder + env override)
- Modify: `crates/rm-cloud/src/client.rs` (`notifications_subscribe` connector)
- Create: `apps/rmapps/src/watch/notify_ws.rs` (the `WsSource` impl)
- Modify: `apps/rmapps/src/watch/notify.rs` (`pub use` / re-export)

**Acceptance Criteria:**
- [ ] `Config` gains `notifications: String` defaulting to the production notifications host, overridable via `RM_CLOUD_HOST` (same single-host override path used today).
- [ ] `Client::notifications_subscribe()` returns a connected websocket stream (authenticated with the user token, refreshed on 401), or an error.
- [ ] `WsSource` implements `NotificationSource`: yields a `Wakeup` per server message; on disconnect, reconnects with exponential backoff (capped), never returning from `next_wakeup` with an error.
- [ ] A `#[ignore]` live test confirms a wakeup arrives after a second-connection commit (folded into Task 10).

**Steps:**

- [ ] **Step 1: Discovery spike (timeboxed).** Confirm the notifications endpoint and auth. Starting points to verify against the live account and open implementations:
  - reMarkable v3 sync commits already carry a `broadcast` flag (`client.rs:199`) — the server broadcasts to subscribers when true. Identify the subscriber endpoint.
  - Cross-reference `rmfakecloud` (its notifications websocket is served at a `/notifications/ws/json/1`-style path) and `rmapi` for the production host (historically discovered via the `service-manager` discovery endpoint).
  - Capture: the exact wss URL, required headers (likely `Authorization: Bearer <user-token>`), and the message envelope.
  Record findings as a comment block at the top of `notify_ws.rs`. **If the endpoint cannot be confirmed in the timebox, stop here:** ship Tasks 0–6, 8–12 with `PollSource` only and leave this task open. The daemon is fully functional in poll mode.

- [ ] **Step 2: Add the notifications host** in `crates/rm-cloud/src/config.rs`:

```rust
pub struct Config {
    pub auth: String,
    pub sync: String,
    pub notifications: String, // wss base for the notification stream
}
// in production(): notifications: "wss://<confirmed-host>".into(),
// in from_env()/single_host(): set notifications = the same override base.
// Add: pub(crate) fn notifications_ws(&self) -> String { format!("{}/<confirmed-path>", self.notifications) }
```

Update `production()`, `from_env()`, `single_host()`, and the existing config tests to set/assert the new field.

- [ ] **Step 3: Add the connector** in `crates/rm-cloud/src/client.rs` using `tokio-tungstenite`:

```rust
/// Connect to the notification websocket, authenticated with the user token.
/// Returns the message stream; caller maps each message to a wakeup.
pub async fn notifications_subscribe(&self)
    -> Result<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>
{
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::header::AUTHORIZATION};
    let token = self.user_token().await?;
    let mut req = self.config.notifications_ws().into_client_request()
        .map_err(|e| Error::Http(format!("ws request: {e}")))?;
    req.headers_mut().insert(AUTHORIZATION, format!("Bearer {token}").parse().unwrap());
    let (stream, _resp) = tokio_tungstenite::connect_async(req).await
        .map_err(|e| Error::Http(format!("ws connect: {e}")))?;
    Ok(stream)
}
```

Add deps to `crates/rm-cloud/Cargo.toml`: `tokio-tungstenite = { version = "0.23", features = ["rustls-tls-native-roots"] }`, `futures-util = "0.3"`.

- [ ] **Step 4: Implement `WsSource`** in `apps/rmapps/src/watch/notify_ws.rs`:

```rust
//! Real push source: subscribes to the reMarkable notification websocket.
//! Discovery notes: <fill from Step 1>.
use async_trait::async_trait;
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Duration;

use rm_cloud::Client;
use crate::watch::notify::{NotificationSource, Wakeup};

pub struct WsSource {
    client: Arc<Client>,
    stream: Option<rm_cloud::NotifyStream>, // type alias re-exported from rm-cloud
    backoff: Duration,
}

impl WsSource {
    pub fn new(client: Arc<Client>) -> Self {
        Self { client, stream: None, backoff: Duration::from_secs(1) }
    }
    async fn ensure_connected(&mut self) {
        while self.stream.is_none() {
            match self.client.notifications_subscribe().await {
                Ok(s) => { self.stream = Some(s); self.backoff = Duration::from_secs(1); }
                Err(e) => {
                    eprintln!("[rmapps] watch: ws connect failed: {e}; retrying in {:?}", self.backoff);
                    tokio::time::sleep(self.backoff).await;
                    self.backoff = (self.backoff * 2).min(Duration::from_secs(60));
                }
            }
        }
    }
}

#[async_trait]
impl NotificationSource for WsSource {
    async fn next_wakeup(&mut self) -> Wakeup {
        loop {
            self.ensure_connected().await;
            match self.stream.as_mut().unwrap().next().await {
                Some(Ok(_msg)) => return Wakeup, // any server message = "account may have changed"
                Some(Err(e)) => { eprintln!("[rmapps] watch: ws error: {e}; reconnecting"); self.stream = None; }
                None => { self.stream = None; } // closed; reconnect
            }
        }
    }
}
```

> `rm_cloud::NotifyStream` is a type alias the connector returns; export it from `rm-cloud` `lib.rs`. Keep `WsSource` resilient: `next_wakeup` never returns an error — it reconnects internally, and the daemon's `PollSource` runs concurrently as the backstop.

- [ ] **Step 5: Build + unit-compile.** `cargo build -p rmapps` → success. (Behavioral verification is the live test in Task 10.)

- [ ] **Step 6: Commit.**

```bash
git add crates/rm-cloud/src/config.rs crates/rm-cloud/src/client.rs crates/rm-cloud/src/lib.rs crates/rm-cloud/Cargo.toml apps/rmapps/src/watch/notify_ws.rs apps/rmapps/src/watch/notify.rs Cargo.lock
git commit -m "feat: reMarkable notification websocket source (push) with reconnect"
```

---

## Task 8: Daemon wiring — `rmapps watch` command + state + select! loop

**Goal:** Assemble the resident daemon: a `Watch` subcommand that loads config, builds one `Cloud`, runs the scheduler + notification source(s) + reactor in a `tokio::select!` loop, persists baseline/last_run/pending_jobs, and does a startup reconcile + schedule catch-up.

**Files:**
- Create: `apps/rmapps/src/watch/state.rs`
- Modify: `apps/rmapps/src/watch/mod.rs` (the `run` entry + loop)
- Modify: `apps/rmapps/src/main.rs` (register `Watch`)
- Modify: `apps/rmapps/src/sync.rs` (drop `on-change`; expose schedule helpers reused by the scheduler if needed)

**Acceptance Criteria:**
- [ ] `rmapps watch` starts, logs the resolved rules + schedule, and runs until killed.
- [ ] Flags `--once`, `--poll-only`, `--poll-interval <dur>` behave per spec.
- [ ] State (`baseline` `{id->hash}`, `last_run`, `pending_jobs`) persists atomically and reloads.
- [ ] On startup: one reconcile against the persisted baseline, and any caught-up scheduled task fires once.
- [ ] A reactor pass: snapshot → `diff_ids` → resolve `ChangedDoc` metadata/paths → `filter_self_writes` → `route` → debounce → `actions::run_job`; failed jobs land in `pending_jobs` with bounded retries.

**Verify:** `cargo build -p rmapps` and `cargo run -p rmapps -- watch --once --poll-only` against a fake host (or `--help`) → runs one pass and exits 0. Full behavior covered by Task 9.

**Steps:**

- [ ] **Step 1: State module** `apps/rmapps/src/watch/state.rs` — extend today's `sync-state.json` shape:

```rust
//! Persistent daemon state: diff baseline + scheduler last-run + failed-job retries.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct WatchState {
    /// Compact diff baseline: doc id -> doc hash at last reconcile.
    #[serde(default)]
    pub baseline: BTreeMap<String, String>,
    /// Scheduler: task key -> last successful run (unix secs).
    #[serde(default)]
    pub last_run: BTreeMap<String, u64>,
    /// Failed reactive jobs awaiting retry.
    #[serde(default)]
    pub pending_jobs: Vec<PendingJob>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingJob { pub rule_path: String, pub doc_id: String, pub new_hash: String, pub attempts: u32 }

pub const MAX_ATTEMPTS: u32 = 5;

pub fn state_path() -> PathBuf {
    let base = dirs::state_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local").join("state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"));
    base.join("rmapps").join("watch-state.json")
}
// load()/save() mirror sync.rs: tolerant load (fresh on corrupt), atomic temp+rename save.
```

> Reuse the exact tolerant-load + atomic-save logic from `sync.rs` (`load_state`/`save_state`). Lift those two helpers into `state.rs` generically and have `sync.rs` migration (Step 4) use the new type, or keep them parallel — your call; keep one obvious source.

- [ ] **Step 2: The daemon `run`** in `apps/rmapps/src/watch/mod.rs`:

```rust
pub mod schedule; pub mod reconcile; pub mod debounce; pub mod actions; pub mod notify; pub mod notify_ws; pub mod state;

use clap::Args;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::cloud::Cloud;
use crate::config::Config;

#[derive(Args)]
pub struct WatchArgs {
    /// Run a single reconcile pass and exit.
    #[arg(long)] pub once: bool,
    /// Skip the websocket; rely on the safety-net poll only.
    #[arg(long)] pub poll_only: bool,
    /// Safety-net poll cadence (default 5m).
    #[arg(long, default_value = "5m")] pub poll_interval: String,
}

pub fn run(args: WatchArgs, cfg: &Config) -> anyhow::Result<()> {
    let cloud = Cloud::from_stored()?;
    // The Cloud owns the runtime; drive the async loop on it.
    cloud.block_on(async { run_loop(&args, cfg, &cloud).await })
}
```

`run_loop` (async): resolve `[[watch]]` rules to `ResolvedRule` (parse debounce); resolve each rule path to a folder id (best-effort, re-resolve on miss); load `WatchState`; do startup reconcile + scheduler catch-up; then loop with `tokio::select!` over: (a) the notification source's `next_wakeup`, (b) `PollSource` tick, (c) the next scheduler deadline (`schedule::next_fire`), (d) the debouncer's `next_deadline`. On wakeup/poll → `reconcile_pass`. On scheduler deadline → run the due `[[sync]]` task via the existing `sync::run_task` equivalent and update `last_run`. On debounce deadline → drain `debouncer.ready(now)` and run each via `actions::run_job`, recording failures into `pending_jobs`. If `args.once`, run exactly one reconcile pass (+ drain debouncer immediately) and return.

- [ ] **Step 3: `reconcile_pass`** (the impure shell over pure cores):

```rust
async fn reconcile_pass(cloud: &Cloud, cfg: &Config, rules: &[reconcile::ResolvedRule],
                        st: &mut state::WatchState, deb: &mut debounce::Debouncer) -> anyhow::Result<()> {
    let snap = cloud.client().snapshot().await?;
    if snap.generation == /* baseline generation tracked alongside */ stored_gen(st) { return Ok(()); }
    let current: std::collections::BTreeMap<String,String> =
        snap.docs().map(|d| (d.id.clone(), d.hash.clone())).collect();
    let changed_ids = reconcile::diff_ids(&st.baseline, &current);
    // Resolve metadata/paths for changed ids only (cheap: usually few). Build ChangedDoc via
    // client.metadata_by + parent-chain resolution (cache folder id->name+parent within this pass).
    let docs = resolve_changed_docs(cloud, &snap, &changed_ids).await?;
    let suffixes = digest_suffixes(cfg); // [annotated_suffix, digest_suffix] if [digest] present
    let app_key_ids = app_key_doc_ids(&docs); // docs whose metadata.extra has rmCloudKey
    let kept = reconcile::filter_self_writes(docs, &suffixes, &app_key_ids);
    let now = Instant::now();
    for job in reconcile::route(&kept, rules) { deb.offer(job, now); }
    st.baseline = current; // advance baseline after building jobs
    state::save(st)?;
    Ok(())
}
```

> `resolve_changed_docs`: for each changed id, `client.metadata_by(hash, id)` → `{visible_name, parent, extra}`; build the full path by walking parent folder metadata up to root (cache within the pass). Mark docs carrying `extra["rmCloudKey"]` for the self-write set. Keep this function small and covered indirectly by Task 9's live-ish fake test.

- [ ] **Step 4: Register the subcommand** in `main.rs`:

```rust
// in enum Command:
/// Run the resident daemon: scheduled tasks + push-driven reactions.
Watch(watch::WatchArgs),
// in match:
Command::Watch(args) => { let cfg = config::load(cfg_path)?; watch::run(args, &cfg) }
```

Update the module docstring's subcommand list. In `sync.rs`, remove the `on-change` arm from `resolve_due` and the `current_generation`/`last_generation` plumbing (now the reactor's responsibility); keep `parse_every`→ or delegate to `schedule::parse_duration`.

- [ ] **Step 5: Build + smoke.** `cargo build -p rmapps`; `cargo run -p rmapps -- watch --help` → shows flags.

- [ ] **Step 6: Commit.**

```bash
git add apps/rmapps/src/watch/ apps/rmapps/src/main.rs apps/rmapps/src/sync.rs
git commit -m "feat(rmapps): rmapps watch daemon — scheduler + reactor + state"
```

---

## Task 9: Reactor integration test against the fake cloud

**Goal:** Prove the end-to-end reactor pipeline (wakeup → diff → route → action) against the in-repo fake cloud, including the self-write no-loop guarantee.

**Files:**
- Create: `apps/rmapps/tests/watch_reactor.rs`

**Acceptance Criteria:**
- [ ] Committing a doc-content change under a watched folder in the fake cloud produces exactly one job for the right rule/action (action behind a recording fake).
- [ ] A digest-output deploy (name ends with the digest suffix) produces **no** follow-on job.

**Verify:** `cargo test -p rmapps --features <fake-needed> watch_reactor` → pass.

**Steps:**

- [ ] **Step 1: Stand up the fake cloud** the same way `rm-cloud` tests do (`crates/rm-cloud/src/fake`). Point a `Client` at it via `Config::single_host(fake_url)`; seed a folder `/Books` and a doc.

- [ ] **Step 2: Test the happy path.** Build a baseline `{id->hash}`, commit a content-only change to the seeded doc (bumps its hash + generation), run one `reconcile_pass` with a recording action sink, assert one `Digest` job for that doc.

- [ ] **Step 3: Test the self-write guard.** Seed a doc named `Book — Digest` (digest suffix) under `/Books`, change it, run a pass, assert **zero** jobs.

- [ ] **Step 4: Run, expect pass.** `cargo test -p rmapps watch_reactor`.

- [ ] **Step 5: Commit.**

```bash
git add apps/rmapps/tests/watch_reactor.rs
git commit -m "test(rmapps): reactor integration against fake cloud incl. self-write guard"
```

---

## Task 10: Tier A automated live e2e (gated)

**Goal:** A `#[ignore]`-gated test that exercises the real cloud + real websocket: an API change produces a push wakeup and the expected targeted action, in an isolated scratch folder, cleaned up on success.

**Files:**
- Create: `apps/rmapps/tests/watch_live.rs`

**Acceptance Criteria:**
- [ ] Gated by `RM_CLOUD_DEVICE_TOKEN` + `#[ignore]`; skips cleanly when unset (mirrors `rm-cloud/tests/real_cloud.rs`).
- [ ] All work inside `rmrs-test/<run-id>/`; folder removed on success, left on failure.
- [ ] Asserts: a second-connection content change delivers a `Wakeup` via the real `WsSource`, the reconcile produces the expected `ChangedDoc`, and the recording action fires.
- [ ] Uses a captured annotated bundle fixture so `digest`/`readback` run on realistic `.rm` data.

**Verify (run by Claude on saturn):**
```bash
RM_CLOUD_DEVICE_TOKEN=$(cat ~/.config/rmapps/auth.json | jq -r .device_token) \
  cargo test -p rmapps -- --ignored watch_live
```
Expected: PASS; scratch folder gone afterward.

**Steps:**

- [ ] **Step 1: Scaffold** mirroring `real_cloud.rs`: `client_or_skip()`, `get_or_create_folder`, run-folder isolation, leave-on-failure.

- [ ] **Step 2: Subscribe + change.** Start `WsSource` on one client; from a second client, `put` a doc into `rmrs-test/<run>/Books` then `put_content_only` to bump it. Await a `Wakeup` with a generous timeout (e.g. 30s); if the websocket is unconfirmed/unavailable, assert the poll path instead and log that push was skipped (no silent cap).

- [ ] **Step 3: Assert routing + action.** Run one `reconcile_pass` with a recording action sink; assert one job for the changed doc.

- [ ] **Step 4: Content fixture.** Commit `apps/rmapps/tests/fixtures/annotated.rmdoc` (a captured real annotated bundle); upload it, assert `digest` `run_one` (dry-run) produces output without error.

- [ ] **Step 5: Run live on saturn** (the Verify command). Confirm pass + cleanup.

- [ ] **Step 6: Commit.**

```bash
git add apps/rmapps/tests/watch_live.rs apps/rmapps/tests/fixtures/annotated.rmdoc
git commit -m "test(rmapps): Tier A gated live e2e for push-driven reaction"
```

---

## Task 11: systemd unit, config example, docs cleanup

**Goal:** Ship the daemon as a service and update docs/config to the new model; remove stale `on-change`/cron framing.

**Files:**
- Create: `apps/rmapps/dist/rmapps-watch.service` (systemd unit template, binds nothing network-facing; logs to journald, reachable via `journalctl` over Tailscale).
- Modify: `README.md` (replace `sync`-on-cron framing with `watch`; document `[[watch]]` + `at`).
- Modify: any config example referencing `trigger = "on-change"`.

**Acceptance Criteria:**
- [ ] A documented `systemctl --user` (or system) unit running `rmapps watch`.
- [ ] README shows `[[sync]]` `at`, `[[watch]]` rules, and the `--poll-only` fallback.
- [ ] No remaining references to the removed `on-change` trigger.

**Verify:** `grep -rn "on-change" README.md apps/ crates/` → no stale references; `cargo build --release` → binary builds.

**Steps:**

- [ ] **Step 1: Unit file** `apps/rmapps/dist/rmapps-watch.service`:

```ini
[Unit]
Description=rmapps watch — reMarkable event-driven sync daemon
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=%h/.cargo/bin/rmapps watch
Restart=on-failure
RestartSec=10

[Install]
WantedBy=default.target
```

- [ ] **Step 2: Update README** — new `### 🔁` section: `rmapps watch` runs scheduled jobs (`at`/`every`) and reacts to the tablet via push; show the config and the systemd unit; note `--poll-only`.

- [ ] **Step 3: Grep + scrub** any `on-change` mentions.

- [ ] **Step 4: Build release.** `cargo build --release`.

- [ ] **Step 5: Commit.**

```bash
git add apps/rmapps/dist/rmapps-watch.service README.md
git commit -m "docs(rmapps): document watch daemon + systemd unit; drop on-change"
```

---

## Task 12: Manual real-device verification (Tier B)

**Goal:** Confirm the real hardware → cloud → reaction loop with Dan: highlight a real book on the Paper Pro, let it sync, and verify a digest appears beside it.

**Files:** none (verification task).

**User Verification Required:**
Before marking this task complete, you MUST call AskUserQuestion:
```yaml
AskUserQuestion:
  question: "With rmapps watch running on saturn: you highlighted a book on the Paper Pro and let it sync. Did a digest appear next to it (and, for an annotated article, did highlights reach Readwise) within ~1 minute?"
  header: "Verification"
  options:
    - label: "Worked"
      description: "Digest/readback appeared promptly — push reaction confirmed end to end."
    - label: "Didn't work"
      description: "Nothing happened or it errored — needs investigation (check journalctl, --poll-only)."
```
**If the user selects the negative option:** the task is NOT complete. Investigate (journald logs, websocket vs poll path, self-write filter, folder path match), fix, and re-verify.

**Steps:**

- [ ] **Step 1:** Deploy the release binary + enable the unit on saturn; confirm `journalctl --user -u rmapps-watch` shows the resolved rules.
- [ ] **Step 2:** Ask Dan to highlight a real book under a watched folder and sync the tablet.
- [ ] **Step 3:** Watch the logs for the reaction; confirm the digest lands beside the source.
- [ ] **Step 4:** Call the AskUserQuestion above. Only mark complete on "Worked".

```json:metadata
{"files": [], "verifyCommand": "", "acceptanceCriteria": ["user confirms digest appears after real highlight"], "requiresUserVerification": true, "userVerificationPrompt": "With rmapps watch running: you highlighted a book on the Paper Pro and synced. Did a digest appear next to it within ~1 minute?"}
```

---

## Self-Review

**Spec coverage:** push trigger (T7), unified daemon (T8), scheduler `at`/`every`/timezone/catch-up (T0,T1,T8), `[[watch]]` rules + validation (T0), reconcile/diff (T2), self-write filter (T3), debounce (T4), targeted actions (T5), notification trait + poll fallback (T6), state/baseline/pending_jobs (T8), startup reconcile (T8), error isolation (T5,T8), fake-cloud integration (T9), Tier A live e2e (T10), Tier B manual (T12), systemd + docs (T11). All spec sections map to a task.

**Type consistency:** `WatchAction`, `WatchRule`, `ResolvedRule`, `ChangedDoc`, `Job`, `Wakeup`, `NotificationSource`, `WatchState`/`PendingJob`, `Sched`, `parse_duration` used consistently across tasks. `run_one`/`sync_collection` signatures match the crates as read.

**Placeholder scan:** the only deliberately-open item is the websocket endpoint specifics in T7 — unavoidable (undocumented protocol), explicitly handled by a discovery spike + poll-only fallback so the feature ships regardless. Flagged, not hidden.

**Verification scan:** prompt/spec requires user verification (Tier B) → Task 12 carries `requiresUserVerification: true`. Gate satisfied.
