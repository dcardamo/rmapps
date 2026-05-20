//! Notebook builders: assemble page fragments and render one PDF each.

pub mod collection;
pub mod future_log;
pub mod month;
pub mod reference;

use std::path::Path;

use crate::config::Config;
use crate::{device, geometry, render, theme};

fn render_notebook(config: &Config, fragments: &[String], out_path: &Path) -> anyhow::Result<()> {
    let dev = device::get_device(&config.device)?;
    let grid = geometry::default_grid(&dev);
    let th = theme::load_theme(&config.theme)?;
    render::render_pdf(&dev, &grid, &th, fragments, out_path)
}
