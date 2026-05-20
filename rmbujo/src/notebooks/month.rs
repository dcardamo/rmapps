use std::path::Path;

use askama::Template;

use crate::calendar::build_month;
use crate::config::Config;
use crate::templates::{DayView, DotGrid, MonthIndex, Tasks};

pub fn build_month_pdf(config: &Config, month: u32, out_path: &Path) -> anyhow::Result<()> {
    let m = build_month(config.year, month, &config.week_start)?;
    let days: Vec<DayView> = m.days.iter()
        .map(|d| DayView { day: d.day, weekday: d.weekday, week_start: d.week_start })
        .collect();
    let mut fragments = vec![
        MonthIndex { month_name: m.name, year: config.year, days: &days }.render()?,
        Tasks.render()?,
    ];
    for _ in 0..config.daily_pages {
        fragments.push(DotGrid.render()?);
    }
    super::render_notebook(config, &fragments, out_path)
}
