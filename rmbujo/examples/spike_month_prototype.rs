//! Throwaway spike: one-month prototype to feel the calendar UX on-device (v2).
//!
//! May 2027, 36pt top safe-area (clears the toolbar). Navigation:
//!   - Monthly view (id="monthly"): day#/weekday -> that day's log page; days with
//!     events show a right-aligned calendar icon -> the agenda for that date.
//!   - Tasks page.
//!   - 31 per-day log pages (id="day-N"): the date (05.DD Dow) links back to the
//!     month page; event days show a calendar icon -> agenda.
//!   - Agenda page: title "2027 May - Agenda" ("2027 May" -> month page); each
//!     date header -> that day's log page; each event -> its detail page.
//!   - Detail page: one block per event (id="evt-I") with when/where + back-link.
//!
//! Sample events hardcoded. 4.0mm spacing so the 31-row monthly list fits.
//! Run: nix develop -c cargo run --example spike_month_prototype

use std::path::Path;

use rmbujo::calendar::build_month;
use rmbujo::device::get_device;
use rmbujo::geometry::dot_grid;
use rmbujo::render::render_pdf;
use rmbujo::theme::load_theme;

const TOP: &str = "padding-top:36pt;";
const YEAR: i32 = 2027;
const MONTH: u32 = 5;
const MONTH_NAME: &str = "May";

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

/// A small CSS-drawn calendar glyph (box + header bar), used as a link affordance.
fn cal_icon() -> String {
    "<span style=\"display:inline-block;width:9pt;height:9pt;border:0.6pt solid #1B365D;\
     border-radius:1.5pt;position:relative;vertical-align:-1.5pt;\">\
     <span style=\"position:absolute;top:0;left:0;right:0;height:2.6pt;background:#1B365D;\
     border-radius:1.5pt 1.5pt 0 0;\"></span></span>"
        .to_string()
}

fn page(id: Option<&str>, class: &str, inner: &str) -> String {
    let id_attr = id.map(|i| format!(" id=\"{i}\"")).unwrap_or_default();
    format!("<section class=\"page {class}\"{id_attr} style=\"{TOP}\">{inner}</section>")
}

/// Label shown left of an event: its time, or "All Day".
fn when_label(time: Option<&str>) -> &str {
    time.unwrap_or("All Day")
}

