//! `rmapps watch` — the resident daemon (scheduler + push reactor).
pub mod actions;
pub mod debounce;
pub mod notify;
pub mod reconcile;
pub mod schedule;
pub mod state;

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
    state.baseline_generation = gen;
    state::save(state)?;
    Ok(())
}

/// Run scheduled tasks that are due. On startup, fire any `at` time already passed today
/// (catch-up) and any `every` task that is due. Updates last_run on success.
fn run_due_scheduled(cfg: &Config, state: &mut state::WatchState, startup: bool) {
    let tz = timezone(cfg);
    let now_utc = chrono::Utc::now();
    let now_s = now_secs();
    for (i, task) in cfg.sync.iter().enumerate() {
        let key = format!("{}#{}", task.app, i);
        let Some(sched) = task_sched(task) else {
            eprintln!("[rmapps] watch: sync task {key} has neither every nor at; skipping");
            continue;
        };
        let last_s = state.last_run.get(&key).copied();
        let due = match &sched {
            // Simple, non-double-firing interval rule: due if never run, or the
            // interval has elapsed since the last successful run.
            Sched::Every(d) => match last_s {
                None => true,
                Some(last) => now_s.saturating_sub(last) >= d.as_secs(),
            },
            Sched::At(_) => {
                let last_dt = last_s
                    .and_then(|s| chrono::DateTime::from_timestamp(s as i64, 0));
                startup && schedule::due_on_startup(&sched, tz, last_dt, now_utc)
            }
        };
        if due {
            println!("[rmapps] watch: running scheduled {key}");
            match crate::sync::run_task(task, &key, cfg) {
                Ok(()) => {
                    state.last_run.insert(key, now_secs());
                }
                Err(e) => eprintln!("[rmapps] watch: scheduled {key} failed: {e:#}"),
            }
        }
    }
}

/// Drain ALL pending debounced jobs immediately (used by --once) and run them.
fn drain_and_run(
    cloud: &Cloud,
    cfg: &Config,
    state: &mut state::WatchState,
    debouncer: &mut crate::watch::debounce::Debouncer,
) {
    // First, retry any previously-failed jobs (bounded).
    retry_pending(cloud, cfg, state);
    let far_future = Instant::now() + Duration::from_secs(10_000_000);
    for job in debouncer.ready(far_future) {
        run_one_job(cloud, cfg, state, &job);
    }
}

fn run_one_job(cloud: &Cloud, cfg: &Config, state: &mut state::WatchState, job: &Job) {
    match crate::watch::actions::run_job(cloud, cfg, job) {
        Ok(()) => {
            println!("[rmapps] watch: reacted: {:?} {}", job.action, job.doc.path);
        }
        Err(e) => {
            eprintln!("[rmapps] watch: action failed for {}: {e:#}", job.doc.path);
            state.pending_jobs.push(state::PendingJob {
                rule_path: job.rule_path.clone(),
                doc_id: job.doc.id.clone(),
                new_hash: String::new(),
                attempts: 1,
            });
        }
    }
}

/// Retry recorded failed jobs. For 8a, retry is best-effort and bounded; a job needs its
/// doc re-resolved to run — for simplicity drop jobs that exceed MAX_ATTEMPTS and leave a
/// note. (8b refines retry against fresh snapshots.)
fn retry_pending(_cloud: &Cloud, _cfg: &Config, state: &mut state::WatchState) {
    state.pending_jobs.retain_mut(|p| {
        p.attempts += 1;
        if p.attempts > state::MAX_ATTEMPTS {
            eprintln!(
                "[rmapps] watch: giving up on job for doc {} after {} attempts",
                p.doc_id,
                p.attempts - 1
            );
            false
        } else {
            true
        }
    });
    // NOTE: actually re-dispatching a pending job requires re-resolving its ChangedDoc;
    // 8b implements that against a fresh reconcile. For 8a we only age/bound them.
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
        run_due_scheduled(cfg, &mut state, /*startup=*/ true);
        reconcile_pass(&cloud, &rules, &exclude, &mut state, &mut debouncer)?;
        drain_and_run(&cloud, cfg, &mut state, &mut debouncer);
        state::save(&state)?;
        return Ok(());
    }
    // 8b will implement the resident loop here.
    anyhow::bail!("the resident watch loop is not implemented yet; run with --once for now");
}
