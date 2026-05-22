//! Orchestrate: fetch -> content -> assemble -> render -> manifest -> deploy targets.
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::content::{process_html, FetchedImage, ImageFetcher};
use crate::readwise::HttpTransport;

/// Real image fetcher over ureq with guards (size cap; content-type sniff).
pub struct UreqImageFetcher {
    /// Per-request network timeout in seconds.
    pub timeout_secs: u64,
    /// Maximum number of concurrent image-fetch threads.
    pub concurrency: usize,
}

impl ImageFetcher for UreqImageFetcher {
    fn fetch(&self, url: &str) -> Option<FetchedImage> {
        // Tight per-request timeout: images are small, and feed content pulls from
        // many hosts — a slow/dead server must fail fast so it can't stall the run.
        let resp = ureq::get(url)
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .call()
            .ok()?;
        let ct = resp.header("content-type").unwrap_or("").to_string();
        if !ct.starts_with("image/") {
            return None;
        }
        // Derive an honest extension from the content-type subtype so downstream
        // code (and asset keys) reflect the actual format.  SVG must be detected
        // first because its subtype is "svg+xml".  Unknown types fall back to
        // "bin", which normalize_image() will still handle correctly.
        let ext = if ct.contains("svg") {
            "svg"
        } else if ct.contains("jpeg") || ct.contains("jpg") {
            "jpg"
        } else if ct.contains("png") {
            "png"
        } else if ct.contains("gif") {
            "gif"
        } else if ct.contains("webp") {
            "webp"
        } else {
            "bin"
        }
        .to_string();
        let mut bytes = Vec::new();
        use std::io::Read;
        resp.into_reader()
            .take(8 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .ok()?;
        Some(FetchedImage { bytes, ext })
    }

    /// Fetch all URLs concurrently using a bounded thread pool.
    ///
    /// Uses `std::thread::scope` (no extra deps) with an `AtomicUsize` work
    /// cursor so N worker threads each claim the next un-fetched index.
    /// Results are written into a pre-sized array of `Mutex`-guarded slots,
    /// guaranteeing output order == input order regardless of completion order.
    fn fetch_many(&self, urls: &[String]) -> Vec<Option<FetchedImage>> {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Mutex,
        };

        let n = urls.len();
        if n == 0 {
            return Vec::new();
        }

        // Pre-allocate result slots; each starts as None.
        let results: Vec<Mutex<Option<FetchedImage>>> = (0..n).map(|_| Mutex::new(None)).collect();
        let cursor = AtomicUsize::new(0);
        // Clamp concurrency to at least 1 and at most the number of URLs.
        let workers = self.concurrency.max(1).min(n);

        std::thread::scope(|s| {
            for _ in 0..workers {
                s.spawn(|| loop {
                    let i = cursor.fetch_add(1, Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    let result = self.fetch(&urls[i]);
                    *results[i].lock().unwrap() = result;
                });
            }
        });

        // Unwrap each Mutex — all threads have joined, no contention remains.
        results
            .into_iter()
            .map(|m| m.into_inner().unwrap())
            .collect()
    }
}

/// Drop docs Readwise has no reader text for (e.g. unparsed saves or archive
/// listing pages) — they would otherwise render as blank articles.
fn drop_empty(docs: Vec<crate::readwise::Document>) -> Vec<crate::readwise::Document> {
    docs.into_iter()
        .filter(|d| {
            d.html_content
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
        })
        .collect()
}

fn build_one(
    collection: &str,
    docs: &[crate::readwise::Document],
    config: &Config,
    fetcher: &dyn ImageFetcher,
    out_dir: &Path,
) -> anyhow::Result<PathBuf> {
    use std::time::Instant;
    let images_enabled = config.images.enabled;
    let max_bytes = config.content.max_article_bytes;
    let total = docs.len();
    let idx = std::cell::Cell::new(0usize);
    eprintln!("[rmreader] {collection}: processing {total} docs");
    let built = crate::assemble::assemble_document(collection, docs, |html, id| {
        let i = idx.get() + 1;
        idx.set(i);
        let t = Instant::now();
        let p = process_html(html, id, images_enabled, max_bytes, fetcher);
        eprintln!(
            "[rmreader]   {collection} {i}/{total}: {} KB html, {} imgs, {:.1}s",
            html.len() / 1024,
            p.assets.len(),
            t.elapsed().as_secs_f32()
        );
        (p.html, p.assets)
    });
    let device = crate::device::get_device(&config.device)?;
    let theme = crate::theme::load_theme(&config.theme)?;
    let pdf_path = out_dir.join(format!("{collection}.pdf"));
    eprintln!(
        "[rmreader] {collection}: rendering {} fragments...",
        built.fragments.len()
    );
    let t = Instant::now();
    crate::render::render_pdf(&device, &theme, &built.fragments, &built.assets, &pdf_path)?;
    eprintln!(
        "[rmreader] {collection}: wrote {} in {:.1}s",
        pdf_path.display(),
        t.elapsed().as_secs_f32()
    );
    // Stamp a persistent clickable Home/Prev/Next bar on every article page.
    // Best-effort: leaves the rendered PDF intact on any failure.
    crate::postprocess::add_per_page_nav(
        &pdf_path,
        docs.len(),
        device.width_pt(),
        device.height_pt(),
    )?;
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

    eprintln!(
        "[rmreader] fetching library {:?}...",
        config.library.locations
    );
    let lib = crate::readwise::fetch_documents(
        transport,
        &config.readwise.token,
        &config.library.locations,
        config.library.max_items,
        |s| std::thread::sleep(std::time::Duration::from_secs(s)),
    )?;
    let lib = drop_empty(lib);
    eprintln!("[rmreader] library: {} docs", lib.len());
    let lib_pdf = build_one("Library", &lib, config, fetcher, &out_dir)?;
    targets.push((lib_pdf, config.deploy.library_folder.clone()));

    if config.feed.enabled {
        eprintln!("[rmreader] fetching feed...");
        let feed = crate::readwise::fetch_documents(
            transport,
            &config.readwise.token,
            &["feed".into()],
            config.feed.max_items,
            |s| std::thread::sleep(std::time::Duration::from_secs(s)),
        )?;
        let feed = drop_empty(feed);
        eprintln!("[rmreader] feed: {} docs", feed.len());
        let feed_pdf = build_one("Feed", &feed, config, fetcher, &out_dir)?;
        targets.push((feed_pdf, config.deploy.feed_folder.clone()));
    }
    Ok(targets)
}
