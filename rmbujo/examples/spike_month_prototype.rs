//! Throwaway spike: a real one-month prototype to *feel* the calendar UX on-device.
//!
//! Structure (May 2027), all with a 44pt top safe-area (clears the toolbar):
//!   - Monthly view (id="monthly"): each row's day#/weekday links to that day's
//!     log page; days with events also show a "diamond" link to the agenda.
//!   - Tasks page.
//!   - 31 per-day log pages (id="day-N"): header "05.DD Dow" left, "Month" +
//!     agenda links right; dotted body.
//!   - Agenda page: date-ordered blocks (id="agenda-N") with all-day + timed
//!     events; break-inside:avoid so a date never splits.
//!
//! Sample events are hardcoded (no ICS yet). Rendered at 4.0mm so the 31-row
//! monthly list fits under the toolbar reserve (a finding to resolve in the spec).
//!
//! Run: nix develop -c cargo run --example spike_month_prototype

use std::path::Path;

use rmbujo::calendar::build_month;
use rmbujo::device::get_device;
use rmbujo::geometry::dot_grid;
use rmbujo::render::render_pdf;
use rmbujo::theme::load_theme;

const TOP: &str = "padding-top:44pt;"; // toolbar safe-area (measured ~40pt + comfort)

/// (day, optional "HH:MM" for timed, title, optional location)
const EVENTS: &[(u32, Option<&str>, &str, Option<&str>)] = &[
    (1, None, "May Day", None),
    (8, Some("10:00"), "Coffee with Sam", Some("Cafe Rio")),
    (19, None, "Victoria Day", None),
    (19, Some("14:00"), "Dentist", Some("Downtown")),
    (24, None, "Vacation", None),
    (
        24,
        Some("09:00"),
        "Flight YYZ\u{2192}YVR",
        Some("Terminal 1"),
    ),
    (25, None, "Vacation", None),
    (26, None, "Vacation", None),
    (30, None, "Mom's birthday", None),
];

fn has_events(day: u32) -> bool {
    EVENTS.iter().any(|e| e.0 == day)
}

fn page(id: Option<&str>, class: &str, inner: &str) -> String {
    let id_attr = id.map(|i| format!(" id=\"{i}\"")).unwrap_or_default();
    format!("<section class=\"page {class}\"{id_attr} style=\"{TOP}\">{inner}</section>")
}

fn main() -> anyhow::Result<()> {
    let dev = get_device("paper-pro-move")?;
    let grid = dot_grid(&dev, 4.0, 6.0); // tighter so 31 rows fit under the reserve
    let theme = load_theme("library")?;
    let m = build_month(2027, 5, "sun")?;

    let mut frags: Vec<String> = Vec::new();

    // --- Monthly view ---
    let mut rows = String::new();
    for d in &m.days {
        let ws = if d.week_start { " weekstart" } else { "" };
        let cal = if has_events(d.day) {
            format!(
                " <a href=\"#agenda-{}\" style=\"color:#1B365D;text-decoration:none;\
                 font-size:9pt;margin-left:6pt;\">\u{25C6}</a>",
                d.day
            )
        } else {
            String::new()
        };
        rows.push_str(&format!(
            "<div class=\"day{ws}\">\
             <a href=\"#day-{day}\" style=\"text-decoration:none;color:inherit;\
             display:inline-flex;gap:6pt;align-items:center;\">\
             <span class=\"num\">{day}</span><span class=\"wd\">{wd}</span></a>{cal}</div>",
            day = d.day,
            wd = d.weekday,
        ));
    }
    frags.push(page(
        Some("monthly"),
        "month-index",
        &format!("<div class=\"h-month\">May 2027</div><div class=\"month-list\">{rows}</div>"),
    ));

    // --- Tasks ---
    frags.push(page(
        None,
        "dotpage",
        "<div class=\"h-section\">Tasks</div>",
    ));

    // --- Per-day log pages ---
    for d in &m.days {
        let cal = if has_events(d.day) {
            format!(
                " <a href=\"#agenda-{}\" style=\"color:#1B365D;text-decoration:none;\">\u{25C6}</a>",
                d.day
            )
        } else {
            String::new()
        };
        let header = format!(
            "<div style=\"display:flex;justify-content:space-between;align-items:baseline;\">\
             <div class=\"h-month\" style=\"font-size:13pt;\">05.{day:02} {wd}</div>\
             <div style=\"font-size:11pt;\">\
             <a href=\"#monthly\" style=\"color:#1B365D;text-decoration:none;\">\u{2039} Month</a>{cal}\
             </div></div>",
            day = d.day,
            wd = d.weekday,
        );
        frags.push(page(Some(&format!("day-{}", d.day)), "dotpage", &header));
    }

    // --- Agenda (one page; sample set fits) ---
    let mut agenda = String::from("<div class=\"h-month\">Agenda \u{2014} May 2027</div>");
    let mut days_with: Vec<u32> = EVENTS.iter().map(|e| e.0).collect();
    days_with.sort_unstable();
    days_with.dedup();
    for day in days_with {
        let wd = m.days[(day - 1) as usize].weekday;
        let mut block = format!(
            "<div id=\"agenda-{day}\" style=\"break-inside:avoid;margin-bottom:8pt;\">\
             <div style=\"font-weight:bold;color:#1B365D;font-size:11pt;\">{day} {wd}</div>"
        );
        // all-day first
        for e in EVENTS.iter().filter(|e| e.0 == day && e.1.is_none()) {
            block.push_str(&format!(
                "<div style=\"margin:2pt 0;\"><span class=\"pill\">{}</span></div>",
                e.2
            ));
        }
        // then timed, by time
        let mut timed: Vec<_> = EVENTS
            .iter()
            .filter(|e| e.0 == day && e.1.is_some())
            .collect();
        timed.sort_by_key(|e| e.1.unwrap());
        for e in timed {
            let loc = e.3.map(|l| format!(" \u{2014} {l}")).unwrap_or_default();
            block.push_str(&format!(
                "<div style=\"font-size:9pt;margin:2pt 0;\">\
                 <b>{}</b>&nbsp;&nbsp;{}{}</div>",
                e.1.unwrap(),
                e.2,
                loc
            ));
        }
        block.push_str("</div>");
        agenda.push_str(&block);
    }
    frags.push(page(None, "", &agenda));

    let dir = Path::new("/tmp/rmbujo-proto");
    std::fs::create_dir_all(dir)?;
    let out = dir.join("2027.05 May (prototype).pdf");
    render_pdf(&dev, &grid, &theme, &frags, &out)?;
    println!("{} pages -> {}", frags.len(), out.display());
    Ok(())
}
