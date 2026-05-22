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
                    title: o.title.clone(),
                    location: o.location.clone(),
                    description: o.description.clone(),
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

/// Conservative rendered height (pt) of an agenda day block, for pagination.
/// Date header (~13pt) + one line per event (~13pt) + bottom margin (~8pt),
/// rounded up so a chunk never overflows the page (which would clip it).
pub fn agenda_block_pt(d: &AgendaDay) -> f32 {
    24.0 + 14.0 * d.events.len() as f32
}

/// Conservative rendered height (pt) of a details day block: date header + per
/// event a title line plus a line per present Where/Notes/Who field.
pub fn detail_block_pt(d: &AgendaDay) -> f32 {
    let mut h = 16.0;
    for e in &d.events {
        let meta = e.location.is_some() as u32
            + e.description.is_some() as u32
            + (!e.attendees.is_empty()) as u32;
        h += 22.0 + 14.0 * meta as f32;
    }
    h + 8.0
}

/// Split `days` into page-sized chunks. A single day's block is never split
/// across a page boundary — it starts a new page if it wouldn't fit.
pub fn paginate(
    days: &[AgendaDay],
    usable_pt: f32,
    block_pt: fn(&AgendaDay) -> f32,
) -> Vec<Vec<AgendaDay>> {
    let mut pages: Vec<Vec<AgendaDay>> = Vec::new();
    let mut cur: Vec<AgendaDay> = Vec::new();
    let mut h = 0.0;
    for d in days {
        let bh = block_pt(d);
        if !cur.is_empty() && h + bh > usable_pt {
            pages.push(std::mem::take(&mut cur));
            h = 0.0;
        }
        cur.push(d.clone());
        h += bh;
    }
    if !cur.is_empty() {
        pages.push(cur);
    }
    pages
}
