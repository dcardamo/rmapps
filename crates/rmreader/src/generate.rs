//! Orchestrate: fetch -> content -> assemble -> render -> manifest -> deploy targets.
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::content::{FetchedImage, ImageFetcher};
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
    cache: &crate::cache::Cache,
) -> anyhow::Result<PathBuf> {
    use std::collections::{HashMap, HashSet};
    use std::time::Instant;
    // Type alias to satisfy clippy::type_complexity for the per-doc (html, assets) map.
    type DocOutput = (String, Vec<(String, Vec<u8>)>);
    let images_enabled = config.images.enabled;
    let max_bytes = config.content.max_article_bytes;

    // Pass A: classify each doc as a cache hit (reuse processed output) or a miss
    // (collect its image URLs for the shared fetch below).
    struct Miss {
        id: String,
        key: String,
        html: String,
        truncated: bool,
        urls: Vec<String>,
    }
    let mut processed: HashMap<String, DocOutput> = HashMap::new();
    let mut misses: Vec<Miss> = Vec::new();
    for d in docs {
        let raw = d.html_content.clone().unwrap_or_default();
        let key = crate::cache::key(&d.id, &raw, max_bytes, images_enabled);
        if let Some(c) = cache.get(&key) {
            processed.insert(d.id.clone(), (c.html, c.assets));
        } else {
            let (html, truncated, urls) =
                crate::content::collect_doc_urls(&raw, max_bytes, images_enabled);
            misses.push(Miss {
                id: d.id.clone(),
                key,
                html,
                truncated,
                urls,
            });
        }
    }
    eprintln!(
        "[rmreader] {collection}: {} docs ({} cached, {} to fetch)",
        docs.len(),
        docs.len() - misses.len(),
        misses.len()
    );

    // One concurrent fetch over the deduped union of all miss-doc image URLs.
    let mut union: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for m in &misses {
        for u in &m.urls {
            if seen.insert(u.clone()) {
                union.push(u.clone());
            }
        }
    }
    let t = Instant::now();
    let results = fetcher.fetch_many(&union);
    let fetched: HashMap<String, crate::content::FetchedImage> = union
        .into_iter()
        .zip(results)
        .filter_map(|(u, r)| r.map(|f| (u, f)))
        .collect();
    eprintln!(
        "[rmreader] {collection}: fetched {} images in {:.1}s",
        fetched.len(),
        t.elapsed().as_secs_f32()
    );

    // Pass B: normalize + sanitize each miss from the shared bytes, then cache it.
    for m in misses {
        let p = crate::content::assemble_processed(
            &m.id,
            &m.html,
            m.truncated,
            images_enabled,
            &m.urls,
            &fetched,
        );
        cache.put(&m.key, &p.html, &p.assets);
        processed.insert(m.id, (p.html, p.assets));
    }

    // Assemble: the closure just hands each doc its precomputed processed output.
    let built = crate::assemble::assemble_document(collection, docs, |_html, id| {
        processed.remove(id).unwrap_or_default()
    });

    let device = crate::device::get_device(&config.device)?;
    let theme = crate::theme::load_theme(&config.theme)?;
    let pdf_path = out_dir.join(format!("{collection}.pdf"));
    eprintln!(
        "[rmreader] {collection}: rendering {} article(s) via Typst...",
        built.typst_articles.len()
    );
    let t = Instant::now();
    // Typst references images at /assets/{key}; serve them there.
    let assets: Vec<(String, Vec<u8>)> = built
        .assets
        .iter()
        .map(|(k, b)| (format!("/assets/{k}"), b.clone()))
        .collect();
    let rendered = crate::render::render_collection(
        &device,
        &theme,
        collection,
        &built.typst_rows,
        &built.typst_articles,
        &assets,
    )?;
    eprintln!(
        "[rmreader] {collection}: rendered in {:.1}s",
        t.elapsed().as_secs_f32()
    );

    // Fill the embedded manifest with the recovered read-back geometry, embed it
    // in the PDF catalog, and write the PDF. Typst draws the chrome (paper fill,
    // nav bar, action band, links) in-flow, so no lopdf post-processing is needed
    // beyond the manifest embed.
    let mut embedded = built.manifest.to_embedded();
    for d in &mut embedded.docs {
        if let Some(pr) = rendered.page_ranges.get(&format!("article-{}", d.id)) {
            d.page_range = *pr;
        }
    }
    embedded.label_rects = rendered.label_rects;
    let mut pdf_doc = lopdf::Document::load_mem(&rendered.pdf)?;
    crate::embed::write(&mut pdf_doc, &embedded)?;
    pdf_doc.save(&pdf_path)?;
    // Sidecar JSON: non-authoritative debug artifact (the PDF embed is canonical).
    built
        .manifest
        .write(&out_dir.join(format!("{collection}.manifest.json")))?;
    Ok(pdf_path)
}

