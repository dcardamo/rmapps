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

    // Rejoin Readwise's PDF text-extraction output (one <p> per source line) into
    // flowing paragraphs so it reflows to our column instead of hard-wrapping at
    // the original PDF's line widths. No-op for normal HTML.
    let reflowed = reflow_line_broken(html);
    let html: &str = &reflowed;

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
    fn reflow_leaves_normal_prose_untouched() {
        // Paragraphs ending in sentence punctuation are not line-broken.
        let mut h = String::new();
        for _ in 0..60 {
            h.push_str("<p>This is a complete sentence that ends properly.</p>\n");
        }
        assert_eq!(reflow_line_broken(&h), h, "normal prose passes through");
    }
}
