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

/// Toggles for which overlays `inspect_with_opts` draws. All default to `true` so
/// the default `InspectOpts` produces the same "show everything" view that
/// `inspect()` historically rendered.
#[derive(Debug, Clone, Copy)]
pub struct ShowFlags {
    pub regions: bool,
    pub links: bool,
    pub synth_strokes: bool,
    pub attributed_strokes: bool,
}

impl Default for ShowFlags {
    fn default() -> Self {
        Self {
            regions: true,
            links: true,
            synth_strokes: true,
            attributed_strokes: true,
        }
    }
}

/// Options for `inspect_with_opts`.
///
/// `layers` is a forward-looking filter for when strokes carry layer metadata
/// (rm-scene layer names). It is currently accepted but ignored — all strokes
/// are treated as one layer until that metadata is plumbed through.
#[derive(Debug, Clone, Default)]
pub struct InspectOpts {
    pub layers: Option<Vec<String>>,
    pub show: ShowFlags,
}

/// Render page `page` of `doc` and composite overlays:
///
/// * region rects in blue (when `opts.show.regions`),
/// * PDF link rects in purple (when `opts.show.links`),
/// * synthetic ink strokes — pen red, highlighter yellow — (when `opts.show.synth_strokes`),
/// * attributed ink strokes in green (when `opts.show.attributed_strokes`).
///
/// PDF-space y (origin bottom-left) is flipped to image y (top-left).
///
/// `synth` and `attributed` are stroke sets supplied by later tasks (10 / 11);
/// pass `&[]` until then. `links` are PDF-space rects `(x0, y0, x1, y1)` for the
/// page being inspected.
pub fn inspect_with_opts(
    doc: &PagedDocument,
    manifest: &Manifest,
    links: &[(f64, f64, f64, f64)],
    synth: &[Stroke],
    attributed: &[Stroke],
    page: usize,
    opts: &InspectOpts,
) -> Result<Vec<u8>> {
    let (mut img, page_h_pt) = rasterize(doc, page)?;
    // Forward-looking: layer filtering will apply once strokes carry layer tags.
    let _ = opts.layers.as_ref();

    // Convert PDF-space (pt, y-up) to image-space (px, y-down).
    let to_px = |x_pt: f64, y_pt: f64| -> (i64, i64) {
        let px = (x_pt as f32 * SCALE).round() as i64;
        let py = ((page_h_pt - y_pt as f32) * SCALE).round() as i64;
        (px, py)
    };

    if opts.show.regions {
        let blue = Rgba([0_u8, 80, 220, 255]);
        for r in &manifest.regions {
            if r.page != page {
                continue;
            }
            let (x0, y0) = to_px(r.rect.x0, r.rect.y1);
            let (x1, y1) = to_px(r.rect.x1, r.rect.y0);
            draw_rect_outline(&mut img, x0, y0, x1, y1, blue);
        }
    }

    if opts.show.links {
        let purple = Rgba([180_u8, 0, 200, 255]);
        for &(lx0, ly0, lx1, ly1) in links {
            let (x0, y0) = to_px(lx0, ly1);
            let (x1, y1) = to_px(lx1, ly0);
            draw_rect_outline(&mut img, x0, y0, x1, y1, purple);
        }
    }

    if opts.show.synth_strokes {
        for s in synth {
            let color = if s.highlighter {
                Rgba([230_u8, 210, 0, 255])
            } else {
                Rgba([220_u8, 0, 0, 255])
            };
            draw_stroke_color(&mut img, s, color, &to_px);
        }
    }

    if opts.show.attributed_strokes {
        let green = Rgba([0_u8, 180, 60, 255]);
        for s in attributed {
            draw_stroke_color(&mut img, s, green, &to_px);
        }
    }

    encode_png(&img)
}

/// Back-compat wrapper: render page 0 with regions + synth strokes, no links,
/// no attributed strokes. Delegates to `inspect_with_opts`.
pub fn inspect(doc: &PagedDocument, manifest: &Manifest, ink: &[Stroke]) -> Result<Vec<u8>> {
    inspect_with_opts(doc, manifest, &[], ink, &[], 0, &InspectOpts::default())
}

fn draw_stroke_color<F: Fn(f64, f64) -> (i64, i64)>(
    img: &mut RgbaImage,
    s: &Stroke,
    color: Rgba<u8>,
    to_px: &F,
) {
    let mut prev: Option<(i64, i64)> = None;
    for p in &s.points {
        let cur = to_px(p.x, p.y);
        if let Some(pp) = prev {
            draw_line(img, pp.0, pp.1, cur.0, cur.1, color);
        }
        prev = Some(cur);
    }
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