/// Build a PDF from explicit documents (no Readwise fetch; images disabled).
/// Used by the make_test_pdf example to produce a small, controlled document.
pub fn build_pdf_from_docs(
    collection: &str,
    docs: &[crate::readwise::Document],
    config: &Config,
    out_dir: &std::path::Path,
) -> anyhow::Result<std::path::PathBuf> {
    // fetcher is constructed but never called because images are disabled
    let fetcher = UreqImageFetcher {
        timeout_secs: 5,
        concurrency: 1,
    };
    let cache = crate::cache::Cache::from_config(&config.cache);
    build_one(collection, docs, config, &fetcher, out_dir, &cache)
}

/// Returns deploy targets: (pdf_path, remarkable_folder).
pub fn generate(
    config: &Config,
    transport: &(dyn HttpTransport + Sync),
    fetcher: &(dyn ImageFetcher + Sync),
) -> anyhow::Result<Vec<(PathBuf, String)>> {
    let out_dir = PathBuf::from(&config.output_dir);
    std::fs::create_dir_all(&out_dir)?;

    let cache = crate::cache::Cache::from_config(&config.cache);
    cache.sweep();

    // Both collections are independent: fetch + build each on its own thread.
    let (lib_res, feed_res) = std::thread::scope(|s| {
        let out_dir = &out_dir;
        let cache = &cache;
        let lib_h = s.spawn(move || -> anyhow::Result<(PathBuf, String)> {
            eprintln!(
                "[rmreader] fetching library {:?}...",
                config.library.locations
            );
            let lib = crate::readwise::fetch_documents(
                transport,
                &config.readwise.token,
                &config.library.locations,
                config.library.max_items,
                sleep_secs,
            )?;
            let lib = drop_empty(lib);
            eprintln!("[rmreader] library: {} docs", lib.len());
            let pdf = build_one("Library", &lib, config, fetcher, out_dir, cache)?;
            Ok((pdf, config.deploy.library_folder.clone()))
        });
        let feed_h = if config.feed.enabled {
            Some(s.spawn(move || -> anyhow::Result<(PathBuf, String)> {
                eprintln!("[rmreader] fetching feed...");
                let feed = crate::readwise::fetch_documents(
                    transport,
                    &config.readwise.token,
                    &["feed".into()],
                    config.feed.max_items,
                    sleep_secs,
                )?;
                let feed = drop_empty(feed);
                eprintln!("[rmreader] feed: {} docs", feed.len());
                let pdf = build_one("Feed", &feed, config, fetcher, out_dir, cache)?;
                Ok((pdf, config.deploy.feed_folder.clone()))
            }))
        } else {
            None
        };
        let lib_res = lib_h.join().expect("library build thread panicked");
        let feed_res = feed_h.map(|h| h.join().expect("feed build thread panicked"));
        (lib_res, feed_res)
    });

    let mut targets = vec![lib_res?];
    if let Some(fr) = feed_res {
        targets.push(fr?);
    }
    Ok(targets)
}

/// Rate-limit sleep used by `fetch_documents` (a plain fn item so both build
/// threads can share it; it is `Copy` and `Sync`).
fn sleep_secs(s: u64) {
    std::thread::sleep(std::time::Duration::from_secs(s));
}
