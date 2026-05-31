//! Build the single "Digest" output as ONE pure-typst document (no lopdf):
//! a hyperlinked digest followed by a rasterized image of each annotated source
//! page (with the highlights painted on). Tapping a digest entry jumps to the
//! page it came from via a typst `#link` → `#label` anchor; every page is also a
//! PDF outline bookmark.
//!
//! Why typst-only: typst can't embed an external PDF's vector pages, so we
//! rasterize the annotated pages with `pdftoppm` and place them as images. This
//! is robust on ANY source PDF (unlike the old lopdf page-tree surgery) and gives
//! real in-document hyperlinks + bookmarks, exactly like rmreader.
//!
//! Highlight placement: the reMarkable stores snap-to-text highlights as the
//! verbatim text PLUS rectangles in an opaque customZoom canvas space that does
//! NOT map linearly onto the source page. So we ignore those rectangles and
//! instead locate the highlighted text on the page with `pdftotext -bbox` — the
//! text is exact, so the painted box lands precisely on the words.
use std::process::Command;

use anyhow::{bail, Context};
use rmfiles::Bundle;
use tiny_skia::{Paint, Pixmap, Rect as SkRect, Transform};

use crate::device::Device;
use crate::extract::Mark;

/// Metadata shown on the cover (same shape as the standalone digest).
pub use crate::digest_doc::DigestMeta;

/// Resolution the source pages are rasterized + text-located at. A point maps to
/// `pt * RASTER_DPI / 72` pixels, so the page image and the bbox painting share
/// one coordinate system. 150 dpi is crisp on the Move's e-ink.
const RASTER_DPI: f32 = 150.0;

