//! Parse raw `.ics` feed bytes into per-day [`EventOccurrence`]s.
//!
//! Handles all-day events (single, multi-day via exclusive `DTEND`, and
//! `RRULE`-recurring) and timed events, expanding each into one occurrence per
//! day, clipped to a target year. Timed events are converted into the
//! configured timezone so both the wall-clock time and the calendar day are
//! correct for that zone.
//!
//! Crate APIs used (verified against the installed sources):
//! - `ical` 0.11: `ical::IcalParser::new(BufRead)` yields
//!   `Result<IcalCalendar, _>`; `IcalCalendar.events: Vec<IcalEvent>`; each
//!   event has `properties: Vec<Property>` where
//!   `Property { name: String, params: Option<Vec<(String, Vec<String>)>>, value: Option<String> }`.
//!   The parser does NOT un-escape TEXT values, so we do it here.
//! - `rrule` 0.14: parse a `"DTSTART...\nRRULE:..."` string into `RRuleSet`,
//!   then `.after(dt).before(dt).all(limit)` -> `RRuleResult { dates, limited }`
//!   where `dates: Vec<DateTime<rrule::Tz>>`. `rrule::Tz` wraps `chrono_tz::Tz`
//!   and implements `From<chrono_tz::Tz>`.

use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use ical::parser::ical::component::IcalEvent;
use ical::property::Property;
use rrule::{RRuleSet, Tz as RTz};
use std::io::BufReader;

use super::EventOccurrence;

/// Upper bound on occurrences generated per recurring rule, to bound work on
/// pathological feeds. A single year only needs a handful, so this is generous.
const RRULE_LIMIT: u16 = 1000;

/// Parse a full ICS feed into per-day occurrences for `year`, in timezone `tz`.
///
/// `color` is stamped onto every produced occurrence. Individual malformed
/// events are skipped with a warning rather than failing the whole feed.
pub fn parse_feed(
    bytes: &[u8],
    color: &str,
    year: i32,
    tz: &Tz,
) -> anyhow::Result<Vec<EventOccurrence>> {
    let reader = ical::IcalParser::new(BufReader::new(bytes));

    let mut out: Vec<EventOccurrence> = Vec::new();
    for cal in reader {
        let cal = match cal {
            Ok(c) => c,
            // A broken VCALENDAR header is feed-level; surface it.
            Err(e) => anyhow::bail!("failed to parse ICS feed: {e}"),
        };
        for event in cal.events {
            match expand_event(&event, color, year, tz) {
                Ok(mut occs) => out.append(&mut occs),
                Err(e) => {
                    let uid = prop(&event, "UID")
                        .and_then(|p| p.value.clone())
                        .unwrap_or_else(|| "<no uid>".into());
                    eprintln!("ics: skipping malformed event {uid}: {e}");
                }
            }
        }
    }

    sort_occurrences(&mut out);
    Ok(out)
}

/// Expand a single VEVENT into its in-year occurrences.
fn expand_event(
    event: &IcalEvent,
    color: &str,
    year: i32,
    tz: &Tz,
) -> anyhow::Result<Vec<EventOccurrence>> {
    let dtstart = prop(event, "DTSTART").ok_or_else(|| anyhow::anyhow!("event has no DTSTART"))?;
    let dtstart_value = dtstart
        .value
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("DTSTART has no value"))?;

    let title = text_prop(event, "SUMMARY").unwrap_or_default();
    let location = text_prop(event, "LOCATION");
    let description = text_prop(event, "DESCRIPTION");
    let attendees = attendees(event);
    let rrule = prop(event, "RRULE").and_then(|p| p.value.clone());

    let make = |date: NaiveDate, time: Option<NaiveTime>| EventOccurrence {
        date,
        time,
        title: title.clone(),
        location: location.clone(),
        description: description.clone(),
        attendees: attendees.clone(),
        color: color.to_string(),
    };

    let mut occs = Vec::new();

    if is_all_day(dtstart) {
        // All-day: DTSTART is a DATE (YYYYMMDD).
        let start = parse_date(dtstart_value)?;
        if let Some(rrule_str) = rrule {
            for date in expand_all_day_rrule(start, &rrule_str, year)? {
                occs.push(make(date, None));
            }
        } else if let Some(dtend) = prop(event, "DTEND").and_then(|p| p.value.as_deref()) {
            // DTEND is exclusive: emit [start, end).
            let end = parse_date(dtend)?;
            let mut d = start;
            while d < end {
                if d.year() == year {
                    occs.push(make(d, None));
                }
                d += Duration::days(1);
            }
        } else if start.year() == year {
            occs.push(make(start, None));
        }
    } else {
        // Timed: DTSTART is a DATE-TIME. Resolve to an absolute instant, then
        // convert into the config tz to get the correct local day + time.
        let tzid = param(dtstart, "TZID");
        let start_instant = resolve_instant(dtstart_value, tzid.as_deref(), tz)?;

        if let Some(rrule_str) = rrule {
            for instant in expand_timed_rrule(start_instant, &rrule_str, tz, year)? {
                let local = instant.with_timezone(tz);
                if local.year() == year {
                    occs.push(make(local.date_naive(), Some(local.time())));
                }
            }
        } else {
            let local = start_instant.with_timezone(tz);
            if local.year() == year {
                occs.push(make(local.date_naive(), Some(local.time())));
            }
        }
    }

    Ok(occs)
}

