use lopdf::Document;
use rmbujo::config::Config;
use rmbujo::notebooks::month::build_month_pdf;

fn tmp() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!("rmbujo-month-{n}.pdf"));
    p
}

#[test]
fn month_page_count_static() {
    let cfg = Config {
        pages_per_day: 1,
        ..Config::new(2027)
    };
    let out = tmp();
    build_month_pdf(&cfg, 5, &std::collections::BTreeMap::new(), &out).unwrap();
    let doc = Document::load(&out).unwrap();
    // monthly view + tasks + 31 daily pages (May has 31 days)
    assert_eq!(doc.get_pages().len(), 2 + 31);
}

#[test]
fn month_pages_per_day_multiplies_daily() {
    let cfg = Config {
        pages_per_day: 2,
        ..Config::new(2027)
    };
    let out = tmp();
    // Feb 2027 = 28 days
    build_month_pdf(&cfg, 2, &std::collections::BTreeMap::new(), &out).unwrap();
    let doc = Document::load(&out).unwrap();
    assert_eq!(doc.get_pages().len(), 2 + 28 * 2);
}

#[test]
fn events_only_add_trailing_pages() {
    use std::collections::BTreeMap;
    let cfg = Config {
        pages_per_day: 1,
        ..Config::new(2027)
    };
    let empty = BTreeMap::new();
    let out_a = tmp();
    build_month_pdf(&cfg, 5, &empty, &out_a).unwrap();
    let base = lopdf::Document::load(&out_a).unwrap().get_pages().len();
    assert_eq!(base, 2 + 31);

    let mut ev = BTreeMap::new();
    ev.insert(
        chrono::NaiveDate::from_ymd_opt(2027, 5, 19).unwrap(),
        vec![rmbujo::ics::EventOccurrence {
            date: chrono::NaiveDate::from_ymd_opt(2027, 5, 19).unwrap(),
            time: None,
            end_time: None,
            title: "Holiday".into(),
            location: None,
            description: None,
            attendees: vec![],
            color: "brick".into(),
        }],
    );
    let out_b = tmp();
    build_month_pdf(&cfg, 5, &ev, &out_b).unwrap();
    let withev = lopdf::Document::load(&out_b).unwrap().get_pages().len();
    assert!(withev > base, "events add trailing pages");
    assert_eq!(
        withev,
        base + 2,
        "1 agenda + 1 details page for a small event set"
    );
}

#[test]
fn busy_month_paginates_agenda_and_details() {
    use std::collections::BTreeMap;
    let cfg = Config {
        pages_per_day: 1,
        ..Config::new(2027)
    };
    // 28 days, 3 events each -> agenda + details each overflow a single page and
    // must paginate (the bug was a single clipped section).
    let mut ev: BTreeMap<chrono::NaiveDate, Vec<rmbujo::ics::EventOccurrence>> = BTreeMap::new();
    for day in 1..=28u32 {
        let d = chrono::NaiveDate::from_ymd_opt(2027, 1, day).unwrap();
        let evs = (0..3)
            .map(|i| rmbujo::ics::EventOccurrence {
                date: d,
                time: None,
                end_time: None,
                title: format!("Event {i}"),
                location: Some("Somewhere".into()),
                description: None,
                attendees: vec![],
                color: "brick".into(),
            })
            .collect();
        ev.insert(d, evs);
    }
    let out = tmp();
    build_month_pdf(&cfg, 1, &ev, &out).unwrap();
    let pages = lopdf::Document::load(&out).unwrap().get_pages().len();
    // Jan static = 2 + 31 = 33; agenda and details each span multiple pages.
    assert!(
        pages > 33 + 2,
        "busy month should paginate agenda+details beyond one page each (got {pages})"
    );
}
