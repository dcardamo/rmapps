//! `rmapps watch` — the resident daemon (scheduler + push reactor).
pub mod actions;
pub mod debounce;
pub mod notify;
pub mod reconcile;
pub mod schedule;
pub mod state;

#[cfg(test)]
mod reactor_tests;

use anyhow::Result;
use clap::Args;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use crate::cloud::Cloud;
use crate::config::{Config, SyncTask};
use crate::watch::reconcile::{ChangedDoc, Job, ResolvedRule};
use crate::watch::schedule::Sched;

#[derive(Args)]
pub struct WatchArgs {
    /// Run a single reconcile pass (+ due scheduled tasks) and exit.
    #[arg(long)]
    pub once: bool,
    /// Skip the websocket; rely on the safety-net poll only. (No effect until 8b/Task 7.)
    #[arg(long)]
    pub poll_only: bool,
    /// Safety-net poll cadence (default 5m). (Used by 8b.)
    #[arg(long, default_value = "5m")]
    pub poll_interval: String,
}

/// Suffixes that mark daemon-generated docs (digest outputs), for self-write filtering
/// and for list_recursive's exclude list.
fn exclude_suffixes(cfg: &Config) -> Vec<String> {
    cfg.digest
        .as_ref()
        .map(|d| {
            vec![
                d.output.annotated_suffix.clone(),
                d.output.digest_suffix.clone(),
            ]
        })
        .unwrap_or_default()
}

/// Build resolved watch rules (debounce parsed). Validation already guaranteed parseability.
fn resolved_rules(cfg: &Config) -> Vec<ResolvedRule> {
    cfg.watch
        .iter()
        .map(|r| ResolvedRule {
            path: r.path.clone(),
            action: r.action,
            debounce: schedule::parse_duration(&r.debounce).unwrap_or(Duration::from_secs(30)),
        })
        .collect()
}

/// Resolve a SyncTask to a schedule form. Returns None if neither every nor at is set
/// (such tasks are skipped by callers).
fn task_sched(task: &SyncTask) -> Option<Sched> {
    if let Some(every) = &task.every {
        schedule::parse_duration(every).ok().map(Sched::Every)
    } else if let Some(times) = &task.at {
        let parsed: Vec<(u32, u32)> = times
            .iter()
            .filter_map(|s| crate::config::parse_hhmm(s).ok())
            .collect();
        Some(Sched::At(parsed))
    } else {
        None
    }
}

