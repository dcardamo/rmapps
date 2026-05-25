use image::{ImageFormat, Rgba, RgbaImage};
use inkapp_core::error::{Error, Result};
use inkapp_core::ink::Stroke;
use inkapp_core::manifest::Manifest;
use typst::layout::PagedDocument;

/// Pixels per point used for the inspector raster.
const SCALE: f32 = 2.0;

/// Rasterize one page of `doc` to an RGBA image plus its page height in points.
/// Shared by `render_page` (clean) and `inspect` (which then draws overlays).
fn rasterize(doc: &PagedDocument, page_index: usize) -> Result<(RgbaImage, f32)> {
    let page = doc
        .pages
        .get(page_index)
        .ok_or_else(|| Error::Render(format!("no page {page_index}")))?;
    let page_h_pt = page.frame.height().to_pt() as f32;
    // Rasterize via typst-render -> tiny_skia::Pixmap, then copy pixels into an
    // image::RgbaImage so we don't couple to typst-render's tiny-skia version.
    //
    // NOTE: tiny-skia stores pixels premultiplied; image::RgbaImage assumes
    // straight alpha. For the opaque (white-background) pages the harness renders
    // these are identical, so we copy bytes directly. Semi-transparent Typst
    // content would appear desaturated — acceptable for a debug/vision artifact.
    let pixmap = typst_render::render(page, SCALE);
    let img: RgbaImage =
        RgbaImage::from_raw(pixmap.width(), pixmap.height(), pixmap.data().to_vec())
            .ok_or_else(|| Error::Render("pixmap->image size mismatch".into()))?;
    Ok((img, page_h_pt))
}

fn encode_png(img: &RgbaImage) -> Result<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png)
        .map_err(|e| Error::Render(format!("png encode: {e}")))?;
    Ok(out.into_inner())
}

/// Render one page of `doc` to PNG bytes with **no overlays** (clean typographic
/// raster). `page_index` is 0-based. Used for golden renders of Display content.
pub fn render_page(doc: &PagedDocument, page_index: usize) -> Result<Vec<u8>> {
    let (img, _) = rasterize(doc, page_index)?;
    encode_png(&img)
}

/// Render page 0 of `doc` and composite region rects (blue) and ink strokes
/// (red for pen, yellow for highlighter) over it. Returns PNG bytes.
///
/// PDF-space y (origin bottom-left) is flipped to image y (top-left).
pub fn inspect(doc: &PagedDocument, manifest: &Manifest, ink: &[Stroke]) -> Result<Vec<u8>> {
    let (mut img, page_h_pt) = rasterize(doc, 0)?;

    // Convert PDF-space (pt, y-up) to image-space (px, y-down).
    let to_px = |x_pt: f64, y_pt: f64| -> (i64, i64) {
        let px = (x_pt as f32 * SCALE).round() as i64;
        // flip y: image y grows downward, PDF y grows upward.
        let py = ((page_h_pt - y_pt as f32) * SCALE).round() as i64;
        (px, py)
    };

    // Draw region outlines in blue.
    let blue = Rgba([0_u8, 80, 220, 255]);
    for r in &manifest.regions {
        if r.page != 0 {
            continue;
        }
        // r.rect.y1 is the top of the rect in PDF space → lowest image y.
        let (x0, y0) = to_px(r.rect.x0, r.rect.y1);
        let (x1, y1) = to_px(r.rect.x1, r.rect.y0);
        draw_rect_outline(&mut img, x0, y0, x1, y1, blue);
    }

    // Draw ink strokes: red for pen, yellow for highlighter.
    for s in ink {
        let color = if s.highlighter {
            Rgba([230_u8, 210, 0, 255])
        } else {
            Rgba([220_u8, 0, 0, 255])
        };
        let mut prev: Option<(i64, i64)> = None;
        for p in &s.points {
            let cur = to_px(p.x, p.y);
            if let Some(pp) = prev {
                draw_line(&mut img, pp.0, pp.1, cur.0, cur.1, color);
            }
            prev = Some(cur);
        }
    }

    encode_png(&img)
}

fn put(img: &mut RgbaImage, x: i64, y: i64, c: Rgba<u8>) {
    if x >= 0 && y >= 0 && (x as u32) < img.width() && (y as u32) < img.height() {
        img.put_pixel(x as u32, y as u32, c);
    }
}

fn draw_rect_outline(img: &mut RgbaImage, x0: i64, y0: i64, x1: i64, y1: i64, c: Rgba<u8>) {
    let (xa, xb) = (x0.min(x1), x0.max(x1));
    let (ya, yb) = (y0.min(y1), y0.max(y1));
    for x in xa..=xb {
        put(img, x, ya, c);
        put(img, x, yb, c);
    }
    for y in ya..=yb {
        put(img, xa, y, c);
        put(img, xb, y, c);
    }
}

/// Integer DDA line rasteriser.
fn draw_line(img: &mut RgbaImage, x0: i64, y0: i64, x1: i64, y1: i64, c: Rgba<u8>) {
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let steps = dx.max(dy).max(1);
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let x = (x0 as f64 + t * (x1 - x0) as f64).round() as i64;
        let y = (y0 as f64 + t * (y1 - y0) as f64).round() as i64;
        put(img, x, y, c);
    }
}