/// A word box from `pdftotext -bbox`, in PDF points, TOP-LEFT origin.
struct WordBox {
    text: String,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

/// Escape for a typst double-quoted string literal.
fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Escape arbitrary text for typst markup (content) mode.
fn esc_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '\\' | '#'
                | '$'
                | '*'
                | '_'
                | '`'
                | '<'
                | '>'
                | '@'
                | '='
                | '~'
                | '"'
                | '\''
                | '['
                | ']'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Lowercased alphanumerics only — for tolerant token matching
/// (so `(844)` matches `844`, `24/7` matches `247`).
fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Rasterize one 1-based page of `pdf` to a PNG at `RASTER_DPI`.
fn rasterize_page(pdf_path: &std::path::Path, page_1based: usize) -> anyhow::Result<Vec<u8>> {
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("page");
    let n = page_1based.to_string();
    let dpi = (RASTER_DPI as u32).to_string();
    let status = Command::new("pdftoppm")
        .args([
            "-f",
            &n,
            "-l",
            &n,
            "-png",
            "-singlefile",
            "-r",
            &dpi,
            pdf_path.to_str().unwrap(),
            prefix.to_str().unwrap(),
        ])
        .status()
        .context("spawn pdftoppm")?;
    if !status.success() {
        bail!("pdftoppm failed for page {page_1based}");
    }
    std::fs::read(prefix.with_extension("png")).context("read rasterized page png")
}

/// Parse `pdftotext -bbox` word boxes for one page (TOP-LEFT origin points).
fn word_boxes(pdf_path: &std::path::Path, page_1based: usize) -> anyhow::Result<Vec<WordBox>> {
    let n = page_1based.to_string();
    let out = Command::new("pdftotext")
        .args(["-bbox", "-f", &n, "-l", &n, pdf_path.to_str().unwrap(), "-"])
        .output()
        .context("spawn pdftotext")?;
    if !out.status.success() {
        bail!("pdftotext -bbox failed for page {page_1based}");
    }
    let xhtml = String::from_utf8_lossy(&out.stdout);
    let mut words = Vec::new();
    let mut rest = xhtml.as_ref();
    while let Some(i) = rest.find("<word ") {
        rest = &rest[i..];
        let Some(tag_end) = rest.find('>') else { break };
        let tag = &rest[..tag_end];
        let after = &rest[tag_end + 1..];
        let Some(close) = after.find("</word>") else {
            break;
        };
        let text = after[..close]
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&#39;", "'")
            .replace("&quot;", "\"");
        let attr = |name: &str| -> f64 {
            tag.find(&format!("{name}=\""))
                .and_then(|p| {
                    let v = &tag[p + name.len() + 2..];
                    v.find('"').and_then(|e| v[..e].parse().ok())
                })
                .unwrap_or(0.0)
        };
        words.push(WordBox {
            text,
            x0: attr("xMin"),
            y0: attr("yMin"),
            x1: attr("xMax"),
            y1: attr("yMax"),
        });
        rest = &after[close + 7..];
    }
    Ok(words)
}

/// A pixel rectangle `(x, y, w, h)`, top-left origin.
type PxRect = (f32, f32, f32, f32);

/// Find the px rectangles (TOP-LEFT) that cover `phrase` among `words`.
///
/// Locates the contiguous run of words whose normalized tokens match the phrase
/// tokens; returns each matched word's box scaled to pixels. Falls back to
/// painting every standalone token match if no contiguous run is found.
fn phrase_boxes(words: &[WordBox], phrase: &str, dpi: f32) -> Vec<PxRect> {
    let toks: Vec<String> = phrase
        .split_whitespace()
        .map(norm)
        .filter(|t| !t.is_empty())
        .collect();
    if toks.is_empty() {
        return Vec::new();
    }
    let wn: Vec<String> = words.iter().map(|w| norm(&w.text)).collect();
    let to_px = |w: &WordBox| {
        let s = dpi / 72.0;
        (
            (w.x0 as f32) * s,
            (w.y0 as f32) * s,
            ((w.x1 - w.x0) as f32) * s,
            ((w.y1 - w.y0) as f32) * s,
        )
    };
    // Contiguous run match.
    if wn.len() >= toks.len() {
        for start in 0..=(wn.len() - toks.len()) {
            if (0..toks.len()).all(|k| wn[start + k] == toks[k]) {
                return (start..start + toks.len())
                    .map(|i| to_px(&words[i]))
                    .collect();
            }
        }
    }
    // Fallback: every standalone token match.
    let set: std::collections::HashSet<&String> = toks.iter().collect();
    words
        .iter()
        .zip(&wn)
        .filter(|(_, n)| set.contains(n))
        .map(|(w, _)| to_px(w))
        .collect()
}

/// Paint translucent yellow boxes onto a rasterized page for each phrase.
fn paint_phrases(page_png: &[u8], words: &[WordBox], phrases: &[&str]) -> anyhow::Result<Vec<u8>> {
    let mut pm = Pixmap::decode_png(page_png).context("decode page png")?;
    let mut paint = Paint::default();
    paint.set_color_rgba8(245, 208, 66, 96); // highlighter yellow, translucent
    paint.anti_alias = true;
    for phrase in phrases {
        for (x, y, w, h) in phrase_boxes(words, phrase, RASTER_DPI) {
            // Pad vertically a touch so the wash covers the glyph ascenders.
            if let Some(r) = SkRect::from_xywh(x, y - 1.0, w, h + 2.0) {
                pm.fill_rect(r, &paint, Transform::identity(), None);
            }
        }
    }
    pm.encode_png().context("encode annotated page png")
}

/// Compiled typst source plus its image assets (`(virtual_path, png_bytes)`).
pub type TypstDoc = (String, Vec<(String, Vec<u8>)>);

/// Build the combined typst source + image assets.
pub fn build_linked(
    meta: &DigestMeta,
    marks: &[Mark],
    bundle: &Bundle,
    device: &Device,
) -> anyhow::Result<TypstDoc> {
    let mut assets: Vec<(String, Vec<u8>)> = Vec::new();
    let mut s = String::new();

    // ── Preamble + cover ───────────────────────────────────────────────────
    s.push_str(&format!(
        r##"#set document(title: "{title}", author: "{author}")
#set page(width: {pw}pt, height: {ph}pt, margin: (x: 7mm, y: 8mm), fill: rgb(250, 249, 246))
#set text(font: "Newsreader", size: 11pt, fill: rgb(26, 26, 26), lang: "en", hyphenate: false)
#set par(leading: 0.65em, spacing: 0.8em, justify: false)
#set heading(outlined: true)
#let kick(t, c) = text(font: "Hanken Grotesk", size: 7pt, weight: "semibold", tracking: 2pt, fill: c)[#upper(t)]
#show heading.where(level: 2): it => block(above: 22pt, below: 6pt, width: 100%)[#it.body]
#align(center + horizon)[
  #text(font: "Hanken Grotesk", size: 7.5pt, weight: "semibold", tracking: 3pt, fill: rgb(120,120,120))[DIGEST]
  #v(14pt)
  #text(font: "Newsreader", size: 20pt, weight: "semibold")[{title_m}]
  #v(6pt)
  #text(font: "Newsreader", size: 12pt, style: "italic", fill: rgb(80,80,80))[{author_m}]
  #v(14pt)
  #line(length: 38%, stroke: 0.5pt + rgb(180,180,180))
  #v(10pt)
  #text(font: "Hanken Grotesk", size: 8pt, fill: rgb(100,100,100))[{nh} {hw} · {nn} {nw}{range}]
]
#pagebreak()
"##,
        title = esc(&meta.title),
        author = esc(&meta.author),
        pw = device.width_pt(),
        ph = device.height_pt(),
        title_m = esc_markup(&meta.title),
        author_m = esc_markup(&meta.author),
        nh = meta.n_highlights,
        hw = if meta.n_highlights == 1 { "highlight" } else { "highlights" },
        nn = meta.n_notes,
        nw = if meta.n_notes == 1 { "note" } else { "notes" },
        range = if meta.date_range.is_empty() {
            String::new()
        } else {
            format!(" --- {}", esc_markup(&meta.date_range))
        },
    ));