fn main() -> anyhow::Result<()> {
    let dev = get_device("paper-pro-move")?;
    let grid = dot_grid(&dev, 4.0, 6.0);
    let theme = load_theme("library")?;
    let m = build_month(YEAR, MONTH, "sun")?;
    let wd_of = |day: u32| m.days[(day - 1) as usize].weekday;

    let mut frags: Vec<String> = Vec::new();

    // --- Monthly view ---
    let mut rows = String::new();
    for d in &m.days {
        let ws = if d.week_start { " weekstart" } else { "" };
        let icon = if has_events(d.day) {
            format!(
                "<a href=\"#agenda-{}\" style=\"margin-left:auto;text-decoration:none;\">{}</a>",
                d.day,
                cal_icon()
            )
        } else {
            String::new()
        };
        rows.push_str(&format!(
            "<div class=\"day{ws}\">\
             <a href=\"#day-{day}\" style=\"text-decoration:none;color:inherit;\
             display:inline-flex;gap:6pt;align-items:center;\">\
             <span class=\"num\">{day}</span><span class=\"wd\">{wd}</span></a>{icon}</div>",
            day = d.day,
            wd = d.weekday,
        ));
    }
    frags.push(page(
        Some("monthly"),
        "month-index",
        &format!("<div class=\"h-month\">{MONTH_NAME} {YEAR}</div><div class=\"month-list\">{rows}</div>"),
    ));

    // --- Tasks ---
    frags.push(page(
        None,
        "dotpage",
        "<div class=\"h-section\">Tasks</div>",
    ));

    // --- Per-day log pages ---
    for d in &m.days {
        let icon = if has_events(d.day) {
            format!(
                "<a href=\"#agenda-{}\" style=\"text-decoration:none;\">{}</a>",
                d.day,
                cal_icon()
            )
        } else {
            String::new()
        };
        let header = format!(
            "<div style=\"display:flex;justify-content:space-between;align-items:center;\">\
             <a href=\"#monthly\" class=\"h-month\" style=\"font-size:13pt;text-decoration:none;\
             color:var(--navy);\">{mn}.{day:02} {wd}</a>\
             <div>{icon}</div></div>",
            mn = MONTH,
            day = d.day,
            wd = d.weekday,
        );
        frags.push(page(Some(&format!("day-{}", d.day)), "dotpage", &header));
    }

    // --- Agenda ---
    let title = format!(
        "<div class=\"h-month\"><a href=\"#monthly\" style=\"color:var(--navy);\
         text-decoration:none;\">{YEAR} {MONTH_NAME}</a> - Agenda</div>"
    );
    let mut agenda = title;
    let mut days_with: Vec<u32> = EVENTS.iter().map(|e| e.0).collect();
    days_with.sort_unstable();
    days_with.dedup();
    for day in days_with {
        agenda.push_str(&format!(
            "<div style=\"break-inside:avoid;margin-bottom:8pt;\" id=\"agenda-{day}\">\
             <a href=\"#day-{day}\" style=\"font-weight:bold;color:#1B365D;font-size:11pt;\
             text-decoration:none;\">{day} {wd}</a>",
            wd = wd_of(day),
        ));
        // all-day first, then timed by time; each line links to its detail page.
        let mut idxs: Vec<usize> = (0..EVENTS.len()).filter(|&i| EVENTS[i].0 == day).collect();
        idxs.sort_by_key(|&i| EVENTS[i].1); // None ("All Day") sorts before Some(time)
        for i in idxs {
            let (_, time, title, loc) = EVENTS[i];
            let loc_s = loc.map(|l| format!(" \u{2014} {l}")).unwrap_or_default();
            agenda.push_str(&format!(
                "<div style=\"font-size:9pt;margin:2pt 0;\">\
                 <a href=\"#evt-{i}\" style=\"color:#1a1a1a;text-decoration:none;\">\
                 <b>{label}</b>&nbsp;&nbsp;{title}{loc_s}</a></div>",
                label = when_label(time),
            ));
        }
        agenda.push_str("</div>");
    }
    frags.push(page(None, "", &agenda));

    // --- Event detail page ---
    let mut detail = String::from("<div class=\"h-month\">Event details</div>");
    for (i, (day, time, title, loc)) in EVENTS.iter().enumerate() {
        let where_line = loc
            .map(|l| format!("<div style=\"font-size:9pt;\">Where: {l}</div>"))
            .unwrap_or_default();
        detail.push_str(&format!(
            "<div id=\"evt-{i}\" style=\"break-inside:avoid;margin-bottom:8pt;\
             border-bottom:0.4pt solid var(--rule);padding-bottom:4pt;\">\
             <div style=\"font-weight:bold;font-size:12pt;color:#1B365D;\">{title}</div>\
             <div style=\"font-size:9pt;\">When: {label} \u{00b7} {MONTH_NAME} {day} {YEAR}</div>\
             {where_line}\
             <div style=\"font-size:9pt;color:#888;\">Notes: \u{2014}</div>\
             <div style=\"font-size:8pt;margin-top:2pt;\">\
             <a href=\"#agenda-{day}\" style=\"color:#1B365D;text-decoration:none;\">\u{2039} Agenda</a>\
             </div></div>",
            label = when_label(*time),
        ));
    }
    frags.push(page(None, "", &detail));

    let dir = Path::new("/tmp/rmbujo-proto");
    std::fs::create_dir_all(dir)?;
    let out = dir.join("2027.05 May (prototype).pdf");
    render_pdf(&dev, &grid, &theme, &frags, &out)?;
    println!("{} pages -> {}", frags.len(), out.display());
    Ok(())
}
