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

/// Image-processing knobs (from `ImagesConfig`). Threaded into the normalizer so
/// fetched article images are downscaled + recompressed before embedding, which
/// cuts reader PDFs from tens of MB to a few MB.
#[derive(Clone, Copy, Debug)]
pub struct ImageProcessing {
    /// Max width in px; larger images are downscaled (aspect preserved, never up).
    pub max_width: u32,
    /// JPEG re-encode quality (1-100).
    pub quality: u8,
    /// Convert to grayscale.
    pub grayscale: bool,
}

impl Default for ImageProcessing {
    fn default() -> Self {
        Self {
            max_width: 1000,
            quality: 72,
            grayscale: false,
        }
    }
}

/// Decode bytes, drop tracking pixels (<=2px on either side), downscale to a max
/// width, optionally grayscale, and re-encode as JPEG (flattening any alpha onto
/// white first, since JPEG has no alpha). Returns (final_bytes, "jpg"). Falls
/// back to the original bytes if decoding/encoding fails so we never drop a real
/// image or crash. Returns `None` only for tracking pixels.
fn normalize_image(bytes: &[u8], proc: &ImageProcessing) -> Option<(Vec<u8>, String)> {
    let Ok(img) = image::load_from_memory(bytes) else {
        // Undecodable: keep the original bytes rather than dropping the image.
        // Tag with a best-effort extension so the asset key looks sane.
        let ext = image::guess_format(bytes)
            .ok()
            .and_then(|f| f.extensions_str().first().copied())
            .unwrap_or("bin");
        return Some((bytes.to_vec(), ext.to_string()));
    };
    let (w, h) = image::GenericImageView::dimensions(&img);
    if w <= 2 || h <= 2 {
        return None; // tracking pixel
    }

    // Downscale to max width (only shrink, never upscale; preserve aspect ratio).
    let mut img = if proc.max_width > 0 && w > proc.max_width {
        let new_h = ((h as u64 * proc.max_width as u64) / w as u64).max(1) as u32;
        img.resize_exact(proc.max_width, new_h, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };

    if proc.grayscale {
        img = image::DynamicImage::ImageLuma8(img.to_luma8());
    }

    // JPEG has no alpha: flatten onto white, then encode as RGB (or luma if gray).
    let mut out = Vec::new();
    let enc_result = {
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, proc.quality);
        if proc.grayscale {
            let gray = img.to_luma8();
            encode_jpeg_luma(encoder, &gray)
        } else {
            let rgb = flatten_to_rgb(&img);
            encode_jpeg_rgb(encoder, &rgb)
        }
    };
    match enc_result {
        Ok(()) => Some((out, "jpg".into())),
        // Encoding failed (very unlikely): keep original bytes as a fallback.
        Err(_) => Some((bytes.to_vec(), "bin".into())),
    }
}

/// Flatten any alpha channel onto a white background, yielding an RGB image.
fn flatten_to_rgb(img: &image::DynamicImage) -> image::RgbImage {
    use image::GenericImageView;
    if img.color().has_alpha() {
        let rgba = img.to_rgba8();
        let (w, h) = img.dimensions();
        let mut rgb = image::RgbImage::new(w, h);
        for (x, y, px) in rgba.enumerate_pixels() {
            let [r, g, b, a] = px.0;
            let a = a as u32;
            // alpha-composite over white
            let blend = |c: u8| -> u8 { ((c as u32 * a + 255 * (255 - a)) / 255) as u8 };
            rgb.put_pixel(x, y, image::Rgb([blend(r), blend(g), blend(b)]));
        }
        rgb
    } else {
        img.to_rgb8()
    }
}

fn encode_jpeg_rgb(
    mut enc: image::codecs::jpeg::JpegEncoder<&mut Vec<u8>>,
    rgb: &image::RgbImage,
) -> image::ImageResult<()> {
    enc.encode(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
        image::ExtendedColorType::Rgb8,
    )
}

fn encode_jpeg_luma(
    mut enc: image::codecs::jpeg::JpegEncoder<&mut Vec<u8>>,
    gray: &image::GrayImage,
) -> image::ImageResult<()> {
    enc.encode(
        gray.as_raw(),
        gray.width(),
        gray.height(),
        image::ExtendedColorType::L8,
    )
}

