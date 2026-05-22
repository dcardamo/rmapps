//! askama template structs (compile-time checked). Each renders an HTML fragment.

use askama::Template;

#[derive(Template)]
#[template(path = "base.html")]
pub struct Base<'a> {
    pub css: &'a str,
    pub pages: &'a [String],
}

#[derive(Template)]
#[template(path = "cover.html")]
pub struct Cover<'a> {
    pub year: i32,
    pub title: &'a str,
    pub blank_title: bool,
}

#[derive(Template)]
#[template(path = "dotgrid.html")]
pub struct DotGrid;

#[derive(Template)]
#[template(path = "tasks.html")]
pub struct Tasks;

#[derive(Template)]
#[template(path = "future_log.html")]
pub struct FutureLog<'a> {
    pub months: &'a [&'a str],
}

#[derive(Clone, Debug)]
pub struct DayRow {
    pub day: u32,
    pub weekday: &'static str,
    pub week_start: bool,
    pub event_count: usize,
}

#[derive(Template)]
#[template(path = "monthly_view.html")]
pub struct MonthlyView<'a> {
    pub month_name: &'a str,
    pub year: i32,
    pub month_num: u32,
    pub row_pt: f32,
    pub days: &'a [DayRow],
}

#[derive(Template)]
#[template(path = "reference.html")]
pub struct Reference;

#[derive(Template)]
#[template(path = "daily_page.html")]
pub struct DailyPage<'a> {
    pub day: u32,
    pub day_pad: String,
    pub month_num: u32,
    pub weekday: &'a str,
    pub event_count: usize,
}

#[derive(Clone, Debug)]
pub struct AgendaEvent {
    /// Stable per-event index within the month -> `#evt-{idx}`.
    pub idx: usize,
    /// "All Day" or "HH:MM" (start).
    pub label: String,
    /// End time "HH:MM" for timed events with a DTEND; shown on the details page.
    pub end_label: Option<String>,
    pub title: String,
    pub location: Option<String>,
    pub description: Option<String>,
    pub attendees: Vec<String>,
    /// Theme color name -> `var(--color)`.
    pub color: String,
    pub is_all_day: bool,
}

#[derive(Clone, Debug)]
pub struct AgendaDay {
    pub day: u32,
    pub weekday: &'static str,
    pub events: Vec<AgendaEvent>,
}

/// One page of a single day's events: a compact agenda list followed by expanded
/// details. `agenda`/`details` hold only the events on THIS page; the heading and
/// continuation flags come from `notebooks::month::agenda::DayPagePlan`.
#[derive(Template)]
#[template(path = "day_events.html")]
pub struct DayEvents<'a> {
    pub month_num: u32,
    pub day: u32,
    pub day_pad: String,
    pub weekday: &'a str,
    pub agenda: &'a [AgendaEvent],
    pub details: &'a [AgendaEvent],
    pub show_agenda_heading: bool,
    pub show_details_heading: bool,
    pub continued: bool,
    pub first_page: bool,
}
