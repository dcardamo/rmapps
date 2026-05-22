//! Throwaway spike: one-month prototype to feel the calendar UX on-device (v3).
//!
//! May 2027, 36pt top safe-area (~12.7mm, clears the toolbar). Navigation:
//!   - Monthly view (id="monthly"): fixed-width date link -> that day's log page;
//!     event days show a calendar icon (aligned column) -> the agenda for that date.
//!   - Tasks page.
//!   - 31 per-day log pages (id="day-N"): the date (5.DD Dow) links to the month
//!     page; event days show a calendar icon -> agenda.
//!   - Agenda page: "2027 May - Agenda" ("2027 May" -> month); date header -> day
//!     log; each event line (compact) -> its detail entry.
//!   - Details page: date-organized like the agenda, expanded per event
//!     (id="evt-I"); Where shown only if present, Notes only if present.
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

/// (day, optional "HH:MM" for timed, title, optional location, optional notes)
type Ev = (
    u32,
    Option<&'static str>,
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
);
const EVENTS: &[Ev] = &[
    (1, None, "May Day", None, None),
    (8, Some("10:00"), "Coffee with Sam", Some("Cafe Rio"), None),
    (19, None, "Victoria Day", None, None),
    (
        19,
        Some("14:00"),
        "Dentist",
        Some("Downtown"),
        Some("Bring insurance card"),
    ),
    (24, None, "Vacation", None, None),
    (
        24,
        Some("09:00"),
        "Flight YYZ\u{2192}YVR",
        Some("Terminal 1"),
        None,
    ),
    (25, None, "Vacation", None, None),
    (26, None, "Vacation", None, None),
    (30, None, "Mom's birthday", None, None),
];

fn event_count(day: u32) -> usize {
    EVENTS.iter().filter(|e| e.0 == day).count()
}

/// A tappable navy pill badge showing the day's event count, linking to the
/// agenda for that date. About the dot-line height, widened to fit the digit;
/// horizontal padding widens the tap target. Empty when the day has no events.
fn agenda_badge(day: u32) -> String {
    let c = event_count(day);
    if c == 0 {
        return String::new();
    }
    format!(
        "<a href=\"#agenda-{day}\" style=\"text-decoration:none;padding:0 3pt;\">\
         <span style=\"display:inline-flex;align-items:center;justify-content:center;\
         min-width:13pt;height:11pt;padding:0 4pt;border-radius:5.5pt;background:#1B365D;\
         color:#fff;font-size:8pt;font-weight:bold;line-height:1;vertical-align:-2.5pt;\">{c}</span></a>"
    )
}

fn page(id: Option<&str>, class: &str, inner: &str) -> String {
    let id_attr = id.map(|i| format!(" id=\"{i}\"")).unwrap_or_default();
    format!("<section class=\"page {class}\"{id_attr} style=\"{TOP}\">{inner}</section>")
}

/// Label shown left of an event: its time, or "All Day".
fn when_label(time: Option<&str>) -> &str {
    time.unwrap_or("All Day")
}

/// Indices of events on a given day, all-day first then timed by time.
fn day_event_idxs(day: u32) -> Vec<usize> {
    let mut idxs: Vec<usize> = (0..EVENTS.len()).filter(|&i| EVENTS[i].0 == day).collect();
    idxs.sort_by_key(|&i| EVENTS[i].1);
    idxs
}

