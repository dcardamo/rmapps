use std::path::Path;

use askama::Template;

use crate::calendar::build_month;
use crate::config::Config;
use crate::templates::{DayRow, DotGrid, MonthlyView, Tasks};

pub fn build_month_pdf(config: &Config, month: u32, out_path: &Path) -> anyhow::Result<()> {
    let m = build_month(config.year, month, &config.week_start)?;
    let day_rows: Vec<DayRow> = m
        .days
        .iter()
        .map(|d| DayRow {
            day: d.day,
            weekday: d.weekday,
            week_start: d.week_start,
            event_count: 0,
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
    for _ in 0..config.daily_pages {
        fragments.push(DotGrid.render()?);
    }
    super::render_notebook(config, &fragments, out_path)
}
