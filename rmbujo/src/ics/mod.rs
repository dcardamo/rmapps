//! ICS (iCalendar) ingestion: parse `.ics` feeds into per-day event occurrences.
//!
//! Only the `parse` submodule exists for now; feed fetching is a later task.

pub mod parse;

use chrono::{NaiveDate, NaiveTime};

/// A single calendar event materialized onto one specific day.
///
/// Multi-day and recurring events expand into one `EventOccurrence` per day.
/// Timed events are already converted into the configured timezone, so `date`
/// and `time` reflect local wall-clock values in that zone.
#[derive(Debug, Clone, PartialEq)]
pub struct EventOccurrence {
    pub date: NaiveDate,
    /// `None` = all-day event; `Some` = timed event (already in the config tz).
    pub time: Option<NaiveTime>,
    pub title: String,
    pub location: Option<String>,
    pub description: Option<String>,
    pub attendees: Vec<String>,
    pub color: String,
}