fn main() -> anyhow::Result<()> {
    let dev = get_device("paper-pro-move")?;
    let grid = dot_grid(&dev, 4.0, 6.0);
    let theme = load_theme("library")?;
    let m = build_month(YEAR, MONTH, "sun")?;
    let wd_of = |day: u32| m.days[(day - 1) as usize].weekday;

    let mut days_with: Vec<u32> = EVENTS.iter().map(|e| e.0).collect();
    days_with.sort_unstable();
    days_with.dedup();

    let mut frags: Vec<String> = Vec::new();

    // --- Monthly view ---
    let mut rows = String::new();
    for d in &m.days {
        let ws = if d.week_start { " weekstart" } else { "" };
        // Fixed-width date link => event-count badges line up in a column.
        rows.push_str(&format!(
            "<div class=\"day{ws}\">\
             <a href=\"#day-{day}\" style=\"text-decoration:none;color:inherit;\
             display:inline-flex;gap:6pt;align-items:center;width:44pt;\">\
             <span class=\"num\">{day}</span><span class=\"wd\">{wd}</span></a>{badge}</div>",
            day = d.day,
            wd = d.weekday,
            badge = agenda_badge(d.day),
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
        let header = format!(
            "<div style=\"display:flex;justify-content:space-between;align-items:center;\">\
             <a href=\"#monthly\" class=\"h-month\" style=\"font-size:13pt;text-decoration:none;\
             color:var(--navy);\">{mn}.{day:02} {wd}</a><div>{badge}</div></div>",
            mn = MONTH,
            day = d.day,
            wd = d.weekday,
            badge = agenda_badge(d.day),
        );
        frags.push(page(Some(&format!("day-{}", d.day)), "dotpage", &header));
    }

    // --- Agenda (compact) ---
    let mut agenda = format!(
        "<div class=\"h-month\"><a href=\"#monthly\" style=\"color:var(--navy);\
         text-decoration:none;\">{YEAR} {MONTH_NAME}</a> - Agenda</div>"
    );
    for &day in &days_with {
        agenda.push_str(&format!(
            "<div style=\"break-inside:avoid;margin-bottom:8pt;\" id=\"agenda-{day}\">\
             <a href=\"#day-{day}\" style=\"font-weight:bold;color:#1B365D;font-size:11pt;\
             text-decoration:none;\">{day} {wd}</a>",
            wd = wd_of(day),
        ));
        for i in day_event_idxs(day) {
            let (_, time, title, loc, _) = EVENTS[i];
            let loc_s = loc.map(|l| format!(" \u{2014} {l}")).unwrap_or_default();
            agenda.push_str(&format!(
                "<div style=\"font-size:9pt;margin:2pt 0;color:#1a1a1a;\">\
                 <a href=\"#evt-{i}\" style=\"color:#1a1a1a;text-decoration:none;\">\
                 <b>{label}</b>&nbsp;&nbsp;{title}{loc_s}</a></div>",
                label = when_label(time),
            ));
        }
        agenda.push_str("</div>");
    }
    frags.push(page(None, "", &agenda));

    // --- Details (date-organized, expanded) ---
    let mut detail = format!(
        "<div class=\"h-month\"><a href=\"#monthly\" style=\"color:var(--navy);\
         text-decoration:none;\">{YEAR} {MONTH_NAME}</a> - Details</div>"
    );
    for &day in &days_with {
        detail.push_str(&format!(
            "<div style=\"break-inside:avoid;margin-bottom:8pt;\">\
             <a href=\"#day-{day}\" style=\"font-weight:bold;color:#1B365D;font-size:11pt;\
             text-decoration:none;\">{day} {wd}</a>",
            wd = wd_of(day),
        ));
        for i in day_event_idxs(day) {
            let (_, time, title, loc, notes) = EVENTS[i];
            let where_line = loc
                .map(|l| format!("<div style=\"font-size:9pt;color:#1a1a1a;\">Where: {l}</div>"))
                .unwrap_or_default();
            let notes_line = notes
                .map(|n| format!("<div style=\"font-size:9pt;color:#1a1a1a;\">Notes: {n}</div>"))
                .unwrap_or_default();
            detail.push_str(&format!(
                "<div id=\"evt-{i}\" style=\"margin:3pt 0 6pt 8pt;\">\
                 <div style=\"font-size:10pt;color:#1a1a1a;\"><b>{label}</b>&nbsp;&nbsp;{title}</div>\
                 {where_line}{notes_line}</div>",
                label = when_label(time),
            ));
        }
        detail.push_str("</div>");
    }
    frags.push(page(None, "", &detail));

    let dir = Path::new("/tmp/rmbujo-proto");
    std::fs::create_dir_all(dir)?;
    let out = dir.join("2027.05 May (prototype).pdf");
    render_pdf(&dev, &grid, &theme, &frags, &out)?;
    println!("{} pages -> {}", frags.len(), out.display());
    Ok(())
}