/// True if the DTSTART property is a DATE (`VALUE=DATE`, or an 8-char value with
/// no time component).
fn is_all_day(dtstart: &Property) -> bool {
    if let Some(v) = param(dtstart, "VALUE") {
        if v.eq_ignore_ascii_case("DATE") {
            return true;
        }
    }
    // Fallback: a bare YYYYMMDD value (no 'T') is a date.
    dtstart
        .value
        .as_deref()
        .map(|v| v.len() == 8 && !v.contains('T'))
        .unwrap_or(false)
}

/// Parse a `YYYYMMDD` date string.
fn parse_date(s: &str) -> anyhow::Result<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y%m%d")
        .map_err(|e| anyhow::anyhow!("bad DATE value {s:?}: {e}"))
}

/// Resolve a DATE-TIME value (`YYYYMMDDTHHMMSS`, optionally `Z`-suffixed) plus an
/// optional TZID into an absolute UTC instant.
///
/// - `Z` suffix -> UTC.
/// - TZID present -> interpret wall-clock in that named zone.
/// - neither -> floating time, interpreted in the config tz `tz`.
fn resolve_instant(value: &str, tzid: Option<&str>, tz: &Tz) -> anyhow::Result<DateTime<Utc>> {
    let value = value.trim();
    if let Some(stripped) = value.strip_suffix('Z') {
        let naive = chrono::NaiveDateTime::parse_from_str(stripped, "%Y%m%dT%H%M%S")
            .map_err(|e| anyhow::anyhow!("bad UTC DATE-TIME {value:?}: {e}"))?;
        return Ok(Utc.from_utc_datetime(&naive));
    }

    let naive = chrono::NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S")
        .map_err(|e| anyhow::anyhow!("bad DATE-TIME {value:?}: {e}"))?;

    // Resolve the zone: a direct IANA name, a Windows/Outlook zone name ("Eastern
    // Standard Time"), or a fixed-offset pseudo-zone ("GMT+0200"); else the config
    // zone (floating time).
    if let Some(id) = tzid {
        let named: Option<Tz> = id
            .parse::<Tz>()
            .ok()
            .or_else(|| windows_to_iana(id).and_then(|n| n.parse::<Tz>().ok()));
        if let Some(zone) = named {
            let dt = zone
                .from_local_datetime(&naive)
                .earliest()
                .ok_or_else(|| anyhow::anyhow!("DATE-TIME {value:?} invalid in zone {zone:?}"))?;
            return Ok(dt.with_timezone(&Utc));
        }
        if let Some(off) = parse_fixed_offset(id) {
            let dt = off
                .from_local_datetime(&naive)
                .earliest()
                .ok_or_else(|| anyhow::anyhow!("DATE-TIME {value:?} invalid at offset {off:?}"))?;
            return Ok(dt.with_timezone(&Utc));
        }
        anyhow::bail!("unknown TZID {id:?}");
    }
    let dt = tz
        .from_local_datetime(&naive)
        .earliest()
        .ok_or_else(|| anyhow::anyhow!("DATE-TIME {value:?} invalid in zone {tz:?}"))?;
    Ok(dt.with_timezone(&Utc))
}

