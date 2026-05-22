//! Turn Readwise html_content into render-ready HTML with embedded local images.
//!
//! # Security model
//! The sanitiser (Pass 2 below) removes `<script>`, `<iframe>`, `<noscript>`,
//! `<style>`, `<object>`, `<embed>`, `<form>`, all `on*` event handlers, every
//! inline `style` attribute (plus legacy presentational attrs like `bgcolor`,
//! `width`), and rewrites every `<img src>` to a local asset key (dropping
//! unresolvable images). Stripping inline styles also neutralises `style url()`
//! references AND stops the source's `font-family` from overriding our embedded
//! fonts — an override renders text blank, since the offline renderer has no
//! system fonts. Remaining content safety — `<link>`, `<meta http-equiv=refresh>`,
//! and any other remote or `data:` targets — relies on fulgur's `file://`-only
//! `NetProvider` as a second line of defence: those targets simply never load
//! and never trigger network or navigation actions during PDF rendering.
use lol_html::{element, rewrite_str, RewriteStrSettings};

#[derive(Clone)]
pub struct FetchedImage {
    pub bytes: Vec<u8>,
    pub ext: String, // "png" | "jpg" | "gif" | "svg" (post-transcode)
}

/// Network seam (real impl in render/generate uses ureq).
pub trait ImageFetcher {
    fn fetch(&self, url: &str) -> Option<FetchedImage>;

    /// Fetch multiple URLs, returning results in the same order as the input.
    ///
    /// The default implementation is sequential so that test fakes (which may
    /// use `RefCell` and are therefore not `Sync`) continue to work without
    /// change.  The real `UreqImageFetcher` overrides this with a concurrent
    /// implementation using `std::thread::scope`.
    fn fetch_many(&self, urls: &[String]) -> Vec<Option<FetchedImage>> {
        urls.iter().map(|u| self.fetch(u)).collect()
    }
}

pub struct Processed {
    pub html: String,
    pub assets: Vec<(String, Vec<u8>)>, // (asset_key, bytes) for AssetBundle
}

/// Collect <img> src URLs (first pass).
fn collect_img_urls(html: &str) -> Vec<String> {
    let urls = std::cell::RefCell::new(Vec::new());
    let _ = rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![element!("img[src]", |el| {
                if let Some(src) = el.get_attribute("src") {
                    if src.starts_with("http://") || src.starts_with("https://") {
                        urls.borrow_mut().push(src);
                    }
                }
                Ok(())
            })],
            ..RewriteStrSettings::default()
        },
    );
    urls.into_inner()
}

/// Decode bytes, drop tracking pixels (<=2px on either side), transcode
/// WebP/AVIF to PNG. Returns (final_bytes, ext) or None to drop the image.
fn normalize_image(bytes: &[u8]) -> Option<(Vec<u8>, String)> {
    let fmt = image::guess_format(bytes).ok()?;
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = image::GenericImageView::dimensions(&img);
    if w <= 2 || h <= 2 {
        return None; // tracking pixel
    }
    use image::ImageFormat as F;
    match fmt {
        F::Png => Some((bytes.to_vec(), "png".into())),
        F::Jpeg => Some((bytes.to_vec(), "jpg".into())),
        F::Gif => Some((bytes.to_vec(), "gif".into())),
        _ => {
            // WebP/AVIF/etc -> re-encode to PNG (krilla supports png/jpeg/gif/svg)
            let mut out = std::io::Cursor::new(Vec::new());
            img.write_to(&mut out, F::Png).ok()?;
            Some((out.into_inner(), "png".into()))
        }
    }
}

/// Truncate at the byte cap (on a UTF-8 boundary) and collect the document's
/// deduplicated `<img>` URLs (first-seen order). When images are disabled the URL
/// list is empty. Returns `(possibly-truncated HTML, truncated flag, urls)`.
pub fn collect_doc_urls(
    html: &str,
    max_bytes: usize,
    images_enabled: bool,
) -> (String, bool, Vec<String>) {
    let (html, truncated) = if html.len() > max_bytes {
        let mut end = max_bytes;
        while end > 0 && !html.is_char_boundary(end) {
            end -= 1;
        }
        (&html[..end], true)
    } else {
        (html, false)
    };
    let urls = if images_enabled {
        let mut seen = std::collections::HashSet::new();
        collect_img_urls(html)
            .into_iter()
            .filter(|u| seen.insert(u.clone()))
            .collect()
    } else {
        Vec::new()
    };
    (html.to_string(), truncated, urls)
}

