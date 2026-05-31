//! Shared calendar event shape, produced by calendar connectors (the read-only
//! ICS feed, the writable local calendar) and rendered by `CalendarView`. Kept in
//! core so the component and every calendar connector agree on one type.

/// One calendar event, reduced to the fields inkapp renders this slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRow {
    /// Stable id from the source. Used to build the app message on cancel; the
    /// region name is the *index*, not the uid, so uid needs no sanitization.
    pub uid: String,
    pub summary: String,
    /// RFC 5545 DTSTART, carried verbatim (no timezone normalization this slice).
    pub start: String,
    /// RFC 5545 DTEND, carried verbatim.
    pub end: String,
    /// Edit/optimistic state. Read-only feeds always produce `false`.
    pub cancelled: bool,
}