/// Map common Windows/Outlook time-zone names to IANA names. Microsoft calendar
/// feeds (Exchange/Outlook) emit these instead of IANA identifiers. Covers the
/// widely-used zones; unknown names fall through and the event is skipped.
fn windows_to_iana(name: &str) -> Option<&'static str> {
    Some(match name.trim() {
        "Dateline Standard Time" => "Etc/GMT+12",
        "Hawaiian Standard Time" => "Pacific/Honolulu",
        "Alaskan Standard Time" => "America/Anchorage",
        "Pacific Standard Time" | "Pacific Standard Time (Mexico)" => "America/Los_Angeles",
        "US Mountain Standard Time" => "America/Phoenix",
        "Mountain Standard Time" | "Mountain Standard Time (Mexico)" => "America/Denver",
        "Central Standard Time" | "Central Standard Time (Mexico)" => "America/Chicago",
        "Canada Central Standard Time" => "America/Regina",
        "US Eastern Standard Time" => "America/Indiana/Indianapolis",
        "Eastern Standard Time" | "Eastern Standard Time (Mexico)" => "America/New_York",
        "Atlantic Standard Time" => "America/Halifax",
        "Newfoundland Standard Time" => "America/St_Johns",
        "SA Pacific Standard Time" => "America/Bogota",
        "E. South America Standard Time" => "America/Sao_Paulo",
        "Argentina Standard Time" => "America/Argentina/Buenos_Aires",
        "Greenwich Standard Time" => "Atlantic/Reykjavik",
        "GMT Standard Time" => "Europe/London",
        "W. Europe Standard Time" => "Europe/Berlin",
        "Central Europe Standard Time" => "Europe/Budapest",
        "Romance Standard Time" => "Europe/Paris",
        "Central European Standard Time" => "Europe/Warsaw",
        "W. Central Africa Standard Time" => "Africa/Lagos",
        "GTB Standard Time" => "Europe/Bucharest",
        "E. Europe Standard Time" => "Europe/Chisinau",
        "South Africa Standard Time" => "Africa/Johannesburg",
        "FLE Standard Time" => "Europe/Kiev",
        "Israel Standard Time" => "Asia/Jerusalem",
        "Russian Standard Time" => "Europe/Moscow",
        "Arabian Standard Time" => "Asia/Dubai",
        "India Standard Time" => "Asia/Kolkata",
        "China Standard Time" => "Asia/Shanghai",
        "Singapore Standard Time" => "Asia/Singapore",
        "Tokyo Standard Time" => "Asia/Tokyo",
        "Korea Standard Time" => "Asia/Seoul",
        "AUS Eastern Standard Time" => "Australia/Sydney",
        "New Zealand Standard Time" => "Pacific/Auckland",
        "UTC" | "Coordinated Universal Time" => "Etc/UTC",
        _ => return None,
    })
}

/// Parse a fixed-offset pseudo-TZID like `GMT+0200`, `UTC-05:00`, or `GMT+2`.
/// iCloud emits these for events that have no named IANA zone.
fn parse_fixed_offset(tzid: &str) -> Option<FixedOffset> {
    let rest = tzid
        .trim()
        .strip_prefix("GMT")
        .or_else(|| tzid.trim().strip_prefix("UTC"))
        .unwrap_or_else(|| tzid.trim());
    let (sign, body) = match rest.strip_prefix('+') {
        Some(b) => (1, b),
        None => (-1, rest.strip_prefix('-')?),
    };
    let digits: String = body.chars().filter(|c| c.is_ascii_digit()).collect();
    let (h, m) = match digits.len() {
        1 | 2 => (digits.parse::<i32>().ok()?, 0),
        3 => (digits[..1].parse().ok()?, digits[1..].parse().ok()?),
        4 => (digits[..2].parse().ok()?, digits[2..].parse().ok()?),
        _ => return None,
    };
    if m >= 60 {
        return None;
    }
    FixedOffset::east_opt(sign * (h * 3600 + m * 60))
}