    // ── Digest entries (each highlight links to its page anchor) ───────────
    let lq = '\u{201C}';
    let rq = '\u{201D}';
    for m in marks {
        match m {
            Mark::Highlight { page, text, color } => {
                let (r, g, b) = crate::theme::pen_rgb(*color);
                let (kr, kg, kb) = darken(r, g, b);
                s.push_str(&format!(
                    "#heading(level: 2, outlined: true)[#link(label(\"p{idx}\"))[#kick(\"page {pg}\", rgb({kr},{kg},{kb}))]]\n\
                     #block(width: 100%, inset: (left: 11pt, right: 4pt, y: 4pt), stroke: (left: 3pt + rgb({r},{g},{b}).lighten(20%)))[#par(leading: 0.7em)[#text(font: \"Newsreader\", size: 12pt)[{lq}{t}{rq}]]]\n\
                     #v(8pt)\n#line(length: 22%, stroke: 0.4pt + rgb(200,200,200))\n#v(2pt)\n",
                    idx = page, pg = page + 1, t = esc_markup(text),
                ));
            }
            Mark::Note { page, png }
            | Mark::InsertedPage {
                after_page: page,
                png,
            } => {
                let name = format!("/assets/note-{}.png", assets.len());
                assets.push((name.clone(), png.clone()));
                s.push_str(&format!(
                    "#heading(level: 3, outlined: true)[#link(label(\"p{idx}\"))[#kick(\"page {pg} · note\", rgb(120,120,120))]]\n\
                     #block(stroke: 0.5pt + rgb(200,200,200), inset: 5pt, radius: 2pt)[#image(\"{name}\", width: 100%)]\n#v(8pt)\n",
                    idx = page, pg = page + 1, name = name,
                ));
            }
        }
    }

    // ── Annotated source pages (rasterized + highlights painted on) ────────
    let mut pages: Vec<usize> = marks
        .iter()
        .map(|m| match m {
            Mark::Highlight { page, .. } | Mark::Note { page, .. } => *page,
            Mark::InsertedPage { after_page, .. } => *after_page,
        })
        .collect();
    pages.sort_unstable();
    pages.dedup();

