pub mod agenda;

use std::collections::BTreeMap;
use std::path::Path;

use askama::Template;
use chrono::NaiveDate;

use crate::calendar::build_month;
use crate::config::Config;
use crate::ics::EventOccurrence;
use crate::templates::{Agenda, DailyPage, DayRow, Details, DotGrid, MonthlyView, Tasks};

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
    // h-month: 1.25*sp font-size + 0.5*sp margin-bottom
    let header_pt = 1.75 * grid.spacing_pt;
    let row_pt = crate::geometry::monthly_row_pt(
        &dev,
        crate::geometry::TOOLBAR_SAFE_PT,
        header_pt,
        grid.margin_pt,
        day_rows.len() as u32,
    );

    let mut fragments = vec![
        MonthlyView {
            month_name: m.name,
            year: config.year,
            month_num: month,
            row_pt,
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

    // Agenda + Details are appended ONLY when the month has events, so a static
    // month keeps its exact page count. Leading pages above are unaffected.
    let days = agenda::agenda_days(&m, events, config.year, month);
    if !days.is_empty() {
        // Paginate at the event level so a busy month's agenda/details never
        // overflow (and clip) a page: a day with too many events to fit is split
        // across pages, repeating its header. Usable height = page minus the
        // toolbar reserve, bottom margin, and title block; content width accounts
        // for side margins (and the details indent) so wrapping is estimated.
        let usable = dev.height_pt() - crate::geometry::TOOLBAR_SAFE_PT - grid.margin_pt - 30.0;
        let content_w = dev.width_pt() - 2.0 * grid.margin_pt - 8.0;
        for chunk in agenda::paginate(&days, usable, agenda::HEADER_PT, |e| {
            agenda::agenda_event_pt(content_w, e)
        }) {
            fragments.push(
                Agenda {
                    month_name: m.name,
                    year: config.year,
                    days: &chunk,
                }
                .render()?,
            );
        }
        for chunk in agenda::paginate(&days, usable, agenda::HEADER_PT, |e| {
            agenda::detail_event_pt(content_w, e)
        }) {
            fragments.push(
                Details {
                    month_name: m.name,
                    year: config.year,
                    days: &chunk,
                }
                .render()?,
            );
        }
    }

    super::render_notebook(config, &fragments, out_path)
}