/// Strip all HTML tags from a fragment, returning the plain text.
pub(crate) fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Collect each `<p>...</p>` block's (trimmed inner-HTML, normalised plain text).
fn paragraphs(html: &str) -> Vec<(String, String)> {
    let mut v = Vec::new();
    let mut rest = html;
    while let Some(p) = rest.find("<p") {
        let after = &rest[p + 2..];
        // Real <p> only (not <pre>/<param>/...): the next char starts the tag.
        let is_p = after
            .chars()
            .next()
            .is_some_and(|c| matches!(c, '>' | ' ' | '\t' | '\n' | '\r' | '/'));
        if !is_p {
            rest = after;
            continue;
        }
        let Some(gt) = after.find('>') else { break };
        let inner_start = p + 2 + gt + 1;
        let Some(close) = rest[inner_start..].find("</p>") else {
            break;
        };
        let inner = &rest[inner_start..inner_start + close];
        let plain = strip_tags(inner)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        v.push((inner.trim().to_string(), plain));
        rest = &rest[inner_start + close + 4..];
    }
    v
}

/// Detect Readwise's PDF text-extraction shape: many `<p>`, each holding a single
/// physical line of the source PDF, so most do NOT end in sentence-terminal
/// punctuation. (Normal prose ends paragraphs with `.`/`!`/`?`.)
fn looks_line_broken(paras: &[(String, String)]) -> bool {
    let n = paras.len();
    if n < 50 {
        return false;
    }
    let terminal = paras
        .iter()
        .filter(|(_, t)| {
            t.chars()
                .next_back()
                .is_some_and(|c| matches!(c, '.' | '!' | '?'))
        })
        .count();
    (terminal as f32 / n as f32) < 0.35
}

/// Rejoin line-broken PDF text (see `looks_line_broken`) into flowing paragraphs,
/// de-hyphenating words split at line ends, so the text reflows to our column
/// instead of hard-wrapping at the original PDF's line widths. Non-line-broken
/// HTML is returned unchanged. (OCR artefacts inside the source — e.g. mid-word
/// spaces like "tha t" — are part of the data and left untouched.)
fn reflow_line_broken(html: &str) -> String {
    let paras = paragraphs(html);
    if !looks_line_broken(&paras) {
        return html.to_string();
    }
    let mut lens: Vec<usize> = paras
        .iter()
        .map(|(_, t)| t.chars().count())
        .filter(|&n| n > 0)
        .collect();
    lens.sort_unstable();
    let median = lens.get(lens.len() / 2).copied().unwrap_or(0);
    let short = (median as f32 * 0.66) as usize;

    fn flush(out: &mut String, buf: &mut String) {
        let t = buf.trim();
        if !t.is_empty() {
            out.push_str("<p>");
            out.push_str(t);
            out.push_str("</p>\n");
        }
        buf.clear();
    }

    let mut out = String::with_capacity(html.len());
    let mut buf = String::new();
    for (inner, plain) in &paras {
        if plain.is_empty() {
            flush(&mut out, &mut buf); // blank line = paragraph break
            continue;
        }
        if buf.is_empty() {
            buf.push_str(inner);
        } else if buf.ends_with('-')
            && buf[..buf.len() - 1]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphabetic())
        {
            buf.pop(); // de-hyphenate: drop line-end hyphen, join with no space
            buf.push_str(inner);
        } else {
            buf.push(' ');
            buf.push_str(inner);
        }
        // Paragraph break on a sentence-final, ragged-short line (not a hyphen join).
        let ends_sentence = plain
            .chars()
            .next_back()
            .is_some_and(|c| matches!(c, '.' | '!' | '?'));
        if ends_sentence && plain.chars().count() < short && !buf.ends_with('-') {
            flush(&mut out, &mut buf);
        }
    }
    flush(&mut out, &mut buf);
    out
}