/// The configured timezone, or UTC if unset/unparseable.
fn timezone(cfg: &Config) -> chrono_tz::Tz {
    cfg.timezone
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(chrono_tz::UTC)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// One reconcile: detect changed docs under watched roots and enqueue reactive jobs.
/// Synchronous. Advances the baseline. Does NOT run actions (caller drains the debouncer).
fn reconcile_pass(
    cloud: &Cloud,
    rules: &[ResolvedRule],
    exclude: &[String],
    state: &mut state::WatchState,
    debouncer: &mut crate::watch::debounce::Debouncer,
) -> Result<()> {
    // Cheap generation check first.
    let gen = cloud.current_generation()?; // Option<i64>
    if let (Some(g), Some(bg)) = (gen, state.baseline_generation) {
        if g == bg {
            return Ok(()); // nothing changed
        }
    }

    // Full snapshot: id -> hash.
    let snap = cloud
        .block_on(cloud.client().snapshot())
        .map_err(|e| anyhow::anyhow!("snapshot: {e}"))?;
    let current: BTreeMap<String, String> =
        snap.docs().map(|d| (d.id.clone(), d.hash.clone())).collect();

    let changed_ids: BTreeSet<String> = reconcile::diff_ids(&state.baseline, &current)
        .into_iter()
        .collect();

    if !changed_ids.is_empty() {
        // Resolve paths for changed docs by joining against list_recursive over each
        // DISTINCT watched root. list_recursive already drops suffix-excluded (digest) docs.
        let mut roots: Vec<&str> = rules.iter().map(|r| r.path.as_str()).collect();
        roots.sort_unstable();
        roots.dedup();
        let mut by_id: BTreeMap<String, ChangedDoc> = BTreeMap::new();
        for root in roots {
            for rd in cloud.list_recursive(root, exclude)? {
                if changed_ids.contains(&rd.id) {
                    by_id.entry(rd.id.clone()).or_insert(ChangedDoc {
                        id: rd.id,
                        name: rd.name,
                        parent: rd.folder,
                        path: rd.path,
                    });
                }
            }
        }
        let docs: Vec<ChangedDoc> = by_id.into_values().collect();
        // Suffix self-write filtering (digest outputs). App-key (rmCloudKey) filtering is
        // intentionally NOT applied: readback-watched articles are app-deployed yet must
        // still react to user annotations. Daemon self-writes are otherwise excluded by
        // baseline advancement + the digest suffix filter.
        let kept = reconcile::filter_self_writes(docs, exclude, &BTreeSet::new());
        let now = Instant::now();
        for job in reconcile::route(&kept, rules) {
            debouncer.offer(job, now);
        }
    }

    state.baseline = current;
    // Use the snapshot's own generation so the next pass's cheap generation check matches
    // and skips a redundant snapshot (rather than the separately-polled `gen` above, which
    // could differ if the account changed between the two calls).
    state.baseline_generation = Some(snap.generation);
    state::save(state)?;
    Ok(())
}

/// Run scheduled tasks that are due. Fires any `every` task whose interval elapsed and any
/// `at` time already passed today that hasn't been run since (catch-up on startup, and at
/// each `at` time during the resident loop). Updates last_run on success and last_attempt on
/// every attempt (so a failing `every` task is paced by its interval, not re-run back-to-back).
/// Returns true if any task was attempted (state changed).
fn run_due_scheduled(cfg: &Config, state: &mut state::WatchState) -> bool {
    let tz = timezone(cfg);
    let now_utc = chrono::Utc::now();
    let now_s = now_secs();
    let mut ran = false;
    for (i, task) in cfg.sync.iter().enumerate() {
        let key = format!("{}#{}", task.app, i);
        let Some(sched) = task_sched(task) else {
            eprintln!("[rmapps] watch: sync task {key} has neither every nor at; skipping");
            continue;
        };
        let last_run_s = state.last_run.get(&key).copied();
        let last_attempt_s = state.last_attempt.get(&key).copied();
        let due = match &sched {
            // Attempt-anchored interval rule: due if never run/attempted, or the interval has
            // elapsed since the LATER of last success and last attempt. Anchoring on the
            // attempt (not just success) means a failing task waits its full interval before
            // retrying instead of busy-looping.
            Sched::Every(d) => {
                schedule::every_due(last_run_s, last_attempt_s, d.as_secs(), now_s)
            }
            // `due_on_startup` means "a past `at` time today exists that we haven't run
            // since". That is exactly the steady-state firing condition too: at each `at`
            // time the most-recent past time advances past `last_run`, so the task fires
            // once and `last_run` advancing on success prevents a re-fire until the next
            // `at` time. So this is NOT gated on `startup` — `at` tasks fire whenever their
            // time arrives, in both the startup catch-up and the resident loop.
            Sched::At(_) => {
                let last_dt = last_run_s
                    .and_then(|s| chrono::DateTime::from_timestamp(s as i64, 0));
                schedule::due_on_startup(&sched, tz, last_dt, now_utc)
            }
        };
        if due {
            ran = true;
            // Record the attempt up-front so even a panic/early-return downstream cannot
            // leave an `every` task eligible to immediately re-fire.
            state.last_attempt.insert(key.clone(), now_secs());
            println!("[rmapps] watch: running scheduled {key}");
            match crate::sync::run_task(task, &key, cfg) {
                Ok(()) => {
                    state.last_run.insert(key, now_secs());
                }
                Err(e) => eprintln!("[rmapps] watch: scheduled {key} failed: {e:#}"),
            }
        }
    }
    ran
}

/// Max reactive-action retries before giving up on a doc (leaving the baseline advanced so it
/// is no longer re-detected).
const MAX_ATTEMPTS: u32 = 5;

/// Drain ALL pending debounced jobs immediately (used by --once) and run them.
/// Returns true if any job ran (state may have changed).
fn drain_and_run(
    cloud: &Cloud,
    cfg: &Config,
    state: &mut state::WatchState,
    debouncer: &mut crate::watch::debounce::Debouncer,
) -> bool {
    let far_future = Instant::now() + Duration::from_secs(10_000_000);
    let mut ran = false;
    for job in debouncer.ready(far_future) {
        run_one_job(cloud, cfg, state, &job);
        ran = true;
    }
    ran
}

/// Run a single reactive job. Retry of failures is driven through the reconcile path: on
/// failure we ROLL BACK the doc's baseline entry (remove it) so the next reconcile re-detects
/// the change and re-routes the job, retrying the action. This is bounded by `failed_attempts`:
/// after `MAX_ATTEMPTS` consecutive failures we give up and leave the baseline advanced (set by
/// reconcile_pass before this drain) so the doc is no longer re-detected. On success we clear
/// the counter; the baseline already holds the current hash.
fn run_one_job(cloud: &Cloud, cfg: &Config, state: &mut state::WatchState, job: &Job) {
    match crate::watch::actions::run_job(cloud, cfg, job) {
        Ok(()) => {
            println!("[rmapps] watch: reacted: {:?} {}", job.action, job.doc.path);
            // Baseline already advanced to the current hash by reconcile_pass; just clear the
            // failure counter so a future change starts fresh.
            state.failed_attempts.remove(&job.doc.id);
        }
        Err(e) => {
            eprintln!("[rmapps] watch: action failed for {}: {e:#}", job.doc.path);
            let attempts = state
                .failed_attempts
                .entry(job.doc.id.clone())
                .or_insert(0);
            *attempts += 1;
            if *attempts >= MAX_ATTEMPTS {
                eprintln!(
                    "[rmapps] watch: giving up on {} after {} attempts",
                    job.doc.path, *attempts
                );
                // Give up: leave baseline advanced (don't roll back) so it isn't re-detected,
                // and drop the counter so memory doesn't grow unbounded.
                state.failed_attempts.remove(&job.doc.id);
            } else {
                // Roll back this doc's baseline entry so the next reconcile re-detects the
                // change and re-routes the job, retrying the action (bounded by poll cadence).
                state.baseline.remove(&job.doc.id);
            }
        }
    }
}

pub fn run(args: WatchArgs, cfg: &Config) -> Result<()> {
    let cloud = Cloud::from_stored()?;
    let rules = resolved_rules(cfg);
    let exclude = exclude_suffixes(cfg);
    let mut state = state::load();
    let mut debouncer = crate::watch::debounce::Debouncer::default();

    println!(
        "[rmapps] watch: {} sync task(s), {} watch rule(s)",
        cfg.sync.len(),
        rules.len()
    );

    if args.once {
        run_due_scheduled(cfg, &mut state);
        reconcile_pass(&cloud, &rules, &exclude, &mut state, &mut debouncer)?;
        drain_and_run(&cloud, cfg, &mut state, &mut debouncer);
        state::save(&state)?;
        return Ok(());
    }

    // NOTE: reconcile_pass already calls state::save internally when it advances the baseline,
    // so the resident loop below only saves when other state (scheduler/reactive) changed.

    // ── Resident loop ──────────────────────────────────────────────────────────────────
    // SYNCHRONOUS: all cloud work runs on this thread, never inside a tokio context, which
    // avoids the nested-runtime panic (Cloud::block_on already drives a runtime internally).
    // Wakeups arrive over a std mpsc channel; the loop blocks on rx.recv_timeout(sleep)
    // where `sleep` is the time until the soonest deadline (poll / debounce / next `at`/`every`
    // fire). The safety-net poll IS the recv_timeout cadence: a timeout triggers a reconcile.
    let poll_interval =
        schedule::parse_duration(&args.poll_interval).unwrap_or(Duration::from_secs(300));
    // Task 7 will feed `_tx` from the websocket source thread; for 8b nothing sends, so the
    // loop is driven purely by recv_timeout deadlines.
    let (_tx, rx) = std::sync::mpsc::channel::<crate::watch::notify::Wakeup>();

    if args.poll_only {
        println!("[rmapps] watch: poll-only mode, reconciling every {poll_interval:?}");
    } else {
        println!(
            "[rmapps] watch: websocket push not yet available; running in poll mode ({poll_interval:?})"
        );
    }

    // Startup: catch up missed scheduled tasks + one reconcile so downtime changes are
    // processed before we settle into the wait loop.
    run_due_scheduled(cfg, &mut state);
    reconcile_pass(&cloud, &rules, &exclude, &mut state, &mut debouncer)?;
    drain_and_run(&cloud, cfg, &mut state, &mut debouncer);
    let _ = state::save(&state);

    let mut poll_deadline = Instant::now() + poll_interval;

    loop {
        let now = Instant::now();
        // Next wake = soonest of: poll deadline, debounce deadline, next scheduled fire.
        let mut next = poll_deadline;
        if let Some(d) = debouncer.next_deadline() {
            next = next.min(d);
        }
        if let Some(d) = next_scheduled_instant(cfg, &state, now) {
            next = next.min(d);
        }
        // Defense-in-depth: clamp to a 1s floor so an unforeseen "deadline == now" (e.g. a
        // scheduled fire that maps to the present instant) can never spin the loop faster
        // than once per second.
        let sleep = next
            .saturating_duration_since(Instant::now())
            .max(Duration::from_secs(1));

        let woke = match rx.recv_timeout(sleep) {
            Ok(_) => true,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => false,
            // No sender exists yet (Task 7 adds one); a disconnected channel behaves like a
            // timeout so the loop keeps running on its deadlines.
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => false,
        };

        let now = Instant::now();
        // Track whether state mutated this tick so we only persist when something changed,
        // not on every idle poll wakeup. (reconcile_pass saves its own baseline advances
        // internally, so `dirty` here covers scheduler + reactive mutations.)
        let mut dirty = false;
        if woke || now >= poll_deadline {
            if let Err(e) = reconcile_pass(&cloud, &rules, &exclude, &mut state, &mut debouncer) {
                eprintln!("[rmapps] watch: reconcile failed: {e:#}");
            }
            poll_deadline = Instant::now() + poll_interval;
        }
        // Fire any scheduled tasks now due (every/at), then run any debounced jobs whose
        // window elapsed. Both mutate state (last_run/last_attempt, baseline/failed_attempts).
        if run_due_scheduled(cfg, &mut state) {
            dirty = true;
        }
        for job in debouncer.ready(Instant::now()) {
            run_one_job(&cloud, cfg, &mut state, &job);
            dirty = true;
        }
        if dirty {
            let _ = state::save(&state);
        }
    }
}

/// Soonest scheduled fire (as a monotonic `Instant`) across all sync tasks, or None if no
/// task has a schedule. Converts each task's next chrono fire-time into an Instant relative
/// to `now`. Tasks without a schedule are skipped. A fire already in the past yields
/// `Some(now)` so the loop wakes promptly to process it.
fn next_scheduled_instant(cfg: &Config, state: &state::WatchState, now: Instant) -> Option<Instant> {
    let tz = timezone(cfg);
    let utc_now = chrono::Utc::now();
    let mut soonest: Option<Instant> = None;
    for (i, task) in cfg.sync.iter().enumerate() {
        let Some(sched) = task_sched(task) else {
            continue;
        };
        let key = format!("{}#{}", task.app, i);
        // For `Every`, anchor the base time on the LATER of last_run and last_attempt so a
        // just-failed task's next fire is a full interval out (matching run_due_scheduled's
        // every_due pacing) rather than ~now (which would spin the loop). For `At`, only the
        // last successful run matters for "have we run since this time".
        let anchor_secs = match &sched {
            Sched::Every(_) => state
                .last_run
                .get(&key)
                .copied()
                .max(state.last_attempt.get(&key).copied()),
            Sched::At(_) => state.last_run.get(&key).copied(),
        };
        let last_dt = anchor_secs.and_then(|s| chrono::DateTime::from_timestamp(s as i64, 0));
        let fire_utc = schedule::next_fire(&sched, tz, last_dt, utc_now);
        // chrono Duration -> std Duration; negative (past) clamps to zero so we wake now.
        let until = (fire_utc - utc_now)
            .to_std()
            .unwrap_or(Duration::ZERO);
        let inst = now + until;
        soonest = Some(soonest.map_or(inst, |s| s.min(inst)));
    }
    soonest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SyncTask;

    fn task_every(app: &str, every: &str) -> SyncTask {
        SyncTask {
            app: app.to_string(),
            every: Some(every.to_string()),
            at: None,
            month_window: None,
        }
    }

    #[test]
    fn next_scheduled_instant_returns_soonest_of_two_tasks() {
        // Two never-run `every` tasks: next_fire for a never-run interval task is
        // `now + interval`, so the soonest deadline is the shorter interval (1m).
        let mut cfg = Config::default();
        cfg.sync = vec![task_every("digest", "1h"), task_every("reader", "1m")];

        let now = Instant::now();
        let got = next_scheduled_instant(&cfg, &state::WatchState::default(), now)
            .expect("two scheduled tasks => Some");

        // The soonest fire is ~1 minute out. Allow generous slack for the Utc::now()
        // calls inside the function vs. the test's `now`.
        let delta = got.saturating_duration_since(now);
        assert!(
            delta <= Duration::from_secs(70),
            "expected soonest deadline ~1m, got {delta:?}"
        );
        assert!(
            delta >= Duration::from_secs(50),
            "expected soonest deadline ~1m, got {delta:?}"
        );
    }

    #[test]
    fn next_scheduled_instant_none_without_schedules() {
        let cfg = Config::default(); // no sync tasks
        assert!(next_scheduled_instant(&cfg, &state::WatchState::default(), Instant::now()).is_none());
    }
}
