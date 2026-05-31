pub mod agenda;

use std::collections::BTreeMap;
use std::path::Path;

use chrono::NaiveDate;

use crate::calendar::build_month;
use crate::config::Config;
use crate::ics::EventOccurrence;
use crate::templates::{DailyPage, DayEvents, DayRow, DotGrid, MonthlyView, Tasks};

pub fn build_month_pdf(
    config: &Config,
    month: u32,
    events: &BTreeMap<NaiveDate, Vec<EventOccurrence>>,
    out_path: &Path,
) -> anyhow::Result<()> {
    let m = build_month(config.year, month, &config.week_start)?;

    // Per-day event count drives the navy badge on the monthly + daily pages.
    let count_for = |day: u32| -> usize {
        NaiveDate::from_ymd_opt(config.year, month, day)
            .and_then(|d| events.get(&d))
            .map(|v| v.len())
            .unwrap_or(0)
    };

    let day_rows: Vec<DayRow> = m
        .days
        .iter()
        .map(|d| DayRow {
            day: d.day,
            weekday: d.weekday,
            week_start: d.week_start,
            event_count: count_for(d.day),
        })
        .collect();

    let dev = crate::device::get_device(&config.device)?;
    let grid =
        crate::geometry::dot_grid(&dev, config.spacing_mm, crate::geometry::DEFAULT_MARGIN_MM);
    let mut fragments = vec![
        MonthlyView {
            month_name: m.name,
            year: config.year,
            month_num: month,
            spacing_pt: grid.spacing_pt,
            days: &day_rows,
        }
        .render()?,
        Tasks.render()?,
    ];
    for d in &m.days {
        fragments.push(
            DailyPage {
                day: d.day,
                day_pad: format!("{:02}", d.day),
                month_num: month,
                weekday: d.weekday,
                event_count: count_for(d.day),
            }
            .render()?,
        );
        for _ in 1..config.pages_per_day {
            fragments.push(DotGrid.render()?);
        }
    }

    // Per-day event pages are appended ONLY when the month has events, so a static
    // month keeps its exact page count. Each day with events gets its own page
    // block listing every event in full (time, title, Where/Notes/Who): a light
    // day is one page, a busy day spills onto more pages, and a day never shares a
    // page with another day. The `#agenda-{day}` pill from the monthly/daily pages
    // lands on the day's first page. Usable height = page minus the toolbar
    // reserve, bottom margin, and title block; content width accounts for side
    // margins (and the event indent) so wrapping is estimated.
    let days = agenda::agenda_days(&m, events, config.year, month);
    if !days.is_empty() {
        let usable = dev.height_pt() - crate::geometry::TOOLBAR_SAFE_PT - grid.margin_pt - 30.0;
        let content_w = dev.width_pt() - 2.0 * grid.margin_pt - 8.0;
        for day in &days {
            for plan in agenda::paginate_day(day, usable, agenda::HEADER_PT, |e| {
                agenda::event_pt(content_w, e)
            }) {
                fragments.push(
                    DayEvents {
                        month_num: month,
                        day: day.day,
                        day_pad: format!("{:02}", day.day),
                        weekday: day.weekday,
                        events: &plan.events,
                        continued: plan.continued,
                        first_page: plan.first_page,
                    }
                    .render()?,
                );
            }
        }
    }

    super::render_notebook(config, &fragments, out_path)
}
