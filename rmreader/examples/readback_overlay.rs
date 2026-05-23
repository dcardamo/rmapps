//! Visual debugger: render PDF pages with highlighter ink + action-label cells
//! + snap-to-text GlyphRange rectangles.
//!
//! Usage: `cargo run --example readback_overlay -- <bundle.rmdoc> <out_dir>`
//!
//! For each page that has at least one highlighter stroke or text highlight:
//!   - renders the source PDF page to a PNG via pdftoppm
//!   - draws action-label cell rects in BLUE
//!   - draws each highlighter stroke's polyline in RED, plus its bbox as a thin
//!     red rectangle
//!   - draws each GlyphRange text-highlight rectangle in GREEN
//!   - prints per-stroke: PDF bbox, center_y, and which label cell (if any) it
//!     hits — mirroring the actual classification logic
//!   - prints per-GlyphRange: text, PDF bbox, and words from the PDF text layer
//!     that sit under the snap rectangle
//!
//! Saves `<out_dir>/page_<N>.png` for every matching page.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use image::{ImageBuffer, Rgb, RgbImage};
use rmreader::{
    embed,
    manifest::EmbeddedManifest,
    readback::{
        coords::{PdfRect, Transform},
        textlayer::TextLayer,
    },
};

const RENDER_DPI: f64 = 150.0;

// ─── drawing helpers ──────────────────────────────────────────────────────────

fn put(img: &mut RgbImage, x: i64, y: i64, color: Rgb<u8>) {
    let (w, h) = img.dimensions();
    if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
        img.put_pixel(x as u32, y as u32, color);
    }
}

/// Bresenham line between two pixel points.
fn draw_line(img: &mut RgbImage, x0: i64, y0: i64, x1: i64, y1: i64, color: Rgb<u8>) {
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx: i64 = if x0 < x1 { 1 } else { -1 };
    let sy: i64 = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;
    let (mut cx, mut cy) = (x0, y0);
    loop {
        put(img, cx, cy, color);
        if cx == x1 && cy == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            cx += sx;
        }
        if e2 < dx {
            err += dx;
            cy += sy;
        }
    }
}

/// Axis-aligned rectangle (outline only), `thickness` pixels wide.
fn draw_rect(
    img: &mut RgbImage,
    px0: i64,
    py0: i64,
    px1: i64,
    py1: i64,
    color: Rgb<u8>,
    thickness: i64,
) {
    for t in 0..thickness {
        draw_line(img, px0 + t, py0 + t, px1 - t, py0 + t, color); // top
        draw_line(img, px0 + t, py1 - t, px1 - t, py1 - t, color); // bottom
        draw_line(img, px0 + t, py0 + t, px0 + t, py1 - t, color); // left
        draw_line(img, px1 - t, py0 + t, px1 - t, py1 - t, color); // right
    }
}

/// Draw a polyline (consecutive points).
fn draw_polyline(img: &mut RgbImage, pts: &[(i64, i64)], color: Rgb<u8>) {
    for w in pts.windows(2) {
        draw_line(img, w[0].0, w[0].1, w[1].0, w[1].1, color);
    }
}

// ─── coordinate helpers ───────────────────────────────────────────────────────

/// PDF point → pixel (image origin top-left, y-axis flipped).
fn pdf_to_pixel(px: f64, py: f64, page_h: f64, scale: f64) -> (i64, i64) {
    ((px * scale) as i64, ((page_h - py) * scale) as i64)
}

/// PdfRect → pixel rect (top-left + bottom-right).
fn rect_to_pixels(r: PdfRect, page_h: f64, scale: f64) -> (i64, i64, i64, i64) {
    let (x0, y0) = pdf_to_pixel(r.x0, r.y1, page_h, scale); // y1 is the top in PDF
    let (x1, y1) = pdf_to_pixel(r.x1, r.y0, page_h, scale); // y0 is the bottom in PDF
    (x0, y0, x1, y1)
}

// ─── cell classification (mirrors classify.rs) ───────────────────────────────

fn cell_for_cy(manifest: &EmbeddedManifest, bbox: &PdfRect) -> String {
    let cy = (bbox.y0 + bbox.y1) / 2.0;
    let x_overlap =
        |b_x0: f64, b_x1: f64| -> f64 { (bbox.x1.min(b_x1) - bbox.x0.max(b_x0)).max(0.0) };
    manifest
        .label_rects
        .iter()
        .filter(|lr| cy >= lr.rect.y0 && cy <= lr.rect.y1)
        .map(|lr| (lr, x_overlap(lr.rect.x0, lr.rect.x1)))
        .filter(|(_, ox)| *ox > 0.0)
        .max_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(lr, _)| lr.kind.clone())
        .unwrap_or_else(|| "none".to_string())
}

