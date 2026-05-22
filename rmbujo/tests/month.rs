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
    // Jan static = 2 + 31 = 33; 28 busy days each produce 1+ combined pages, so the
    // event pages add well beyond a single trailing page.
    assert!(
        pages > 33 + 1,
        "busy month should paginate per-day event pages beyond one page (got {pages})"
    );
}

#[test]
fn paginate_day_small_day_one_page() {
    use rmbujo::notebooks::month::agenda::paginate_day;
    use rmbujo::templates::{AgendaDay, AgendaEvent};
    let mk = |i| AgendaEvent {
        idx: i,
        label: "09:00".into(),
        end_label: None,
        title: "Event".into(),
        location: None,
        description: None,
        attendees: vec![],
        color: "cal1".into(),
        is_all_day: false,
    };
    let day = AgendaDay {
        day: 5,
        weekday: "Wed",
        events: (0..2).map(mk).collect(),
    };
    // header 20 + agenda(subhead 5 + 2*10) + details(subhead 5 + 2*10) = 70 <= 200
    let pages = paginate_day(&day, 200.0, 20.0, 5.0, |_| 10.0, |_| 10.0);
    assert_eq!(pages.len(), 1, "small day fits on one page");
    let p = &pages[0];
    assert!(p.first_page && !p.continued);
    assert!(p.show_agenda_heading && p.show_details_heading);
    assert_eq!(p.agenda.len(), 2);
    assert_eq!(p.details.len(), 2);
}

#[test]
fn paginate_day_agenda_overflows_then_details() {
    use rmbujo::notebooks::month::agenda::paginate_day;
    use rmbujo::templates::{AgendaDay, AgendaEvent};
    let mk = |i| AgendaEvent {
        idx: i,
        label: "09:00".into(),
        end_label: None,
        title: "Event".into(),
        location: None,
        description: None,
        attendees: vec![],
        color: "cal1".into(),
        is_all_day: false,
    };
    let day = AgendaDay {
        day: 5,
        weekday: "Wed",
        events: (0..3).map(mk).collect(),
    };
    // header 20, subhead 0, each item 40, usable 100. Numbers below are the
    // running page height *after* placing each item (header included).
    // p0: a0->60, a1->100 (full); a2 needs 40 -> 140>100 flush.
    // p1: a2->60, d0->100 (full); d1 needs 40 -> flush. p2: d1->60, d2->100.
    let pages = paginate_day(&day, 100.0, 20.0, 0.0, |_| 40.0, |_| 40.0);
    assert_eq!(pages.len(), 3);
    assert!(pages[0].first_page && pages[0].show_agenda_heading);
    assert_eq!(pages[0].agenda.len(), 2);
    assert!(pages[0].details.is_empty());
    assert!(pages[1].continued && pages[1].show_details_heading);
    assert_eq!(pages[1].agenda.len(), 1);
    assert_eq!(pages[1].details.len(), 1);
    assert!(!pages[1].show_agenda_heading);
    assert_eq!(pages[2].details.len(), 2);
    assert!(!pages[2].show_agenda_heading && !pages[2].show_details_heading);
    let total: usize = pages.iter().map(|p| p.agenda.len() + p.details.len()).sum();
    assert_eq!(total, 6, "no events lost (3 agenda + 3 detail)");
}

#[test]
fn paginate_day_details_heading_not_orphaned() {
    use rmbujo::notebooks::month::agenda::paginate_day;
    use rmbujo::templates::{AgendaDay, AgendaEvent};
    let mk = |i| AgendaEvent {
        idx: i,
        label: "09:00".into(),
        end_label: None,
        title: "Event".into(),
        location: None,
        description: None,
        attendees: vec![],
        color: "cal1".into(),
        is_all_day: false,
    };
    let day = AgendaDay {
        day: 5,
        weekday: "Wed",
        events: (0..2).map(mk).collect(),
    };
    // header 20, subhead 20, each item 40, usable 100.
    // p0: header20 + agenda-head20 + a0(40)=80; a1 needs 40 -> 120>100 flush.
    // p1: header20 + a1(40)=60; d0 needs head20+40=60 -> 120>100 flush (no orphan head).
    // p2: header20 + details-head20 + d0(40)=80; d1 needs 40 -> flush. p3: d1.
    let pages = paginate_day(&day, 100.0, 20.0, 20.0, |_| 40.0, |_| 40.0);
    // The details heading lands on the page that holds the first detail item.
    let det_page = pages.iter().find(|p| !p.details.is_empty()).unwrap();
    assert!(det_page.show_details_heading);
    assert_eq!(
        det_page.details[0].idx, 0,
        "heading travels with its first item"
    );
    // Exactly one page shows the details heading.
    assert_eq!(pages.iter().filter(|p| p.show_details_heading).count(), 1);
    assert_eq!(pages.iter().filter(|p| p.show_agenda_heading).count(), 1);
}

#[test]
fn paginate_day_oversized_lone_item_placed() {
    use rmbujo::notebooks::month::agenda::paginate_day;
    use rmbujo::templates::{AgendaDay, AgendaEvent};
    let mk = |i| AgendaEvent {
        idx: i,
        label: "09:00".into(),
        end_label: None,
        title: "Event".into(),
        location: None,
        description: None,
        attendees: vec![],
        color: "cal1".into(),
        is_all_day: false,
    };
    let day = AgendaDay {
        day: 5,
        weekday: "Wed",
        events: (0..1).map(mk).collect(),
    };
    // One agenda item far taller than the page: must be placed, not dropped/looped.
    // The oversized item stays alone on p0; the detail item flushes onto p1.
    let pages = paginate_day(&day, 100.0, 20.0, 5.0, |_| 500.0, |_| 10.0);
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].agenda.len(), 1);
    assert!(pages[0].details.is_empty());
    assert_eq!(pages.iter().map(|p| p.agenda.len()).sum::<usize>(), 1);
    assert_eq!(pages.iter().map(|p| p.details.len()).sum::<usize>(), 1);
}
