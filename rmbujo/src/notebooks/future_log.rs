use std::path::Path;

use askama::Template;

use crate::calendar::MONTH_NAMES;
use crate::config::Config;
use crate::templates::{Cover, FutureLog};

pub fn build_future_log_pdf(config: &Config, out_path: &Path) -> anyhow::Result<()> {
    let names: &[&str] = &MONTH_NAMES[1..]; // 12 month names
    let mut fragments = vec![Cover { year: config.year, title: "Future Log", blank_title: false }.render()?];
    for chunk in names.chunks(3) {
        fragments.push(FutureLog { months: chunk }.render()?);
    }
    super::render_notebook(config, &fragments, out_path)
}
