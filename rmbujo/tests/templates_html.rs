use askama::Template;
use rmbujo::calendar::build_month;
use rmbujo::templates::{
    AgendaEvent, Cover, DayEvents, DayRow, FutureLog, MonthlyView, Reference, Tasks,
};

#[test]
fn monthly_view_day_count() {
    let m = build_month(2026, 5, "sun").unwrap();
    let days: Vec<DayRow> = m
        .days
        .iter()
        .map(|d| DayRow {
            day: d.day,
            weekday: d.weekday,
            week_start: d.week_start,
            event_count: 0,
        })
        .collect();
    let html = MonthlyView {
        month_name: "May",
        year: 2026,
        month_num: 5,
        row_pt: 12.0,
        days: &days,
    }
    .render()
    .unwrap();
    assert_eq!(html.matches("href=\"#day-").count(), 31);
    assert!(html.contains(">18<") && html.contains("Mon"));
    assert!(html.contains("id=\"monthly\""));
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

fn sample_day_events() -> Vec<AgendaEvent> {
    vec![
        AgendaEvent {
            idx: 0,
            label: "09:00".into(),
            end_label: Some("10:00".into()),
            title: "Standup".into(),
            location: Some("Zoom".into()),
            description: None,
            attendees: vec![],
            color: "accent".into(),
            is_all_day: false,
        },
        AgendaEvent {
            idx: 1,
            label: "12:00".into(),
            end_label: None,
            title: "Lunch".into(),
            location: None,
            description: None,
            attendees: vec!["Sam".into()],
            color: "rust".into(),
            is_all_day: false,
        },
    ]
}

#[test]
fn day_events_first_page_anchor_and_links() {
    let evs = sample_day_events();
    let html = DayEvents {
        month_num: 5,
        day: 5,
        day_pad: "05".into(),
        weekday: "Wed",
        agenda: &evs,
        details: &evs,
        show_agenda_heading: true,
        show_details_heading: true,
        continued: false,
        first_page: true,
    }
    .render()
    .unwrap();
    assert!(
        html.contains("id=\"agenda-5\""),
        "pill target on first page"
    );
    assert!(
        html.contains("href=\"#day-5\""),
        "header links to daily page"
    );
    assert_eq!(
        html.matches("href=\"#evt-").count(),
        2,
        "agenda lines link to details"
    );
    assert!(html.contains("id=\"evt-0\"") && html.contains("id=\"evt-1\""));
    assert!(html.contains(">Agenda<") && html.contains(">Details<"));
    assert_eq!(
        html.matches("class=\"swatch\"").count(),
        4,
        "2 agenda + 2 detail swatches"
    );
    assert!(html.contains("var(--accent)"));
    assert!(
        html.contains("09:00&#8211;10:00"),
        "details show start-end range"
    );
    // Empty fields omitted; only the populated meta lines render.
    assert!(html.contains("Where: Zoom"));
    assert!(html.contains("Who: Sam"));
    assert_eq!(html.matches("Notes:").count(), 0);
}

#[test]
fn day_events_continuation_omits_anchor_and_headings() {
    let evs = sample_day_events();
    let html = DayEvents {
        month_num: 5,
        day: 5,
        day_pad: "05".into(),
        weekday: "Wed",
        agenda: &[],
        details: &evs,
        show_agenda_heading: false,
        show_details_heading: false,
        continued: true,
        first_page: false,
    }
    .render()
    .unwrap();
    assert!(
        !html.contains("id=\"agenda-5\""),
        "anchor only on first page"
    );
    assert!(html.contains("cont."), "continuation marker");
    assert!(!html.contains(">Agenda<") && !html.contains(">Details<"));
    assert!(
        html.contains("href=\"#day-5\""),
        "running header still links to daily page"
    );
}