    if let Some(pdf) = bundle.source_pdf() {
        // Write the source PDF once for pdftoppm / pdftotext to read.
        let tmp = tempfile::tempdir()?;
        let pdf_path = tmp.path().join("source.pdf");
        std::fs::write(&pdf_path, pdf)?;

        for idx in pages {
            // bundle page index → 1-based source PDF page (identity redirection).
            let pdf_page = idx + 1;
            let raw = match rasterize_page(&pdf_path, pdf_page) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("rmdigest: rasterize page {pdf_page} failed ({e:#}); skipping image");
                    continue;
                }
            };
            // Highlight texts on this page, located via the page's own text layer.
            let phrases: Vec<&str> = marks
                .iter()
                .filter_map(|m| match m {
                    Mark::Highlight { page, text, .. } if *page == idx => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            let composite = match word_boxes(&pdf_path, pdf_page) {
                Ok(words) => paint_phrases(&raw, &words, &phrases).unwrap_or(raw),
                Err(_) => raw,
            };
            let name = format!("/assets/p{idx}.png");
            assets.push((name.clone(), composite));
            s.push_str(&format!(
                "#pagebreak()\n#heading(level: 2, outlined: true)[#kick(\"page {pg}\", rgb(120,120,120))] #label(\"p{idx}\")\n#v(4pt)\n#image(\"{name}\", width: 100%)\n",
                pg = pdf_page, idx = idx, name = name,
            ));
        }
    }

    Ok((s, assets))
}

/// Darken a bright pen color so kicker text stays legible on warm paper.
fn darken(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let bright = (r as u16 + g as u16 + b as u16) / 3;
    let f = if bright > 200 {
        0.55
    } else if bright > 150 {
        0.75
    } else {
        1.0
    };
    (
        (r as f32 * f) as u8,
        (g as f32 * f) as u8,
        (b as f32 * f) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wb(text: &str, x0: f64) -> WordBox {
        WordBox {
            text: text.into(),
            x0,
            y0: 10.0,
            x1: x0 + 20.0,
            y1: 22.0,
        }
    }

    #[test]
    fn norm_strips_punct_and_lowercases() {
        assert_eq!(norm("(844)"), "844");
        assert_eq!(norm("24/7"), "247");
        assert_eq!(norm("Roadside"), "roadside");
    }

    #[test]
    fn phrase_boxes_matches_contiguous_run() {
        // Page has a distractor "Roadside" earlier, then the real run.
        let words = vec![
            wb("Roadside", 0.0), // distractor (not followed by Assistance)
            wb("Service", 30.0),
            wb("Roadside", 100.0), // start of the real match
            wb("Assistance", 130.0),
            wb("24/7", 170.0),
        ];
        let boxes = phrase_boxes(&words, "Roadside Assistance 24/7", 72.0);
        // 72 dpi → 1pt = 1px, so x positions are the run's word x0s.
        assert_eq!(boxes.len(), 3, "should match the 3-word contiguous run");
        assert!(
            (boxes[0].0 - 100.0).abs() < 0.01,
            "first box at x=100, got {}",
            boxes[0].0
        );
    }

    #[test]
    fn phrase_boxes_falls_back_to_token_matches() {
        // No contiguous run; each token appears standalone.
        let words = vec![wb("alpha", 0.0), wb("zzz", 30.0), wb("beta", 60.0)];
        let boxes = phrase_boxes(&words, "alpha beta", 72.0);
        assert_eq!(boxes.len(), 2);
    }

    #[test]
    fn build_linked_produces_hyperlinked_multipage_pdf() {
        // Real fixture: 1 source page, 4 highlighter highlights ("ARCHIVE" + body).
        let bundle = rmfiles::Bundle::open(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/stamped-labels.rmdoc"
        )))
        .expect("open fixture");
        let marks =
            crate::extract::extract(&bundle, &(0..bundle.pages().len()).collect::<Vec<_>>())
                .expect("extract");
        let meta = DigestMeta {
            title: "T".into(),
            author: String::new(),
            n_highlights: marks.len(),
            n_notes: 0,
            date_range: String::new(),
        };
        let (src, assets) =
            build_linked(&meta, &marks, &bundle, &crate::device::MOVE).expect("build_linked");
        let pdf = crate::render::compile(&src, &assets).expect("compile");
        let doc = lopdf::Document::load_mem(&pdf).expect("valid pdf");
        // cover + entries + at least one rasterized source page.
        assert!(doc.get_pages().len() >= 2, "expected multi-page output");
        // typst #link must have emitted at least one /Link annotation.
        let has_link = doc.objects.values().any(|o| {
            o.as_dict()
                .ok()
                .and_then(|d| d.get(b"Subtype").ok())
                .and_then(|s| s.as_name().ok())
                .map(|n| n == b"Link")
                .unwrap_or(false)
        });
        assert!(has_link, "digest entries must carry GoTo /Link annotations");
    }
}
