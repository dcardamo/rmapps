//! Build the "Digest" output: ONE lightweight pure-typst document.
//!
//! Each highlight is shown **in context** — the surrounding sentences pulled
//! from the source PDF's text layer (`pdftotext`), with the highlighted span
//! itself emphasized via typst `#highlight`. This is fast (no page
//! rasterization), small, and self-contained.
//!
//! Highlights do NOT embed whole pages (reMarkable can't link between documents,
//! and a full image-copy of the book is far too heavy) — the context window
//! recalls the passage instead.
//!
//! Pen notes ARE flattened onto a crop of their source page: the ink is drawn
//! over the rasterized page and cropped to the vertical span the strokes cover
//! (full width), so circling/bracketing/underlining content pulls that content
//! into the digest. Notes on inserted blank pages are ink-only. Handwriting is
//! not transcribed (image-only, by design).
use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, Context};
use rmfiles::{Bundle, Stroke};

use crate::device::Device;
use crate::extract::Mark;
use crate::ink::{ink_bbox, render_strokes, render_strokes_on_canvas, Background, InkOpts};

/// DPI at which note pages are rasterized for flattening. A stroke point in the
/// 226-dpi device scene maps to `scene * RASTER_DPI / 226` pixels.
const RASTER_DPI: f32 = 150.0;

/// Metadata shown on the digest cover page.
pub struct DigestMeta {
    pub title: String,
    pub author: String,
    pub n_highlights: usize,
    pub n_notes: usize,
    pub date_range: String,
}

/// Compiled typst source plus its image assets (`(virtual_path, png_bytes)`).
pub type TypstDoc = (String, Vec<(String, Vec<u8>)>);

/// How much context (characters) to try to include before and after a highlight,
/// snapped outward to sentence boundaries.
const CTX_BEFORE: usize = 240;
const CTX_AFTER: usize = 240;

/// Accent colour (typst expression) for the kicker + vertical bar — a clear blue
/// that pops against the warm paper better than the pale highlight tint.
const BLUE: &str = "rgb(54, 110, 190)";

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

