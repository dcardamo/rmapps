use rmbujo::calendar::{build_month, build_year, MONTH_NAMES};

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
