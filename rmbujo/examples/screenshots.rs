//! Generates the README screenshots (docs/images/*.png) from richer mock data.
//!
//! Run with: `nix develop -c cargo run --example screenshots`
//! Requires `pdftoppm` (poppler) on PATH, same as the visual-regression tests.
//!
//! Calendar data here is entirely fictional — it is NOT anyone's real calendar.

use std::path::{Path, PathBuf};
use std::process::Command;

use askama::Template;
use rmbujo::calendar::build_month;
use rmbujo::device::get_device;
use rmbujo::geometry::{default_grid, monthly_row_pt, TOOLBAR_SAFE_PT};
use rmbujo::render::render_pdf;
use rmbujo::templates::{AgendaEvent, Cover, DayEvents, DayRow, FutureLog, MonthlyView, Reference};
use rmbujo::theme::load_theme;

fn out_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/images")
}

fn render_png(fragment: &str, png: &Path) {
    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let theme = load_theme("library").unwrap();
    let mut pdf = std::env::temp_dir();
    pdf.push("rmbujo-shot.pdf");
    render_pdf(&dev, &grid, &theme, &[fragment.to_string()], &pdf).unwrap();

    let prefix = png.with_extension("");
    let status = Command::new("pdftoppm")
        .args([
            "-png",
            "-r",
            "150",
            "-singlefile",
            pdf.to_str().unwrap(),
            prefix.to_str().unwrap(),
        ])
        .status()
        .expect("pdftoppm");
    assert!(status.success(), "pdftoppm failed");
}

fn main() {
    std::fs::create_dir_all(out_dir()).unwrap();

    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let m = build_month(2026, 5, "sun").unwrap();

    // A month with events scattered across several days and calendars.
    let counts = |day: u32| -> usize {
        match day {
            4 => 1,  // Team sync
            8 => 2,  // Project review + gym
            12 => 1, // Book club
            18 => 1, // Holiday
            19 => 2, // Holiday + dentist
            23 => 3, // Hike, lunch, flight
            27 => 1, // 1:1
            _ => 0,
        }
    };
    let day_rows: Vec<DayRow> = m
        .days
        .iter()
        .map(|d| DayRow {
            day: d.day,
            weekday: d.weekday,
            week_start: d.week_start,
            event_count: counts(d.day),
        })
        .collect();
    let header_pt = 1.75 * grid.spacing_pt;
    let row_pt = monthly_row_pt(
        &dev,
        TOOLBAR_SAFE_PT,
        header_pt,
        grid.margin_pt,
        day_rows.len() as u32,
    );
    let monthly_view = MonthlyView {
        month_name: "May",
        year: 2026,
        month_num: 5,
        row_pt,
        days: &day_rows,
    }
    .render()
    .unwrap();

    // A busy day, events spanning three different calendars (accent/rust/primary).
    let day_evts = vec![
        AgendaEvent {
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
            label: "09:30".into(),
            end_label: Some("10:00".into()),
            title: "Standup".into(),
            location: None,
            description: None,
            attendees: vec![],
            color: "primary".into(),
            is_all_day: false,
        },
        AgendaEvent {
            label: "12:30".into(),
            end_label: Some("13:30".into()),
            title: "Lunch with Sam".into(),
            location: Some("Café Mercato".into()),
            description: None,
            attendees: vec!["Sam Rivera".into()],
            color: "primary".into(),
            is_all_day: false,
        },
        AgendaEvent {
            label: "18:15".into(),
            end_label: Some("19:00".into()),
            title: "Dentist".into(),
            location: Some("Downtown".into()),
            description: Some("Bring insurance card".into()),
            attendees: vec!["Dr. Lee".into()],
            color: "rust".into(),
            is_all_day: false,
        },
    ];
    let day_events = DayEvents {
        month_num: 5,
        day: 19,
        day_pad: "19".into(),
        weekday: "Tue",
        events: &day_evts,
        continued: false,
        first_page: true,
    }
    .render()
    .unwrap();

    let cover = Cover {
        year: 2026,
        title: "Future Log",
        blank_title: false,
    }
    .render()
    .unwrap();

    let future_log = FutureLog {
        months: &["January", "February", "March"],
    }
    .render()
    .unwrap();

    let reference = Reference.render().unwrap();

    for (name, frag) in [
        ("cover", &cover),
        ("monthly_view", &monthly_view),
        ("day_events", &day_events),
        ("future_log", &future_log),
        ("reference", &reference),
    ] {
        let png = out_dir().join(format!("{name}.png"));
        render_png(frag, &png);
        println!("wrote {}", png.display());
    }
}
