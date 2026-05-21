//! Orchestrate: fetch -> content -> assemble -> render -> manifest -> deploy targets.
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::content::{process_html, FetchedImage, ImageFetcher};
use crate::readwise::HttpTransport;

/// Real image fetcher over ureq with guards (size cap; content-type sniff).
pub struct UreqImageFetcher;
impl ImageFetcher for UreqImageFetcher {
    fn fetch(&self, url: &str) -> Option<FetchedImage> {
        let resp = ureq::get(url).call().ok()?;
        let ct = resp.header("content-type").unwrap_or("").to_string();
        if !ct.starts_with("image/") {
            return None;
        }
        let ext = if ct.contains("svg") { "svg" } else { "bin" }.to_string();
        let mut bytes = Vec::new();
        use std::io::Read;
        resp.into_reader()
            .take(8 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .ok()?;
        Some(FetchedImage { bytes, ext })
    }
}

fn build_one(
    collection: &str,
    docs: &[crate::readwise::Document],
    config: &Config,
    fetcher: &dyn ImageFetcher,
    out_dir: &Path,
) -> anyhow::Result<PathBuf> {
    let images_enabled = config.images.enabled;
    let built = crate::assemble::assemble_document(collection, docs, |html, _id| {
        let p = process_html(html, images_enabled, fetcher);
        (p.html, p.assets)
    });
    let device = crate::device::get_device(&config.device)?;
    let theme = crate::theme::load_theme(&config.theme)?;
    let pdf_path = out_dir.join(format!("{collection}.pdf"));
    crate::render::render_pdf(&device, &theme, &built.fragments, &built.assets, &pdf_path)?;
    built
        .manifest
        .write(&out_dir.join(format!("{collection}.manifest.json")))?;
    Ok(pdf_path)
}

/// Returns deploy targets: (pdf_path, remarkable_folder).
pub fn generate(
    config: &Config,
    transport: &dyn HttpTransport,
    fetcher: &dyn ImageFetcher,
) -> anyhow::Result<Vec<(PathBuf, String)>> {
    let out_dir = PathBuf::from(&config.output_dir);
    std::fs::create_dir_all(&out_dir)?;
    let mut targets = Vec::new();

    let lib = crate::readwise::fetch_documents(
        transport,
        &config.readwise.token,
        &config.library.locations,
        config.library.max_items,
        |s| std::thread::sleep(std::time::Duration::from_secs(s)),
    )?;
    let lib_pdf = build_one("Library", &lib, config, fetcher, &out_dir)?;
    targets.push((lib_pdf, config.deploy.library_folder.clone()));

    if config.feed.enabled {
        let feed = crate::readwise::fetch_documents(
            transport,
            &config.readwise.token,
            &["feed".into()],
            config.feed.max_items,
            |s| std::thread::sleep(std::time::Duration::from_secs(s)),
        )?;
        let feed_pdf = build_one("Feed", &feed, config, fetcher, &out_dir)?;
        targets.push((feed_pdf, config.deploy.feed_folder.clone()));
    }
    Ok(targets)
}
