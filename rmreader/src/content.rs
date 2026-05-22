//! Turn Readwise html_content into render-ready HTML with embedded local images.
//!
//! # Security model
//! The sanitiser (Pass 2 below) removes `<script>`, `<iframe>`, `<noscript>`,
//! `<style>`, `<object>`, `<embed>`, `<form>`, and all `on*` event handlers,
//! and rewrites every `<img src>` to a local asset key (dropping unresolvable
//! images). This is the first line of defence. Remaining content safety —
//! `style` `url()` references, `<link>`, `<meta http-equiv=refresh>`, and
//! any other remote or `data:` targets — relies on fulgur's `file://`-only
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

/// Sanitise `html` and embed images as local assets.
///
/// `doc_id` is included in every asset key so that keys remain globally unique
/// when assets from multiple documents are merged into a single `AssetBundle`.
///
/// `max_bytes` caps pathologically large articles so fulgur's layout time
/// can't blow up and freeze the render. Comes from `config.content.max_article_bytes`.
pub fn process_html(
    html: &str,
    doc_id: &str,
    images_enabled: bool,
    max_bytes: usize,
    fetcher: &dyn ImageFetcher,
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

    // Cap pathologically large articles so fulgur's layout time can't blow up and
    // freeze the render. Truncate at a UTF-8 boundary; a note (appended below) sends
    // the reader to Readwise for the full text.
    let (html, truncated) = if html.len() > max_bytes {
        let mut end = max_bytes;
        while end > 0 && !html.is_char_boundary(end) {
            end -= 1;
        }
        (&html[..end], true)
    } else {
        (html, false)
    };

    // Pass 1: collect deduplicated <img> URLs (first-seen order), fetch them
    // all at once (concurrently if the fetcher supports it), then build
    // url_to_key + assets in deterministic index order so asset keys
    // (img-{safe_id}-{i}) are stable across runs.
    let mut url_to_key: HashMap<String, String> = HashMap::new();
    let mut assets: Vec<(String, Vec<u8>)> = Vec::new();
    if images_enabled {
        // Deduplicate while preserving first-seen order.
        let mut seen = std::collections::HashSet::new();
        let urls: Vec<String> = collect_img_urls(html)
            .into_iter()
            .filter(|u| seen.insert(u.clone()))
            .collect();

        let results = fetcher.fetch_many(&urls);

        for (i, (url, fetched_opt)) in urls.into_iter().zip(results).enumerate() {
            if let Some(fetched) = fetched_opt {
                let normalized = if fetched.ext == "svg" {
                    Some((fetched.bytes.clone(), "svg".into()))
                } else {
                    normalize_image(&fetched.bytes)
                };
                if let Some((bytes, ext)) = normalized {
                    // Include doc_id so keys are unique across articles when
                    // assets from multiple documents are merged into one bundle.
                    let key = format!("img-{safe_id}-{i}.{ext}");
                    url_to_key.insert(url.clone(), key.clone());
                    assets.push((key, bytes));
                }
            }
        }
    }

    // Pass 2: rewrite img src -> key (drop unresolved/disabled), strip dangerous nodes/attrs.
    let mut cleaned = rewrite_str(
        html,
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
                    let names: Vec<String> = el.attributes().iter().map(|a| a.name()).collect();
                    for n in names {
                        if n.starts_with("on") {
                            el.remove_attribute(&n);
                        }
                    }
                    Ok(())
                }),
            ],
            ..RewriteStrSettings::default()
        },
    )
    .unwrap_or_else(|_| html.to_string());

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
