use std::path::Path;

use askama::Template;

use crate::config::Config;
use crate::templates::{Cover, Reference};

pub fn build_reference_pdf(config: &Config, out_path: &Path) -> anyhow::Result<()> {
    let fragments = vec![
        Cover { year: config.year, title: "Reference", blank_title: false }.render()?,
        Reference.render()?,
    ];
    super::render_notebook(config, &fragments, out_path)
}