// ─── page rendering ───────────────────────────────────────────────────────────

/// Shell out to pdftoppm to render page `page_1based` (1-based) of `pdf_path`.
/// Returns the loaded image, or an error.
fn render_page_png(pdf_path: &Path, page_1based: usize, dpi: u32) -> anyhow::Result<RgbImage> {
    let tmp_dir = tempfile::tempdir()?;
    let prefix = tmp_dir.path().join("pg");
    let prefix_str = prefix.to_str().context("prefix path not UTF-8")?;
    let page_s = page_1based.to_string();

    let status = Command::new("pdftoppm")
        .args([
            "-f",
            &page_s,
            "-l",
            &page_s,
            "-r",
            &dpi.to_string(),
            "-png",
            pdf_path.to_str().context("pdf path not UTF-8")?,
            prefix_str,
        ])
        .status()
        .context("failed to run pdftoppm")?;

    if !status.success() {
        anyhow::bail!("pdftoppm exited with status {status}");
    }

    // pdftoppm writes <prefix>-<NN>.png, zero-padded to however many digits are needed.
    // Glob for the first .png in the temp dir.
    let png_path: PathBuf = std::fs::read_dir(tmp_dir.path())?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().map(|x| x == "png").unwrap_or(false))
        .context("pdftoppm produced no PNG")?;

    let img = image::open(&png_path)
        .with_context(|| format!("failed to open {}", png_path.display()))?
        .into_rgb8();
    Ok(img)
}

// ─── first_page_size (mirrors readback/mod.rs) ────────────────────────────────

fn first_page_size(doc: &lopdf::Document) -> Option<(f64, f64)> {
    let page_id = *doc.get_pages().values().next()?;
    let mb = doc
        .get_dictionary(page_id)
        .ok()?
        .get(b"MediaBox")
        .ok()?
        .as_array()
        .ok()?;
    let num = |o: &lopdf::Object| {
        o.as_float()
            .map(|f| f as f64)
            .or_else(|_| o.as_i64().map(|i| i as f64))
            .ok()
    };
    Some((
        num(mb.get(2)?)? - num(mb.first()?)?,
        num(mb.get(3)?)? - num(mb.get(1)?)?,
    ))
}

