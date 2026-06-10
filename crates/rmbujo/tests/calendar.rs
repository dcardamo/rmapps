use rmbujo::calendar::{build_month, build_year, segments, MONTH_NAMES};

#[test]
fn may_2026_basics() {
    let m = build_month(2026, 5, "sun").unwrap();
    assert_eq!(m.name, "May");
    assert_eq!(m.days.len(), 31);
    assert_eq!(m.days[17].day, 18);
    assert_eq!(m.days[17].weekday, "Mon");
}

#[test]
fn february_leap() {
    assert_eq!(build_month(2024, 2, "sun").unwrap().days.len(), 29);
    assert_eq!(build_month(2026, 2, "sun").unwrap().days.len(), 28);
}

#[test]
fn week_start_sunday() {
    let m = build_month(2026, 5, "sun").unwrap();
    let starts: Vec<u32> = m
        .days
        .iter()
        .filter(|d| d.week_start)
        .map(|d| d.day)
        .collect();
    assert_eq!(starts, vec![3, 10, 17, 24, 31]);
}

#[test]
fn week_start_monday() {
    let m = build_month(2026, 5, "mon").unwrap();
    let starts: Vec<u32> = m
        .days
        .iter()
        .filter(|d| d.week_start)
        .map(|d| d.day)
        .collect();
    assert_eq!(starts, vec![4, 11, 18, 25]);
}

#[test]
fn year_has_12_months() {
    let y = build_year(2026, "sun").unwrap();
    assert_eq!(y.len(), 12);
    assert_eq!(MONTH_NAMES[5], "May");
}

#[test]
fn bad_week_start_errors() {
    assert!(build_month(2026, 5, "xyz").is_err());
}

#[test]
fn days_in_month_counts() {
    assert_eq!(rmbujo::calendar::days_in_month(2026, 2), 28);
    assert_eq!(rmbujo::calendar::days_in_month(2024, 2), 29);
    assert_eq!(rmbujo::calendar::days_in_month(2026, 5), 31);
}

#[test]
fn segments_may_2026_sunday() {
    // Sunday week-starts in May 2026 = 3,10,17,24,31; plus day-1 boundary.
    let m = build_month(2026, 5, "sun").unwrap();
    let segs = segments(&m);
    let firsts: Vec<u32> = segs.iter().map(|s| s.first_day()).collect();
    assert_eq!(firsts, vec![1, 3, 10, 17, 24, 31]);
    // Every day covered exactly once, in order.
    let flat: Vec<u32> = segs.iter().flat_map(|s| s.days.iter().map(|d| d.day)).collect();
    assert_eq!(flat, (1..=31).collect::<Vec<_>>());
    // First segment is the partial lead-in (days 1..=2).
    assert_eq!(segs[0].first_day(), 1);
    assert_eq!(segs[0].last_day(), 2);
    // Last segment is a single day (the 31st).
    assert_eq!(segs.last().unwrap().first_day(), 31);
    assert_eq!(segs.last().unwrap().last_day(), 31);
}

#[test]
fn segments_may_2026_monday() {
    // Monday week-starts in May 2026 = 4,11,18,25; plus day-1 boundary.
    let m = build_month(2026, 5, "mon").unwrap();
    let firsts: Vec<u32> = segments(&m).iter().map(|s| s.first_day()).collect();
    assert_eq!(firsts, vec![1, 4, 11, 18, 25]);
}

#[test]
fn segment_date_range_padded() {
    let m = build_month(2026, 5, "mon").unwrap();
    let segs = segments(&m);
    // Segment starting on the 4th runs 4..=10 (Mon..Sun).
    let s = segs.iter().find(|s| s.first_day() == 4).unwrap();
    assert_eq!(s.last_day(), 10);
    assert_eq!(s.date_range(5), "05.04 – 05.10");
    assert_eq!(s.id(), 4);
    // Single-day last segment renders a same-day range.
    let last = segs.last().unwrap();
    assert_eq!(last.date_range(5), format!("05.{:02} – 05.{:02}", last.first_day(), last.last_day()));
}