/// Build sanitized HTML + normalized image assets for one document, given the
/// already-truncated HTML, its deduped image URLs (first-seen order), and a
/// shared `url -> fetched bytes` map. Asset keys are `img-{safe_id}-{i}` over
/// `urls`, matching the legacy single-document path byte-for-byte.
pub fn assemble_processed(
    doc_id: &str,
    truncated_html: &str,
    truncated: bool,
    images_enabled: bool,
    urls: &[String],
    fetched: &std::collections::HashMap<String, FetchedImage>,
) -> Processed {
    use std::collections::HashMap;
    // Sanitise doc_id so the key is always filename-safe.
    let safe_id: String = doc_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();

    let mut url_to_key: HashMap<String, String> = HashMap::new();
    let mut assets: Vec<(String, Vec<u8>)> = Vec::new();
    if images_enabled {
        for (i, url) in urls.iter().enumerate() {
            let Some(f) = fetched.get(url) else { continue };
            let normalized = if f.ext == "svg" {
                Some((f.bytes.clone(), "svg".to_string()))
            } else {
                normalize_image(&f.bytes)
            };
            if let Some((bytes, ext)) = normalized {
                // Include doc_id so keys are unique across articles when assets
                // from multiple documents are merged into one bundle.
                let key = format!("img-{safe_id}-{i}.{ext}");
                url_to_key.insert(url.clone(), key.clone());
                assets.push((key, bytes));
            }
        }
    }

    // Pass 2: rewrite img src -> key (drop unresolved/disabled), strip dangerous nodes/attrs.
    let mut cleaned = rewrite_str(
        truncated_html,
        RewriteStrSettings {
            element_content_handlers: vec![
                element!("script,iframe,noscript,style,object,embed,form", |el| {
                    el.remove();
                    Ok(())
                }),
                element!("img", |el| {
                    let keep = el
                        .get_attribute("src")
                        .and_then(|s| url_to_key.get(&s).cloned());
                    match keep {
                        Some(key) => {
                            let _ = el.set_attribute("src", &key);
                        }
                        None => el.remove(),
                    }
                    Ok(())
                }),
                element!("*", |el| {
                    // Strip event handlers, inline styles, and legacy presentational
                    // attributes. Inline `font-family` (ubiquitous in newsletter
                    // emails) is the critical one: it overrides our embedded fonts
                    // with system fonts the offline renderer lacks, so the text
                    // renders BLANK. Dropping all inline styling also gives clean,
                    // uniform reader styling instead of the source's.
                    let names: Vec<String> = el.attributes().iter().map(|a| a.name()).collect();
                    for n in names {
                        if n.starts_with("on")
                            || matches!(
                                n.as_str(),
                                "style"
                                    | "class"
                                    | "align"
                                    | "valign"
                                    | "bgcolor"
                                    | "color"
                                    | "face"
                                    | "width"
                                    | "height"
                            )
                        {
                            el.remove_attribute(&n);
                        }
                    }
                    Ok(())
                }),
            ],
            ..RewriteStrSettings::default()
        },
    )
    .unwrap_or_else(|_| truncated_html.to_string());

    if truncated {
        cleaned.push_str(
            "<p class=\"truncated\"><em>… Article truncated for on-device reading — open it in Readwise for the full text.</em></p>",
        );
    }

    Processed {
        html: cleaned,
        assets,
    }
}

/// Sanitise `html` and embed images as local assets (single-document path used by
/// tests and any caller without a shared fetch pool). Delegates to
/// `collect_doc_urls` + `fetch_many` + `assemble_processed`.
///
/// `doc_id` is included in every asset key so that keys remain globally unique
/// when assets from multiple documents are merged into a single `AssetBundle`.
/// `max_bytes` caps pathologically large articles so fulgur's layout time can't
/// blow up. Comes from `config.content.max_article_bytes`.
pub fn process_html(
    html: &str,
    doc_id: &str,
    images_enabled: bool,
    max_bytes: usize,
    fetcher: &dyn ImageFetcher,
) -> Processed {
    let (truncated_html, truncated, urls) = collect_doc_urls(html, max_bytes, images_enabled);
    let results = fetcher.fetch_many(&urls);
    let fetched: std::collections::HashMap<String, FetchedImage> = urls
        .iter()
        .cloned()
        .zip(results)
        .filter_map(|(u, r)| r.map(|f| (u, f)))
        .collect();
    assemble_processed(
        doc_id,
        &truncated_html,
        truncated,
        images_enabled,
        &urls,
        &fetched,
    )
}
