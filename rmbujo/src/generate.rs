//! Orchestrate building a whole year of notebook PDFs.

use std::path::{Path, PathBuf};

use crate::calendar::MONTH_NAMES;
use crate::config::Config;
use crate::notebooks::{collection, future_log, month, reference};

pub fn generate_year(config: &Config, out_dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(out_dir)?;
    let y = config.year;
    let mut paths = Vec::new();

    let fl = out_dir.join(format!("{y} Future Log.pdf"));
    future_log::build_future_log_pdf(config, &fl)?;
    paths.push(fl);

    for mo in 1..=12u32 {
        let p = out_dir.join(format!("{y}.{mo:02} {name}.pdf", name = MONTH_NAMES[mo as usize]));
        month::build_month_pdf(config, mo, &p)?;
        paths.push(p);
    }

    let col = out_dir.join(format!("{y} Collection Template.pdf"));
    collection::build_collection_pdf(config, &col)?;
    paths.push(col);

    let r = out_dir.join(format!("{y} Reference.pdf"));
    reference::build_reference_pdf(config, &r)?;
    paths.push(r);

    Ok(paths)
}
