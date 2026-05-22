use askama::Template;
use rmbujo::calendar::build_month;
use rmbujo::templates::{
    Agenda, AgendaDay, AgendaEvent, Cover, DayRow, Details, FutureLog, MonthlyView, Reference,
    Tasks,
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

/// Two days, three events total: one with a location+notes, one bare, one timed.
fn sample_agenda_days() -> Vec<AgendaDay> {
    vec![
        AgendaDay {
            day: 19,
            weekday: "Wed",
            events: vec![
                AgendaEvent {
                    idx: 0,
                    label: "All Day".into(),
                    end_label: None,
                    title: "Victoria Day".into(),
                    location: None,
                    description: None,
                    attendees: vec![],
                    color: "accent".into(),
                    is_all_day: true,
                },
                AgendaEvent {
                    idx: 1,
                    label: "14:00".into(),
                    end_label: Some("15:00".into()),
                    title: "Dentist".into(),
                    location: Some("Downtown".into()),
                    description: Some("Bring insurance card".into()),
                    attendees: vec!["Dr. Lee".into()],
                    color: "rust".into(),
                    is_all_day: false,
                },
            ],
        },
        AgendaDay {
            day: 24,
            weekday: "Mon",
            events: vec![AgendaEvent {
                idx: 2,
                label: "09:00".into(),
                end_label: Some("10:30".into()),
                title: "Flight".into(),
                location: None,
                description: None,
                attendees: vec![],
                color: "primary".into(),
                is_all_day: false,
            }],
        },
    ]
}

#[test]
fn agenda_links_and_swatches() {
    let days = sample_agenda_days();
    let html = Agenda {
        month_name: "May",
        year: 2027,
        days: &days,
    }
    .render()
    .unwrap();
    // Title "2027 May" links back to the monthly view.
    assert!(html.contains("href=\"#monthly\""));
    assert!(html.contains("2027 May"));
    // Two date headers -> #day-N, two day blocks with #agenda-N anchors.
    assert_eq!(html.matches("href=\"#day-19\"").count(), 1);
    assert_eq!(html.matches("href=\"#day-24\"").count(), 1);
    assert!(html.contains("id=\"agenda-19\""));
    assert!(html.contains("id=\"agenda-24\""));
    // Three events -> three #evt-K links and three swatches.
    assert_eq!(html.matches("href=\"#evt-").count(), 3);
    assert_eq!(html.matches("class=\"swatch\"").count(), 3);
    // Color swatch resolves a theme var.
    assert!(html.contains("var(--accent)"));
    // Location appended on the agenda line.
    assert!(html.contains("Downtown"));
}

#[test]
fn details_omit_empty_fields() {
    let days = sample_agenda_days();
    let html = Details {
        month_name: "May",
        year: 2027,
        days: &days,
    }
    .render()
    .unwrap();
    assert!(html.contains("href=\"#monthly\""));
    // Three event blocks with stable ids.
    assert!(html.contains("id=\"evt-0\""));
    assert!(html.contains("id=\"evt-1\""));
    assert!(html.contains("id=\"evt-2\""));
    // The populated event shows Where/Notes/Who exactly once each.
    assert_eq!(html.matches("Where:").count(), 1);
    assert_eq!(html.matches("Notes:").count(), 1);
    assert_eq!(html.matches("Who:").count(), 1);
    assert!(html.contains("Bring insurance card"));
    assert!(html.contains("Dr. Lee"));
    // Timed events show a start–end range (en dash); all-day shows no range.
    assert!(html.contains("14:00&#8211;15:00"));
    assert!(html.contains("09:00&#8211;10:30"));
    // The bare all-day event must not emit empty meta lines.
    assert!(html.contains("Victoria Day"));
}
