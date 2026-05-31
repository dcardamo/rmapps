//! Convert the ICS event map into per-day agenda/detail rows for one month.

use std::collections::BTreeMap;

use chrono::{NaiveDate, NaiveTime};

use crate::ics::EventOccurrence;
use crate::templates::{AgendaDay, AgendaEvent};

/// Build the agenda rows for `month`, one `AgendaDay` per calendar day that has
/// events.
pub fn agenda_days(
    m: &crate::calendar::Month,
    events: &BTreeMap<NaiveDate, Vec<EventOccurrence>>,
    year: i32,
    month: u32,
) -> Vec<AgendaDay> {
    let mut out = Vec::new();
    for d in &m.days {
        let date = NaiveDate::from_ymd_opt(year, month, d.day).unwrap();
        let Some(occs) = events.get(&date) else {
            continue;
        };
        if occs.is_empty() {
            continue;
        }
        let day_events = occs
            .iter()
            .map(|o| AgendaEvent {
                label: label_for(o.time),
                end_label: o.end_time.map(|t| t.format("%H:%M").to_string()),
                title: cap(&o.title, 100),
                location: o.location.as_deref().map(|l| cap(l, 90)),
                description: o.description.as_deref().map(|d| cap(d, 140)),
                attendees: o.attendees.clone(),
                color: o.color.clone(),
                is_all_day: o.time.is_none(),
            })
            .collect();
        out.push(AgendaDay {
            day: d.day,
            weekday: d.weekday,
            events: day_events,
        });
    }
    out
}

fn label_for(time: Option<NaiveTime>) -> String {
    match time {
        None => "All Day".to_string(),
        Some(t) => t.format("%H:%M").to_string(),
    }
}

/// Truncate a field to `max` characters at a word boundary, for clean, bounded
/// rows — long descriptions are often boilerplate (e.g. meeting-join blurbs).
fn cap(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max).collect();
    if let Some(i) = t.rfind(' ') {
        t.truncate(i);
    }
    t.push('…');
    t
}

/// Conservative wrapped-line count for `chars` of text at `font_pt` within
/// `width_pt`. Uses a ~0.55em average advance so it slightly *over*-estimates,
/// keeping pagination on the safe side (never overflowing a page).
fn lines_for(chars: usize, font_pt: f32, width_pt: f32) -> f32 {
    if chars == 0 {
        return 0.0;
    }
    let cpl = (width_pt / (font_pt * 0.55)).max(1.0);
    (chars as f32 / cpl).ceil().max(1.0)
}

/// Vertical cost (pt) of a date header on a day's event page.
pub const HEADER_PT: f32 = 20.0;

/// Estimated height (pt) of one event entry: a title line plus a wrapping line
/// for each present Where/Notes/Who field.
pub fn event_pt(width_pt: f32, e: &AgendaEvent) -> f32 {
    let title_chars = e.label.chars().count()
        + e.end_label
            .as_ref()
            .map(|s| s.chars().count() + 1)
            .unwrap_or(0)
        + 2
        + e.title.chars().count();
    let mut h = lines_for(title_chars, 10.0, width_pt) * 14.0 + 3.0;
    if let Some(l) = &e.location {
        h += lines_for(7 + l.chars().count(), 9.0, width_pt) * 12.0;
    }
    if let Some(d) = &e.description {
        h += lines_for(7 + d.chars().count(), 9.0, width_pt) * 12.0;
    }
    if !e.attendees.is_empty() {
        let chars: usize = e.attendees.iter().map(|a| a.chars().count() + 2).sum();
        h += lines_for(5 + chars, 9.0, width_pt) * 12.0;
    }
    h + 8.0
}

/// One rendered page of a single day's events: the events that fit on this page
/// plus continuation/first-page flags. Agenda and details are merged into one
/// list, so a page is fully described by its event slice.
#[derive(Clone, Debug, Default)]
pub struct DayPagePlan {
    pub events: Vec<AgendaEvent>,
    /// True on every page after the day's first (drives the "· cont." marker).
    pub continued: bool,
    /// True on the day's first page (the `#agenda-{day}` pill target).
    pub first_page: bool,
}

/// Paginate ONE day's events into per-page plans. Each event is rendered as a full
/// entry (time, title, Where/Notes/Who) sized by `event_pt`, and events fill pages
/// in order. Each page costs `header_pt` (the running date header). An event taller
/// than a fresh page is placed as-is (field capping bounds entry height).
pub fn paginate_day(
    day: &AgendaDay,
    usable_pt: f32,
    header_pt: f32,
    event_pt: impl Fn(&AgendaEvent) -> f32,
) -> Vec<DayPagePlan> {
    let mut pages: Vec<DayPagePlan> = Vec::new();
    let mut cur = DayPagePlan {
        first_page: true,
        ..Default::default()
    };
    let mut h = header_pt;

    for e in &day.events {
        let eh = event_pt(e);
        // Flush when the next event won't fit and the page already has content.
        // A lone oversized event is placed as-is (height is bounded by field caps).
        if h + eh > usable_pt && !cur.events.is_empty() {
            pages.push(std::mem::take(&mut cur));
            cur = DayPagePlan {
                continued: true,
                ..Default::default()
            };
            h = header_pt;
        }
        cur.events.push(e.clone());
        h += eh;
    }
    if !cur.events.is_empty() {
        pages.push(cur);
    }
    pages
}