/// rrule 0.14 requires `UNTIL` to be UTC when DTSTART is UTC or carries a TZID.
/// Real feeds often give a date-only or floating `UNTIL`; rewrite it to a UTC
/// datetime so the rule parses.
fn normalize_until_utc(rrule_str: &str) -> String {
    rrule_str
        .split(';')
        .map(|part| {
            let p = part.trim();
            match p.find('=') {
                Some(eq) if p[..eq].eq_ignore_ascii_case("UNTIL") => {
                    let v = p[eq + 1..].trim().trim_end_matches(['Z', 'z']);
                    if v.len() == 8 {
                        format!("UNTIL={v}T000000Z")
                    } else {
                        format!("UNTIL={v}Z")
                    }
                }
                _ => p.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Expand an all-day recurring event into in-year dates.
///
/// To keep expansion independent of the machine's local timezone, we feed rrule
/// a midnight-UTC DTSTART and read each occurrence's UTC calendar date. For
/// date-based frequencies (YEARLY/MONTHLY/WEEKLY/DAILY at midnight) this yields
/// the intended calendar dates.
fn expand_all_day_rrule(
    start: NaiveDate,
    rrule_str: &str,
    year: i32,
) -> anyhow::Result<Vec<NaiveDate>> {
    let dtstart_line = format!("DTSTART:{}T000000Z", start.format("%Y%m%d"));
    let input = format!("{dtstart_line}\nRRULE:{}", normalize_until_utc(rrule_str));
    let set: RRuleSet = input
        .parse()
        .map_err(|e| anyhow::anyhow!("bad all-day RRULE {rrule_str:?}: {e}"))?;

    let (after, before) = year_bounds_utc(year);
    let result = set.after(after).before(before).all(RRULE_LIMIT);

    Ok(result
        .dates
        .into_iter()
        .map(|dt| dt.with_timezone(&Utc).date_naive())
        .filter(|d| d.year() == year)
        .collect())
}

/// Expand a timed recurring event into in-year absolute instants.
///
/// The recurrence is anchored at the resolved start instant, expressed in the
/// config tz so DST-aware wall-clock semantics match the user's calendar view.
fn expand_timed_rrule(
    start: DateTime<Utc>,
    rrule_str: &str,
    tz: &Tz,
    year: i32,
) -> anyhow::Result<Vec<DateTime<Utc>>> {
    let local = start.with_timezone(tz);
    // Emit DTSTART with an explicit TZID so rrule anchors in the right zone.
    let dtstart_line = format!(
        "DTSTART;TZID={}:{}",
        tz.name(),
        local.format("%Y%m%dT%H%M%S")
    );
    let input = format!("{dtstart_line}\nRRULE:{}", normalize_until_utc(rrule_str));
    let set: RRuleSet = input
        .parse()
        .map_err(|e| anyhow::anyhow!("bad timed RRULE {rrule_str:?}: {e}"))?;

    let (after, before) = year_bounds_rtz(year, tz);
    let result = set.after(after).before(before).all(RRULE_LIMIT);

    Ok(result
        .dates
        .into_iter()
        .map(|dt| dt.with_timezone(&Utc))
        .collect())
}

/// `[after, before]` bounds in UTC spanning the whole `year` with a one-day pad
/// either side, so day-shifting near year boundaries isn't clipped early.
fn year_bounds_utc(year: i32) -> (DateTime<RTz>, DateTime<RTz>) {
    let after = RTz::UTC
        .with_ymd_and_hms(year - 1, 12, 31, 0, 0, 0)
        .single()
        .expect("valid UTC bound");
    let before = RTz::UTC
        .with_ymd_and_hms(year + 1, 1, 2, 0, 0, 0)
        .single()
        .expect("valid UTC bound");
    (after, before)
}

/// Same as [`year_bounds_utc`] but expressed in the config zone, for timed rules.
fn year_bounds_rtz(year: i32, tz: &Tz) -> (DateTime<RTz>, DateTime<RTz>) {
    let rtz: RTz = (*tz).into();
    let after = rtz
        .with_ymd_and_hms(year - 1, 12, 31, 0, 0, 0)
        .single()
        .expect("valid zone bound");
    let before = rtz
        .with_ymd_and_hms(year + 1, 1, 2, 0, 0, 0)
        .single()
        .expect("valid zone bound");
    (after, before)
}

/// Sort: by date, then all-day before timed, then time, then title.
fn sort_occurrences(occs: &mut [EventOccurrence]) {
    occs.sort_by(|a, b| {
        a.date
            .cmp(&b.date)
            // all-day (time None) sorts before timed (time Some)
            .then(a.time.is_some().cmp(&b.time.is_some()))
            .then(a.time.cmp(&b.time))
            .then(a.title.cmp(&b.title))
    });
}

/// First property whose name matches `name` case-insensitively.
fn prop<'e>(event: &'e IcalEvent, name: &str) -> Option<&'e Property> {
    event
        .properties
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
}

/// First parameter value for `key` (case-insensitive) on a property.
fn param(p: &Property, key: &str) -> Option<String> {
    p.params.as_ref().and_then(|params| {
        params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .and_then(|(_, vals)| vals.first().cloned())
    })
}

/// A TEXT-typed property value, with iCal escaping decoded.
fn text_prop(event: &IcalEvent, name: &str) -> Option<String> {
    prop(event, name)
        .and_then(|p| p.value.as_deref())
        .map(unescape_text)
}

/// All ATTENDEE values, decoded (the common `mailto:` prefix is stripped).
fn attendees(event: &IcalEvent) -> Vec<String> {
    event
        .properties
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case("ATTENDEE"))
        .filter_map(|p| p.value.as_deref())
        .map(|v| {
            let v = unescape_text(v);
            v.strip_prefix("mailto:")
                .or_else(|| v.strip_prefix("MAILTO:"))
                .map(str::to_string)
                .unwrap_or(v)
        })
        .collect()
}

/// Decode RFC-5545 TEXT escaping: `\n`/`\N` -> newline, `\,` `\;` `\\` literals.
fn unescape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(',') => out.push(','),
                Some(';') => out.push(';'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}