/// Reflow line-broken PDF text, truncate at the byte cap (on a UTF-8 boundary),
/// and collect the document's deduplicated `<img>` URLs (first-seen order). When
/// images are disabled the URL list is empty. Returns `(processed HTML, truncated
/// flag, urls)`. The returned HTML is what `assemble_processed` then sanitises, so
/// reflow happens exactly once, before truncation — matching the legacy order.
pub fn collect_doc_urls(
    html: &str,
    max_bytes: usize,
    images_enabled: bool,
) -> (String, bool, Vec<String>) {
    // Rejoin Readwise's PDF text-extraction output (one <p> per source line) into
    // flowing paragraphs before truncation/URL-collection. No-op for normal HTML.
    let reflowed = reflow_line_broken(html);
    let html: &str = &reflowed;

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
/// already reflowed-and-truncated HTML (from `collect_doc_urls`), its deduped
/// image URLs (first-seen order), and a shared `url -> fetched bytes` map. Asset
/// keys are `img-{safe_id}-{i}` over `urls`, matching the legacy single-document
/// path byte-for-byte.
pub fn assemble_processed(
    doc_id: &str,
    truncated_html: &str,
    truncated: bool,
    images_enabled: bool,
    urls: &[String],
    fetched: &std::collections::HashMap<String, FetchedImage>,
    proc: &ImageProcessing,
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
                normalize_image(&f.bytes, proc)
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
    proc: &ImageProcessing,
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
        proc,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflow_rejoins_and_dehyphenates_line_broken() {
        // 60 single-line <p>, none ending in sentence punctuation (Readwise PDF
        // shape), with one hyphenated word split across two lines.
        let mut h = String::new();
        for _ in 0..58 {
            h.push_str("<p>the quick brown fox jumps over a lazy dog and runs</p>\n");
        }
        h.push_str("<p>here is some inter-</p>\n<p>esting material to read</p>\n");
        let out = reflow_line_broken(&h);
        assert!(
            out.contains("interesting"),
            "should de-hyphenate across lines"
        );
        assert!(!out.contains("inter-"), "line-end hyphen should be removed");
        assert!(
            out.matches("<p>").count() < 30,
            "lines should merge, got {} <p>",
            out.matches("<p>").count()
        );
    }

    #[test]
    fn normalize_downscales_and_jpeg_encodes_large_image() {
        // Synthesize a 3000x2000 RGB image and run it through the processor.
        let mut src = image::RgbImage::new(3000, 2000);
        for (x, y, px) in src.enumerate_pixels_mut() {
            *px = image::Rgb([(x % 256) as u8, (y % 256) as u8, 128]);
        }
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(src)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();

        let proc = ImageProcessing::default();
        let (out, ext) = normalize_image(&png, &proc).expect("should not drop a real image");
        assert_eq!(ext, "jpg", "output should be JPEG");
        assert_eq!(
            image::guess_format(&out).unwrap(),
            image::ImageFormat::Jpeg,
            "bytes should decode as JPEG"
        );
        let decoded = image::load_from_memory(&out).expect("output should decode");
        let (w, h) = image::GenericImageView::dimensions(&decoded);
        assert!(w <= 1000, "width should be downscaled to <=1000, got {w}");
        assert_eq!(h, 666, "aspect ratio preserved: 2000*1000/3000 = 666");
        // The transform shrinks 6M source pixels to <0.7M, so the output covers
        // far fewer pixels than the original regardless of codec entropy.
        assert!(
            (w as u64 * h as u64) < (3000u64 * 2000),
            "downscaled image covers fewer pixels than the 3000x2000 source"
        );
    }

    #[test]
    fn normalize_flattens_alpha_to_jpeg() {
        // RGBA image with transparency must encode (JPEG has no alpha).
        let img = image::RgbaImage::from_pixel(50, 40, image::Rgba([10, 20, 30, 128]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let proc = ImageProcessing::default();
        let (out, ext) = normalize_image(&png, &proc).unwrap();
        assert_eq!(ext, "jpg");
        assert!(image::load_from_memory(&out).is_ok());
    }

    #[test]
    fn normalize_keeps_undecodable_bytes() {
        // Garbage that isn't an image: fall back to original bytes, don't crash.
        let junk = b"not an image at all";
        let (out, _ext) = normalize_image(junk, &ImageProcessing::default()).unwrap();
        assert_eq!(out, junk);
    }

    #[test]
    fn normalize_drops_tracking_pixel() {
        let img = image::RgbImage::from_pixel(1, 1, image::Rgb([0, 0, 0]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert!(normalize_image(&png, &ImageProcessing::default()).is_none());
    }

    #[test]
    fn reflow_leaves_normal_prose_untouched() {
        // Paragraphs ending in sentence punctuation are not line-broken.
        let mut h = String::new();
        for _ in 0..60 {
            h.push_str("<p>This is a complete sentence that ends properly.</p>\n");
        }
        assert_eq!(reflow_line_broken(&h), h, "normal prose passes through");
    }
}
