//! Pure scheduling math for `[[sync]]` tasks. No I/O; `now` is always injected.
use chrono::{DateTime, Duration as ChDuration, TimeZone, Utc};
use chrono_tz::Tz;
use std::time::Duration;

/// A task's schedule form, resolved from config.
#[allow(dead_code)]
pub enum Sched {
    Every(Duration),
    At(Vec<(u32, u32)>), // (hour, minute)
}

/// Parse `<N>s|m|h|d`.
#[allow(dead_code)]
pub fn parse_duration(s: &str) -> anyhow::Result<Duration> {
    let s = s.trim();
    let idx = s
        .find(|c: char| !c.is_ascii_digit())
        .ok_or_else(|| anyhow::anyhow!("invalid duration {s:?}: expected <N>s|m|h|d"))?;
    let (num, unit) = s.split_at(idx);
    let n: u64 = num
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid duration {s:?}: bad number"))?;
    let secs = match unit {
        "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        "d" => n * 86400,
        other => anyhow::bail!("invalid duration {s:?}: unknown unit {other:?}"),
    };
    Ok(Duration::from_secs(secs))
}

/// Next fire instant (UTC) strictly after `now`.
#[allow(dead_code)]
pub fn next_fire(
    sched: &Sched,
    tz: Tz,
    last_run: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> DateTime<Utc> {
    match sched {
        Sched::Every(d) => {
            let base = last_run.unwrap_or(now);
            let cand = base + ChDuration::from_std(*d).unwrap_or_else(|_| ChDuration::zero());
            if cand > now {
                cand
            } else {
                now
            }
        }
        Sched::At(times) => {
            debug_assert!(!times.is_empty(), "next_fire called with empty At time list");
            let mut times = times.clone();
            times.sort_unstable();
            let local_now = now.with_timezone(&tz);
            for day in 0..=1 {
                let date = (local_now + ChDuration::days(day)).date_naive();
                for &(h, m) in &times {
                    if let Some(naive) = date.and_hms_opt(h, m, 0) {
                        // `.single()` returns None for nonexistent (spring-forward gap) or
                        // ambiguous (fall-back) local times; we deliberately skip such times
                        // rather than guess, so an `at` time landing exactly on a DST
                        // transition won't fire that day.
                        if let Some(dt) = tz.from_local_datetime(&naive).single() {
                            let utc = dt.with_timezone(&Utc);
                            if utc > now {
                                return utc;
                            }
                        }
                    }
                }
            }
            now // unreachable in practice
        }
    }
}

/// Pure due-check for an `Every` task. Anchors on the LATER of the last successful run and
/// the last attempt: `last_run` records success, but `last_attempt` (recorded on every
/// attempt regardless of outcome) governs pacing. A task that just failed therefore waits its
/// full interval before retrying instead of busy-looping. `every_secs == 0` is treated as
/// always-due (degenerate config).
///
/// - Never attempted and never run => due.
/// - Anchored < interval ago => NOT due.
/// - Anchored >= interval ago => due.
pub fn every_due(
    last_run: Option<u64>,
    last_attempt: Option<u64>,
    every_secs: u64,
    now_secs: u64,
) -> bool {
    let anchor = match (last_run, last_attempt) {
        (None, None) => return true,
        (a, b) => a.max(b).unwrap_or(0),
    };
    now_secs.saturating_sub(anchor) >= every_secs
}

/// True if an `At` time for *today* has already passed and the task has not run since it.
pub fn due_on_startup(
    sched: &Sched,
    tz: Tz,
    last_run: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    let Sched::At(times) = sched else {
        return false;
    };
    let local_now = now.with_timezone(&tz);
    let today = local_now.date_naive();
    let mut latest_passed: Option<DateTime<Utc>> = None;
    for &(h, m) in times {
        if let Some(naive) = today.and_hms_opt(h, m, 0) {
            // `.single()` returns None for nonexistent (spring-forward gap) or ambiguous
            // (fall-back) local times; we deliberately skip such times rather than guess,
            // so an `at` time landing exactly on a DST transition won't fire that day.
            if let Some(dt) = tz.from_local_datetime(&naive).single() {
                let utc = dt.with_timezone(&Utc);
                if utc <= now {
                    latest_passed = Some(latest_passed.map_or(utc, |p| p.max(utc)));
                }
            }
        }
    }
    match (latest_passed, last_run) {
        (Some(passed), Some(last)) => last < passed,
        (Some(_), None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, TimeZone, Timelike, Utc};
    use chrono_tz::America::Halifax;

    fn task_at(times: &[(u32, u32)]) -> Sched {
        Sched::At(times.to_vec())
    }

    #[test]
    fn parse_duration_units() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("12h").unwrap(), Duration::from_secs(12 * 3600));
        assert_eq!(parse_duration("1d").unwrap(), Duration::from_secs(86400));
        assert!(parse_duration("10").is_err());
        assert!(parse_duration("10x").is_err());
    }

    #[test]
    fn at_picks_next_time_today() {
        let now = Halifax
            .with_ymd_and_hms(2026, 6, 1, 5, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        let next = next_fire(&task_at(&[(6, 0), (18, 0)]), Halifax, None, now);
        let local = next.with_timezone(&Halifax);
        assert_eq!((local.hour(), local.minute()), (6, 0));
        assert_eq!(local.day(), 1);
    }

    #[test]
    fn at_rolls_to_tomorrow_after_last_time() {
        let now = Halifax
            .with_ymd_and_hms(2026, 6, 1, 19, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        let next = next_fire(&task_at(&[(6, 0), (18, 0)]), Halifax, None, now);
        let local = next.with_timezone(&Halifax);
        assert_eq!((local.hour(), local.day()), (6, 2));
    }

    #[test]
    fn every_due_paces_on_attempt() {
        let every = 3600u64;
        let now = 100_000u64;
        // Never attempted, never run => due.
        assert!(every_due(None, None, every, now));
        // Attempted < interval ago, even with a stale last_run => NOT due (no busy-loop).
        assert!(!every_due(Some(0), Some(now - 10), every, now));
        // Attempted just now with no prior success => NOT due (failed task waits).
        assert!(!every_due(None, Some(now - 10), every, now));
        // Last attempt > interval ago => due again.
        assert!(every_due(Some(0), Some(now - every - 1), every, now));
        // Only last_run set (e.g. pre-upgrade state), interval elapsed => due.
        assert!(every_due(Some(now - every - 1), None, every, now));
        // Only last_run set, within interval => NOT due.
        assert!(!every_due(Some(now - 10), None, every, now));
    }

    #[test]
    fn every_uses_last_run() {
        let now = Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap();
        let last = Utc.with_ymd_and_hms(2026, 6, 1, 11, 0, 0).unwrap();
        let next = next_fire(
            &Sched::Every(Duration::from_secs(7200)),
            Halifax,
            Some(last),
            now,
        );
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 6, 1, 13, 0, 0).unwrap());
    }

    #[test]
    fn catchup_fires_once_for_missed_morning() {
        let now = Halifax
            .with_ymd_and_hms(2026, 6, 1, 7, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(due_on_startup(&task_at(&[(6, 0), (18, 0)]), Halifax, None, now));
        let last = Halifax
            .with_ymd_and_hms(2026, 6, 1, 6, 30, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert!(!due_on_startup(
            &task_at(&[(6, 0), (18, 0)]),
            Halifax,
            Some(last),
            now
        ));
    }
}
