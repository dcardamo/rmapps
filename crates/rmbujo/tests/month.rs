use lopdf::Document;
use rmbujo::config::Config;
use rmbujo::notebooks::month::build_month_pdf;

fn weekly_pages(year: i32, month: u32, week_start: &str) -> usize {
    let m = rmbujo::calendar::build_month(year, month, week_start).unwrap();
    rmbujo::calendar::segments(&m).len() * 2
}

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
    // monthly view + tasks + 31 daily pages (May has 31 days) + 2 pages per week segment
    let weekly = weekly_pages(2027, 5, "sun");
    assert_eq!(doc.get_pages().len(), 2 + 31 + weekly);
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
    let weekly = weekly_pages(2027, 2, "sun");
    assert_eq!(doc.get_pages().len(), 2 + 28 * 2 + weekly);
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
    let weekly = weekly_pages(2027, 5, "sun");
    let base = lopdf::Document::load(&out_a).unwrap().get_pages().len();
    assert_eq!(base, 2 + 31 + weekly);

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
        base + 1,
        "a small one-event day is a single combined agenda+details page"
    );
}

#[test]
fn busy_month_paginates_per_day_pages() {
    use std::collections::BTreeMap;
    let cfg = Config {
        pages_per_day: 1,
        ..Config::new(2027)
    };
    // 28 busy days each produce one or more combined per-day event pages, so the
    // month paginates well beyond a single trailing page.
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
    // Jan static = 2 + 31 + weekly; 28 busy days each produce 1+ combined pages, so
    // the event pages add well beyond a single trailing page.
    let weekly = weekly_pages(2027, 1, "sun");
    let base = 2 + 31 + weekly;
    assert!(
        pages > base + 1,
        "busy month should paginate per-day event pages beyond one page (got {pages}, base {base})"
    );
}

fn mk_event(title: &str) -> rmbujo::templates::AgendaEvent {
    rmbujo::templates::AgendaEvent {
        label: "09:00".into(),
        end_label: None,
        title: title.into(),
        location: None,
        description: None,
        attendees: vec![],
        color: "cal1".into(),
        is_all_day: false,
    }
}

#[test]
fn paginate_day_small_day_one_page() {
    use rmbujo::notebooks::month::agenda::paginate_day;
    use rmbujo::templates::AgendaDay;
    let day = AgendaDay {
        day: 5,
        weekday: "Wed",
        events: (0..2).map(|_| mk_event("Event")).collect(),
    };
    // header 20 + 2 events * 10 = 40 <= 200 -> a single page.
    let pages = paginate_day(&day, 200.0, 20.0, |_| 10.0);
    assert_eq!(pages.len(), 1, "small day fits on one page");
    let p = &pages[0];
    assert!(p.first_page && !p.continued);
    assert_eq!(p.events.len(), 2);
}

#[test]
fn paginate_day_overflows_across_pages() {
    use rmbujo::notebooks::month::agenda::paginate_day;
    use rmbujo::templates::AgendaDay;
    let day = AgendaDay {
        day: 5,
        weekday: "Wed",
        events: (0..5).map(|_| mk_event("Event")).collect(),
    };
    // header 20, each event 40, usable 100. Running page height after each placement:
    // p0: e0->60, e1->100 (full); e2 needs 40 -> 140>100 flush.
    // p1: e2->60, e3->100 (full); e4 needs 40 -> flush. p2: e4->60.
    let pages = paginate_day(&day, 100.0, 20.0, |_| 40.0);
    assert_eq!(pages.len(), 3);
    assert!(pages[0].first_page && !pages[0].continued);
    assert_eq!(pages[0].events.len(), 2);
    assert!(pages[1].continued && !pages[1].first_page);
    assert_eq!(pages[1].events.len(), 2);
    assert!(pages[2].continued);
    assert_eq!(pages[2].events.len(), 1);
    let total: usize = pages.iter().map(|p| p.events.len()).sum();
    assert_eq!(total, 5, "no events lost when splitting");
}

#[test]
fn paginate_day_oversized_lone_event_placed() {
    use rmbujo::notebooks::month::agenda::paginate_day;
    use rmbujo::templates::AgendaDay;
    let day = AgendaDay {
        day: 5,
        weekday: "Wed",
        events: vec![mk_event("BIG"), mk_event("small")],
    };
    // First event far taller than a page: must be placed alone (not dropped/looped),
    // the second flushes onto p1.
    let pages = paginate_day(&day, 100.0, 20.0, |e| {
        if e.title == "BIG" {
            500.0
        } else {
            40.0
        }
    });
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].events.len(), 1, "oversized event placed alone");
    assert_eq!(pages[0].events[0].title, "BIG");
    assert_eq!(pages[1].events.len(), 1);
}
