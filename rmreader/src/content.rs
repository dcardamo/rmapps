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
pub fn process_html(
    html: &str,
    doc_id: &str,
    images_enabled: bool,
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

    // Pass 1: fetch + normalize images into a url -> key map.
    let mut url_to_key: HashMap<String, String> = HashMap::new();
    let mut assets: Vec<(String, Vec<u8>)> = Vec::new();
    if images_enabled {
        for (i, url) in collect_img_urls(html).into_iter().enumerate() {
            if url_to_key.contains_key(&url) {
                continue;
            }
            if let Some(fetched) = fetcher.fetch(&url) {
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
    let cleaned = rewrite_str(
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

    Processed {
        html: cleaned,
        assets,
    }
}