/// Whitespace-collapsed plain text of every page of `pdf_path`, one entry per
/// page (pages are split on the form-feed `pdftotext` emits between them).
///
/// We search the whole document for each highlight's text rather than trusting a
/// bundle-page → source-page mapping: inserting note-pages on the device shifts
/// the bundle page indices, but the highlighted *text* still uniquely locates the
/// real source page (and its surrounding context).
fn document_pages(pdf_path: &std::path::Path) -> Vec<String> {
    let out = match Command::new("pdftotext")
        .args([pdf_path.to_str().unwrap(), "-"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };
    String::from_utf8_lossy(&out.stdout)
        .split('\u{000C}') // form feed = page break
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

/// Find the 0-based page index containing `phrase` (case-insensitive).
fn find_page(pages: &[String], phrase: &str) -> Option<usize> {
    let needle = phrase.trim().to_lowercase();
    if needle.is_empty() {
        return None;
    }
    pages
        .iter()
        .position(|p| p.to_lowercase().contains(&needle))
}

/// Nudge a byte index to the nearest char boundary at or before `i`.
fn floor_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Nudge a byte index to the nearest char boundary at or after `i`.
fn ceil_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Find `highlight` inside `text` and return `(before, highlighted, after)`,
/// where `before`/`after` are the surrounding sentences (snapped to sentence
/// boundaries within a character budget). Returns `None` if the highlight text
/// isn't found in the page text (e.g. an ink highlight with no text layer).
fn context(text: &str, highlight: &str) -> Option<(String, String, String)> {
    let hl = highlight.trim();
    if hl.is_empty() {
        return None;
    }
    // Case-insensitive search; positions hold for ASCII-dominant PDF text.
    let lower = text.to_lowercase();
    let start = lower.find(&hl.to_lowercase())?;
    let end = (start + hl.len()).min(text.len());
    let end = ceil_boundary(text, end);

    // Expand left to a sentence start within the budget.
    let lo = floor_boundary(text, start.saturating_sub(CTX_BEFORE));
    let b_start = text[lo..start]
        .rfind(['.', '!', '?'])
        .map(|i| lo + i + 1)
        .unwrap_or(lo);
    let b_start = ceil_boundary(text, b_start);

    // Expand right to a sentence end within the budget.
    let hi = ceil_boundary(text, (end + CTX_AFTER).min(text.len()));
    let a_end = text[end..hi]
        .find(['.', '!', '?'])
        .map(|i| end + i + 1)
        .unwrap_or(hi);
    let a_end = ceil_boundary(text, a_end);

    let before = text[b_start..start].trim().to_string();
    let highlighted = text[start..end].to_string();
    let after = text[end..a_end].trim().to_string();
    Some((before, highlighted, after))
}

/// Rasterize one 1-based page of the PDF at `RASTER_DPI` to a PNG.
fn rasterize_page(pdf_path: &Path, page_1based: usize) -> anyhow::Result<Vec<u8>> {
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("p");
    let n = page_1based.to_string();
    let dpi = (RASTER_DPI as u32).to_string();
    let st = Command::new("pdftoppm")
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
    if !st.success() {
        return Err(anyhow!("pdftoppm failed for page {page_1based}"));
    }
    std::fs::read(prefix.with_extension("png")).context("read page png")
}

/// Flatten `strokes` onto the source page: rasterize the page, draw the ink on
/// top, and crop to the vertical span the strokes cover (full page width) so the
/// circled/bracketed/underlined content between the marks comes along.
fn flatten_note(
    pdf_path: &Path,
    page_1based: usize,
    strokes: &[&Stroke],
) -> anyhow::Result<Vec<u8>> {
    let page_png = rasterize_page(pdf_path, page_1based)?;
    let mut page = tiny_skia::Pixmap::decode_png(&page_png).context("decode page png")?;
    let (w, h) = (page.width(), page.height());

    // Stroke scene coords → page pixels: px = scene_x*scale + W/2, py = scene_y*scale.
    let scale = RASTER_DPI / 226.0;
    let origin = (-(w as f32 / 2.0) / scale, 0.0);
    let opts = InkOpts {
        background: Background::Transparent,
        scale,
        margin_px: 0,
    };
    let overlay_png = render_strokes_on_canvas(strokes, origin, w, h, &opts)?;
    let overlay = tiny_skia::Pixmap::decode_png(&overlay_png).context("decode overlay")?;
    page.draw_pixmap(
        0,
        0,
        overlay.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
    );

    // Crop to the VERTICAL range the ink spans (+ a small margin), at FULL page
    // width. This captures everything "in between" the user's marks — circled
    // content, and the lines they bracketed/underlined beside the text — rather
    // than just the thin strip the strokes themselves occupy.
    let (_minx, miny, _maxx, maxy) = ink_bbox(strokes).ok_or_else(|| anyhow!("no ink"))?;
    let to_y = |sy: f32| sy * scale;
    let m = (RASTER_DPI * 0.12) as i64;
    let cy0 = ((to_y(miny) as i64) - m).clamp(0, h as i64) as u32;
    let cy1 = ((to_y(maxy) as i64) + m).clamp(0, h as i64) as u32;
    let (cx0, cw) = (0u32, w);
    let ch = cy1.saturating_sub(cy0).max(1);

    let composited = page.encode_png().context("encode composited")?;
    let img = image::load_from_memory(&composited).context("load composited")?;
    let cropped = image::imageops::crop_imm(&img, cx0, cy0, cw, ch).to_image();
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(cropped).write_to(&mut out, image::ImageFormat::Png)?;
    Ok(out.into_inner())
}

/// Render `strokes` alone on white (for notes on inserted blank pages).
fn ink_only(strokes: &[&Stroke]) -> anyhow::Result<Vec<u8>> {
    let opts = InkOpts {
        background: Background::White,
        ..Default::default()
    };
    render_strokes(strokes, &opts)
}

/// Build the digest typst source + image assets.
pub fn build_linked(
    meta: &DigestMeta,
    marks: &[Mark],
    bundle: &Bundle,
    device: &Device,
) -> anyhow::Result<TypstDoc> {
    // Stage the source PDF once: its whole text (for locating highlights) and
    // its path (for rasterizing note pages to flatten ink onto).
    let tmp = tempfile::tempdir()?;
    let (pdf_path, doc_pages): (Option<std::path::PathBuf>, Vec<String>) =
        if let Some(pdf) = bundle.source_pdf() {
            let p = tmp.path().join("source.pdf");
            std::fs::write(&p, pdf).context("stage source pdf")?;
            let pages = document_pages(&p);
            (Some(p), pages)
        } else {
            (None, Vec::new())
        };

    let mut assets: Vec<(String, Vec<u8>)> = Vec::new();
    let mut s = String::new();

    // ── Preamble + cover ───────────────────────────────────────────────────
    s.push_str(&format!(
        r##"#set document(title: "{title}", author: "{author}")
#set page(width: {pw}pt, height: {ph}pt, margin: (x: 7mm, y: 8mm), fill: rgb(250, 249, 246))
#set text(font: "Newsreader", size: 11pt, fill: rgb(26, 26, 26), lang: "en", hyphenate: false)
#set par(leading: 0.62em, spacing: 0.7em, justify: false)
#set heading(outlined: true)
#let kick(t, c) = text(font: "Hanken Grotesk", size: 7pt, weight: "semibold", tracking: 2pt, fill: c)[#upper(t)]
#show heading.where(level: 2): it => block(above: 20pt, below: 6pt, width: 100%)[#it.body]
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

    // ── One block per mark ─────────────────────────────────────────────────
    for m in marks {
        match m {
            Mark::Highlight { page, text, rgb } => {
                // Wash colour = the device's exact highlighter colour. The kicker
                // and vertical bar use a fixed blue accent (it pops more than the
                // pale highlight tint).
                let (r, g, b) = *rgb;
                // Locate the highlight in the source text → real page + context.
                // Fall back to the bundle page index if the text isn't found.
                let found = find_page(&doc_pages, text);
                let display_page = found.map(|p| p + 1).unwrap_or(page + 1);
                let ctx = found.and_then(|p| context(&doc_pages[p], text));
                s.push_str(&format!(
                    "#heading(level: 2, outlined: true)[#kick(\"page {pg}\", {BLUE})]\n",
                    pg = display_page,
                ));
                // Render the highlight inside its surrounding sentences.
                match ctx {
                    Some((before, hl, after)) => {
                        s.push_str(&format!(
                            "#block(width: 100%, inset: (left: 11pt, right: 4pt, y: 4pt), stroke: (left: 3pt + {BLUE}))[\n\
                             #par(leading: 0.66em)[#text(font: \"Newsreader\", size: 11.5pt, fill: rgb(70,70,70))[{bef} #highlight(fill: rgb({r},{g},{b}), extent: 1pt)[#text(fill: rgb(26,26,26))[{hl}]] {aft}]]]\n",
                            bef = esc_markup(&before),
                            hl = esc_markup(&hl),
                            aft = esc_markup(&after),
                        ));
                    }
                    None => {
                        // No text layer match — wash the bare quote itself.
                        let lq = '\u{201C}';
                        let rq = '\u{201D}';
                        s.push_str(&format!(
                            "#block(width: 100%, inset: (left: 11pt, right: 4pt, y: 4pt), stroke: (left: 3pt + {BLUE}))[#par(leading: 0.7em)[#text(font: \"Newsreader\", size: 12pt)[#highlight(fill: rgb({r},{g},{b}), extent: 1pt)[{lq}{t}{rq}]]]]\n",
                            t = esc_markup(text),
                        ));
                    }
                }
                s.push_str(
                    "#v(8pt)\n#line(length: 22%, stroke: 0.4pt + rgb(200,200,200))\n#v(2pt)\n",
                );
            }
            Mark::Note {
                page,
                source_page,
                strokes,
            } => {
                let refs: Vec<&Stroke> = strokes.iter().collect();
                // Flatten onto the backing source page (so circled content is
                // included); fall back to ink-only for inserted blank pages or if
                // rasterization fails.
                let png = match (&pdf_path, *source_page) {
                    (Some(path), Some(sp)) => {
                        flatten_note(path, sp + 1, &refs).or_else(|_| ink_only(&refs))?
                    }
                    _ => ink_only(&refs)?,
                };
                let display_page = source_page.map(|p| p + 1).unwrap_or(page + 1);
                let name = format!("/assets/note-{}.png", assets.len());
                assets.push((name.clone(), png));
                s.push_str(&format!(
                    "#heading(level: 3, outlined: true)[#kick(\"page {pg} · note\", {BLUE})]\n\
                     #block(stroke: 0.5pt + rgb(200,200,200), inset: 5pt, radius: 2pt)[#image(\"{name}\", width: 100%)]\n#v(8pt)\n",
                    pg = display_page, name = name,
                ));
            }
        }
    }

    Ok((s, assets))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_extracts_surrounding_sentences() {
        let text = "Intro sentence one. The roadside assistance line is open 24/7 for members. \
                    Another following sentence here. And one more after that.";
        let (before, hl, after) = context(text, "roadside assistance line is open 24/7").unwrap();
        assert!(hl.to_lowercase().contains("roadside assistance"));
        // `before` should include the start of the containing sentence ("The").
        assert!(before.starts_with("The"), "before={before:?}");
        // `after` should carry on to a sentence end ("for members.").
        assert!(after.contains("members"), "after={after:?}");
    }

    #[test]
    fn context_none_when_text_absent() {
        let text = "Nothing relevant here at all.";
        assert!(context(text, "roadside assistance").is_none());
    }

    #[test]
    fn find_page_locates_text_by_content_not_index() {
        // Simulates inserted note-pages shifting indices: the phrase is on the
        // 3rd source page regardless of where it sits in the bundle.
        let pages = vec![
            "front matter".to_string(),
            "table of contents".to_string(),
            "Roadside Assistance 24/7 call us".to_string(),
        ];
        assert_eq!(find_page(&pages, "roadside assistance 24/7"), Some(2));
        assert_eq!(find_page(&pages, "not present"), None);
    }

    #[test]
    fn build_digest_compiles_with_context_and_is_multipage() {
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
            build_linked(&meta, &marks, &bundle, &crate::device::MOVE).expect("build");
        let pdf = crate::render::compile(&src, &assets).expect("compile");
        let doc = lopdf::Document::load_mem(&pdf).expect("valid pdf");
        assert!(doc.get_pages().len() >= 2);
        // The stamped-labels body sentence should appear as surrounding context.
        assert!(
            src.contains("highlight("),
            "expected typst #highlight in output"
        );
    }
}
