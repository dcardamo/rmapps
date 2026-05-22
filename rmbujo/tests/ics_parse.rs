use chrono::{Datelike, NaiveDate, NaiveTime};
use rmbujo::ics::{parse::parse_feed, EventOccurrence};

fn parse(file: &str, tz: &str) -> Vec<EventOccurrence> {
    let bytes = std::fs::read(format!("tests/fixtures/{file}")).unwrap();
    parse_feed(&bytes, "brick", 2027, &tz.parse().unwrap()).unwrap()
}

#[test]
fn all_day_dated_multiday_rrule_yearclip() {
    let evs = parse("holidays.ics", "America/Toronto");
    assert!(evs.iter().all(|e| e.time.is_none()));
    assert!(evs.iter().all(|e| e.date.year() == 2027)); // out-of-year excluded
    let on = |m, d| {
        evs.iter()
            .filter(|e| e.date == NaiveDate::from_ymd_opt(2027, m, d).unwrap())
            .count()
    };
    assert_eq!(on(5, 19), 1); // dated
    assert_eq!(on(5, 24) + on(5, 25) + on(5, 26), 3); // 3-day span (DTEND 27 exclusive)
    assert_eq!(on(5, 27), 0); // DTEND exclusive
    assert!(evs
        .iter()
        .any(|e| e.title.contains("birthday")
            && e.date == NaiveDate::from_ymd_opt(2027, 1, 15).unwrap())); // RRULE
    assert!(evs.iter().all(|e| e.color == "brick"));
}

#[test]
fn timed_convert_to_config_tz_with_day_shift() {
    let evs = parse("timed.ics", "America/Toronto");
    assert!(evs.iter().any(|e| e.title == "Dentist"
        && e.time == Some(NaiveTime::from_hms_opt(14, 0, 0).unwrap())
        && e.date == NaiveDate::from_ymd_opt(2027, 5, 19).unwrap()
        && e.location.as_deref() == Some("Downtown")));
    // UTC 2027-05-20T03:00Z -> 2027-05-19 23:00 local
    assert!(evs.iter().any(|e| e.title == "Late call"
        && e.date == NaiveDate::from_ymd_opt(2027, 5, 19).unwrap()
        && e.time == Some(NaiveTime::from_hms_opt(23, 0, 0).unwrap())));
}
