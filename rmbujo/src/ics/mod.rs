//! ICS (iCalendar) ingestion: parse `.ics` feeds into per-day event occurrences.

pub mod fetch;
pub mod parse;

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{NaiveDate, NaiveTime};

use crate::config::Config;

/// Build a `BTreeMap` from calendar date to the events that fall on that day,
/// merging all feeds defined in `config`.
///
/// * Feeds are fetched via `fetcher` using `fetch::feed_bytes`.
/// * A feed that fails *and* has no cache is skipped with a warning; the
///   remaining feeds are still included in the result.
/// * Each day's vec is sorted: all-day events first, then by time, then by
///   title — deterministic ordering regardless of feed order.
pub fn build_event_map(
    config: &Config,
    out_dir: &Path,
    refresh: bool,
    fetcher: &dyn fetch::Fetcher,
) -> anyhow::Result<BTreeMap<NaiveDate, Vec<EventOccurrence>>> {
    let tz: chrono_tz::Tz = config
        .timezone
        .parse()
        .map_err(|_| anyhow::anyhow!("unknown timezone: {:?}", config.timezone))?;

    let mut map: BTreeMap<NaiveDate, Vec<EventOccurrence>> = BTreeMap::new();

    for feed in &config.ics {
        let bytes = match fetch::feed_bytes(out_dir, feed, refresh, fetcher) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("rmbujo: skipping feed {:?}: {e}", feed.name);
                continue;
            }
        };
        for o in parse::parse_feed(&bytes, &feed.color, config.year, &tz)? {
            map.entry(o.date).or_default().push(o);
        }
    }

    // Sort each day: all-day first, then by time, then by title.
    for v in map.values_mut() {
        v.sort_by(|a, b| {
            a.time
                .is_some()
                .cmp(&b.time.is_some())
                .then(a.time.cmp(&b.time))
                .then_with(|| a.title.cmp(&b.title))
        });
    }

    Ok(map)
}

/// A single calendar event materialized onto one specific day.
///
/// Multi-day and recurring events expand into one `EventOccurrence` per day.
/// Timed events are already converted into the configured timezone, so `date`
/// and `time` reflect local wall-clock values in that zone.
#[derive(Debug, Clone, PartialEq)]
pub struct EventOccurrence {
    pub date: NaiveDate,
    /// `None` = all-day event; `Some` = timed event (already in the config tz).
    pub time: Option<NaiveTime>,
    pub title: String,
    pub location: Option<String>,
    pub description: Option<String>,
    pub attendees: Vec<String>,
    pub color: String,
}
