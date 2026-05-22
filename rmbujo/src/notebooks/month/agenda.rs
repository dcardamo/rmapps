//! Convert the ICS event map into per-day agenda/detail rows for one month.

use std::collections::BTreeMap;

use chrono::{NaiveDate, NaiveTime};

use crate::ics::EventOccurrence;
use crate::templates::{AgendaDay, AgendaEvent};

/// Build the agenda rows for `month`, one `AgendaDay` per calendar day that has
/// events. Event `idx` is assigned sequentially in date order across the whole
/// month so `#evt-K` anchors are stable and unique.
pub fn agenda_days(
    m: &crate::calendar::Month,
    events: &BTreeMap<NaiveDate, Vec<EventOccurrence>>,
    year: i32,
    month: u32,
) -> Vec<AgendaDay> {
    let mut out = Vec::new();
    let mut idx = 0usize;
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
            .map(|o| {
                let e = AgendaEvent {
                    idx,
                    label: label_for(o.time),
                    end_label: o.end_time.map(|t| t.format("%H:%M").to_string()),
                    title: cap(&o.title, 100),
                    location: o.location.as_deref().map(|l| cap(l, 90)),
                    description: o.description.as_deref().map(|d| cap(d, 140)),
                    attendees: o.attendees.clone(),
                    color: o.color.clone(),
                    is_all_day: o.time.is_none(),
                };
                idx += 1;
                e
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

/// Vertical cost (pt) of a date header on an agenda/detail page.
pub const HEADER_PT: f32 = 20.0;

/// Estimated height (pt) of one agenda line (label + title + location, wrapping).
pub fn agenda_event_pt(width_pt: f32, e: &AgendaEvent) -> f32 {
    let mut chars = e.label.chars().count() + 2 + e.title.chars().count();
    if let Some(end) = &e.end_label {
        chars += end.chars().count() + 1;
    }
    if let Some(l) = &e.location {
        chars += l.chars().count() + 3;
    }
    lines_for(chars, 9.0, width_pt) * 13.0 + 3.0
}

/// Estimated height (pt) of one details block: a title line plus a wrapping line
/// for each present Where/Notes/Who field.
pub fn detail_event_pt(width_pt: f32, e: &AgendaEvent) -> f32 {
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

/// Paginate `days` into page-sized chunks at the EVENT level: a day with more
/// events than fit on a page is split across pages, repeating its date header.
/// `header_pt` is the per-day header cost; `event_pt` gives each event's height.
pub fn paginate(
    days: &[AgendaDay],
    usable_pt: f32,
    header_pt: f32,
    event_pt: impl Fn(&AgendaEvent) -> f32,
) -> Vec<Vec<AgendaDay>> {
    let new_day = |d: &AgendaDay| AgendaDay {
        day: d.day,
        weekday: d.weekday,
        events: Vec::new(),
    };
    let mut pages: Vec<Vec<AgendaDay>> = Vec::new();
    let mut page: Vec<AgendaDay> = Vec::new();
    let mut h = 0.0;
    for day in days {
        let mut cur = new_day(day);
        let mut header_counted = false;
        for ev in &day.events {
            let eh = event_pt(ev);
            let need = if header_counted { eh } else { header_pt + eh };
            // Flush when the next event won't fit and the current page already has
            // content (this day's events, or earlier days). An oversized lone event
            // on an otherwise-empty page is placed as-is (bounded by field capping).
            let has_content = !cur.events.is_empty() || !page.is_empty();
            if h + need > usable_pt && has_content {
                if !cur.events.is_empty() {
                    page.push(std::mem::replace(&mut cur, new_day(day)));
                }
                pages.push(std::mem::take(&mut page));
                h = 0.0;
                header_counted = false;
            }
            if !header_counted {
                h += header_pt;
                header_counted = true;
            }
            cur.events.push(ev.clone());
            h += eh;
        }
        if !cur.events.is_empty() {
            page.push(cur);
        }
    }
    if !page.is_empty() {
        pages.push(page);
    }
    pages
}
