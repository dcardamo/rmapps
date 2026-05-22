use askama::Template;
use rmbujo::calendar::build_month;
use rmbujo::templates::{
    Cover, DayRow, DayView, FutureLog, MonthIndex, MonthlyView, Reference, Tasks,
};

#[test]
fn month_index_rows() {
    let m = build_month(2026, 5, "sun").unwrap();
    let days: Vec<DayView> = m
        .days
        .iter()
        .map(|d| DayView {
            day: d.day,
            weekday: d.weekday,
            week_start: d.week_start,
        })
        .collect();
    let html = MonthIndex {
        month_name: "May",
        year: 2026,
        days: &days,
    }
    .render()
    .unwrap();
    assert_eq!(html.matches("class=\"day").count(), 31);
    assert!(html.contains("weekstart"));
    assert!(html.contains(">18<") && html.contains("Mon"));
    // Month index sits on the dot grid (consistent with daily pages), not ruled rows.
    // Dots are anchored to the day list so labels land between dot rows.
    assert!(html.contains("month-index"));
    assert!(html.contains("month-list"));
    assert!(!html.contains("gutter"));
}

#[test]
fn cover_blank_vs_titled() {
    let blank = Cover {
        year: 2026,
        title: "",
        blank_title: true,
    }
    .render()
    .unwrap();
    assert!(blank.contains("title-blank"));
    assert!(!blank.contains("class=\"title\""));
    let titled = Cover {
        year: 2026,
        title: "Reference",
        blank_title: false,
    }
    .render()
    .unwrap();
    assert!(titled.contains("Reference"));
}

#[test]
fn tasks_uses_dot_page() {
    let html = Tasks.render().unwrap();
    // Background dot grid (renders reliably even as a single page), not the old
    // absolutely-positioned overlay that collapsed when rendered in isolation.
    assert!(html.contains("dotpage"));
    assert!(!html.contains("dotgrid--below"));
    assert!(html.contains("Tasks"));
}

#[test]
fn future_log_blocks() {
    let html = FutureLog {
        months: &["January", "February", "March"],
    }
    .render()
    .unwrap();
    assert_eq!(html.matches("fl-block").count(), 3);
    assert!(html.contains("February"));
}

#[test]
fn monthly_view_rows_and_links() {
    let days: Vec<DayRow> = (1..=31)
        .map(|day| DayRow {
            day,
            weekday: "Mon",
            week_start: false,
            event_count: 0,
        })
        .collect();
    let html = MonthlyView {
        month_name: "May",
        year: 2027,
        month_num: 5,
        row_pt: 12.0,
        days: &days,
    }
    .render()
    .unwrap();
    assert_eq!(html.matches("href=\"#day-").count(), 31);
    assert!(html.contains("id=\"monthly\""));
    assert!(!html.contains("cbadge"));
}

#[test]
fn reference_legend() {
    let html = Reference.render().unwrap();
    assert!(html.contains("Feeling / mood"));
    assert_eq!(html.matches("class=\"page\"").count(), 2);
}
