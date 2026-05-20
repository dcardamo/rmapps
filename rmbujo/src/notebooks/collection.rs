use std::path::Path;

use askama::Template;

use crate::config::Config;
use crate::templates::{Cover, DotGrid};

pub fn build_collection_pdf(config: &Config, out_path: &Path) -> anyhow::Result<()> {
    let mut fragments = vec![Cover { year: config.year, title: "", blank_title: true }.render()?];
    for _ in 0..config.collection_pages {
        fragments.push(DotGrid.render()?);
    }
    super::render_notebook(config, &fragments, out_path)
}
