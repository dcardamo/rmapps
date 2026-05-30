//! Guards the per-day event pagination (`agenda::paginate_day`) and that a month
//! with events renders to a sane page layout: each day's events occupy their own
//! page(s), busy days spill onto continuation pages, and nothing errors. The old
//! fulgur text-overlap inspection is gone — Typst flows content into fresh pages
//! rather than overlapping, so page *counts* are the meaningful invariant now.

use std::collections::BTreeMap;

use chrono::{NaiveDate, NaiveTime};
use lopdf::Document;
use rmbujo::config::Config;
use rmbujo::ics::EventOccurrence;
use rmbujo::notebooks::month;

fn tmp(tag: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!("rmbujo-layout-{tag}-{n}.pdf"));
    p
}

fn pages(p: &std::path::Path) -> usize {
    Document::load(p).unwrap().get_pages().len()
}

fn ev(title: &str, hour: u32) -> EventOccurrence {
    EventOccurrence {
        date: NaiveDate::from_ymd_opt(2026, 5, 19).unwrap(),
        title: title.to_string(),
        time: Some(NaiveTime::from_hms_opt(hour, 0, 0).unwrap()),
        end_time: Some(NaiveTime::from_hms_opt(hour + 1, 0, 0).unwrap()),
        location: Some("Somewhere with a fairly long address line".to_string()),
        description: Some(
            "A description long enough to wrap across a couple of lines.".to_string(),
        ),
        attendees: vec!["alice@example.com".into(), "bob@example.com".into()],
        color: "accent".to_string(),
    }
}

#[test]
fn empty_month_has_no_event_pages() {
    // monthly view + tasks + one page per day (May has 31 days), no event pages.
    let cfg = Config::new(2026);
    let out = tmp("empty");
    month::build_month_pdf(&cfg, 5, &BTreeMap::new(), &out).unwrap();
    assert_eq!(pages(&out), 2 + 31);
}

#[test]
fn one_event_day_adds_one_event_page() {
    let mut events: BTreeMap<NaiveDate, Vec<EventOccurrence>> = BTreeMap::new();
    events.insert(
        NaiveDate::from_ymd_opt(2026, 5, 19).unwrap(),
        vec![ev("Dentist", 14)],
    );
    let out = tmp("one");
    month::build_month_pdf(&Config::new(2026), 5, &events, &out).unwrap();
    // 2 chrome + 31 daily + exactly one event page for the single busy day.
    assert_eq!(pages(&out), 2 + 31 + 1);
}

#[test]
fn busy_day_spills_onto_continuation_pages() {
    // Many full-detail events on one day must paginate onto more than one page,
    // and never overflow off the page (Typst would otherwise just keep flowing).
    let mut events: BTreeMap<NaiveDate, Vec<EventOccurrence>> = BTreeMap::new();
    let day: Vec<EventOccurrence> = (8..22).map(|h| ev("Packed meeting", h)).collect();
    events.insert(NaiveDate::from_ymd_opt(2026, 5, 19).unwrap(), day);
    let out = tmp("busy");
    month::build_month_pdf(&Config::new(2026), 5, &events, &out).unwrap();
    // 2 chrome + 31 daily + >= 2 event pages (the day spilled).
    assert!(
        pages(&out) >= 2 + 31 + 2,
        "busy day should spill onto continuation pages, got {} pages",
        pages(&out)
    );
}