// ─── main ─────────────────────────────────────────────────────────────────────

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let bundle_path = args
        .next()
        .expect("usage: readback_overlay <bundle.rmdoc> <out_dir>");
    let out_dir = args
        .next()
        .expect("usage: readback_overlay <bundle.rmdoc> <out_dir>");
    std::fs::create_dir_all(&out_dir)?;

    let bundle = rmfiles::Bundle::open(Path::new(&bundle_path))?;
    let Some(pdf_bytes) = bundle.source_pdf().map(|b| b.to_vec()) else {
        eprintln!("no source PDF in bundle; nothing to render");
        return Ok(());
    };

    // Write source PDF to a temp file so pdftoppm can read it.
    let mut pdf_tmp = tempfile::NamedTempFile::with_suffix(".pdf")?;
    pdf_tmp.write_all(&pdf_bytes)?;
    pdf_tmp.flush()?;
    let pdf_tmp_path = pdf_tmp.path().to_path_buf();

    let doc = lopdf::Document::load_mem(&pdf_bytes)?;
    let manifest_opt = embed::read(&doc)?;

    // Page size from the source PDF MediaBox; fall back to device canvas size.
    let page = first_page_size(&doc).unwrap_or_else(|| bundle.canvas_size());
    let (page_w, page_h) = page;
    let transform = Transform::new(page);
    let scale = RENDER_DPI / 72.0;

    // Build the dummy manifest so label_rects is always valid (even if empty).
    let empty_manifest = EmbeddedManifest {
        schema_version: 1,
        collection: String::new(),
        docs: Vec::new(),
        label_rects: Vec::new(),
    };
    let manifest = manifest_opt.as_ref().unwrap_or(&empty_manifest);

    let blue = Rgb([0u8, 0u8, 220u8]);
    let red = Rgb([220u8, 0u8, 0u8]);
    let green = Rgb([0u8, 200u8, 0u8]);

    // Extract the PDF text layer once for GlyphRange analysis.
    let textlayer = match TextLayer::extract(&pdf_bytes) {
        Ok(tl) => tl,
        Err(e) => {
            eprintln!("warning: could not extract PDF text layer: {e:#}");
            TextLayer::default()
        }
    };

    let mut pages_written = 0usize;

    for pg in bundle.pages() {
        let scene = match pg.scene()? {
            Some(s) => s,
            None => continue,
        };

        let hl_strokes: Vec<_> = scene
            .strokes()
            .into_iter()
            .filter(|s| s.is_highlighter())
            .collect();
        let text_highlights = scene.text_highlights();

        // Render a page only if it has ink highlights or GlyphRange highlights.
        if hl_strokes.is_empty() && text_highlights.is_empty() {
            continue;
        }

        let page_1based = pg.index + 1;
        let out_path = format!("{}/page_{}.png", out_dir, pg.index);

        // Render page to PNG.
        let mut img: RgbImage = match render_page_png(&pdf_tmp_path, page_1based, RENDER_DPI as u32)
        {
            Ok(i) => i,
            Err(e) => {
                eprintln!(
                    "warning: failed to render page {} from PDF: {e:#}",
                    pg.index
                );
                // Fall back to a blank canvas sized to match the expected output.
                let w = (page_w * scale) as u32;
                let h = (page_h * scale) as u32;
                ImageBuffer::from_pixel(w.max(1), h.max(1), Rgb([240u8, 240u8, 240u8]))
            }
        };

        // Draw label_rects in BLUE.
        for lr in &manifest.label_rects {
            let r = PdfRect {
                x0: lr.rect.x0,
                y0: lr.rect.y0,
                x1: lr.rect.x1,
                y1: lr.rect.y1,
            };
            let (px0, py0, px1, py1) = rect_to_pixels(r, page_h, scale);
            draw_rect(&mut img, px0, py0, px1, py1, blue, 2);
        }

        // Draw each highlighter stroke in RED.
        for stroke in &hl_strokes {
            // Transform all points to PDF space.
            let pdf_pts: Vec<(f64, f64)> = stroke
                .points
                .iter()
                .map(|p| transform.device_to_pdf(p.x as f64, p.y as f64))
                .collect();

            // Draw polyline.
            let pixel_pts: Vec<(i64, i64)> = pdf_pts
                .iter()
                .map(|&(px, py)| pdf_to_pixel(px, py, page_h, scale))
                .collect();
            draw_polyline(&mut img, &pixel_pts, red);

            // Compute bbox and draw it.
            if let Some(bbox) =
                transform.pdf_bbox(stroke.points.iter().map(|p| (p.x as f64, p.y as f64)))
            {
                let (bx0, by0, bx1, by1) = rect_to_pixels(bbox, page_h, scale);
                draw_rect(&mut img, bx0, by0, bx1, by1, red, 1);

                // Classification report.
                let cell = cell_for_cy(manifest, &bbox);
                let cy = (bbox.y0 + bbox.y1) / 2.0;
                println!(
                    "page={} bbox=[{:.1},{:.1},{:.1},{:.1}] center_y={:.1} -> cell={}",
                    pg.index, bbox.x0, bbox.y0, bbox.x1, bbox.y1, cy, cell
                );
            }
        }

        // Draw each GlyphRange text-highlight rectangle in GREEN and print analysis.
        for hl in &text_highlights {
            for rect in &hl.rectangles {
                // Rect corners in device space: (x, y) top-left, (x+w, y+h) bottom-right.
                let (pdf_x0, pdf_y0) = transform.device_to_pdf(rect.x, rect.y);
                let (pdf_x1, pdf_y1) = transform.device_to_pdf(rect.x + rect.w, rect.y + rect.h);

                // After the flip (device y-down → PDF y-up) the smaller device y maps to
                // the larger PDF y, so normalise into a proper PdfRect (y0 < y1).
                let bbox = PdfRect {
                    x0: pdf_x0.min(pdf_x1),
                    y0: pdf_y0.min(pdf_y1),
                    x1: pdf_x0.max(pdf_x1),
                    y1: pdf_y0.max(pdf_y1),
                };

                let (px0, py0, px1, py1) = rect_to_pixels(bbox, page_h, scale);
                draw_rect(&mut img, px0, py0, px1, py1, green, 2);

                // Per-highlight analysis: stored text vs. words physically under bbox.
                let snippet: String = hl.text.chars().take(50).collect();
                let truncated = if hl.text.chars().count() > 50 {
                    format!("{snippet}…")
                } else {
                    snippet
                };
                let under = textlayer.words_under(pg.index, &bbox);
                let under_snippet: String = under.chars().take(80).collect();
                let under_display = if under.chars().count() > 80 {
                    format!("{under_snippet}…")
                } else {
                    under_snippet
                };
                println!(
                    "page={} GLYPH text={:?} pdf_bbox=[{:.1},{:.1},{:.1},{:.1}] textlayer_under={:?}",
                    pg.index, truncated, bbox.x0, bbox.y0, bbox.x1, bbox.y1, under_display
                );
            }
        }

        img.save(&out_path)
            .with_context(|| format!("failed to save {out_path}"))?;
        println!("wrote {out_path}");
        pages_written += 1;
    }

    if pages_written == 0 {
        println!("no pages with highlighter strokes or text highlights found");
    } else {
        println!("done: {pages_written} page(s) written to {out_dir}");
    }

    Ok(())
}
