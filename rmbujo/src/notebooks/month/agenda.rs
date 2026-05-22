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
