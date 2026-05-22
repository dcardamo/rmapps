use std::collections::HashMap;

use chrono::NaiveDate;
use rmbujo::config::{Config, IcsFeed};
use rmbujo::ics::build_event_map;
use rmbujo::ics::fetch::Fetcher;

fn tmp_dir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let mut p = std::env::temp_dir();
    p.push(format!(
        "rmbujo-icsmod-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

struct MapFetcher {
    ok: HashMap<String, Vec<u8>>,
}
impl Fetcher for MapFetcher {
    fn get(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        self.ok
            .get(url)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no route for {url}"))
    }
}

const ALLDAY: &str = "BEGIN:VCALENDAR\nVERSION:2.0\nPRODID:-//t//EN\nBEGIN:VEVENT\nUID:a@t\nDTSTART;VALUE=DATE:20270519\nSUMMARY:Holiday\nEND:VEVENT\nEND:VCALENDAR\n";
const TIMED: &str = "BEGIN:VCALENDAR\nVERSION:2.0\nPRODID:-//t//EN\nBEGIN:VEVENT\nUID:b@t\nDTSTART;TZID=America/Toronto:20270519T090000\nSUMMARY:Standup\nEND:VEVENT\nEND:VCALENDAR\n";

fn cfg(feeds: Vec<IcsFeed>) -> Config {
    Config {
        ics: feeds,
        timezone: "America/Toronto".into(),
        ..Config::new(2027)
    }
}

#[test]
fn merges_and_orders_all_day_before_timed() {
    let dir = tmp_dir();
    let feeds = vec![
        IcsFeed {
            name: "T".into(),
            url: "u-timed".into(),
            color: "olive".into(),
        },
        IcsFeed {
            name: "H".into(),
            url: "u-allday".into(),
            color: "brick".into(),
        },
    ];
    let mut ok = HashMap::new();
    ok.insert("u-timed".to_string(), TIMED.as_bytes().to_vec());
    ok.insert("u-allday".to_string(), ALLDAY.as_bytes().to_vec());
    let map = build_event_map(&cfg(feeds), &dir, true, &MapFetcher { ok }).unwrap();
    let day = map
        .get(&NaiveDate::from_ymd_opt(2027, 5, 19).unwrap())
        .unwrap();
    assert_eq!(day.len(), 2);
    assert!(day[0].time.is_none(), "all-day first");
    assert!(day[1].time.is_some(), "timed second");
}

#[test]
fn failing_feed_skipped_not_fatal() {
    let dir = tmp_dir();
    let feeds = vec![
        IcsFeed {
            name: "Good".into(),
            url: "u-allday".into(),
            color: "brick".into(),
        },
        IcsFeed {
            name: "Bad".into(),
            url: "u-missing".into(),
            color: "navy".into(),
        },
    ];
    let mut ok = HashMap::new();
    ok.insert("u-allday".to_string(), ALLDAY.as_bytes().to_vec());
    let map = build_event_map(&cfg(feeds), &dir, true, &MapFetcher { ok }).unwrap();
    assert!(map.contains_key(&NaiveDate::from_ymd_opt(2027, 5, 19).unwrap()));
}
