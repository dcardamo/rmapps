//! Render a whole reMarkable bundle (pure-ink notebook or annotated doc) to a
//! standalone PDF — one rasterized page per bundle page, ink drawn on white.
//!
//! This is deliberately separate from the digest pipeline. `generate`/`extract`
//! target PDF-backed docs and SKIP pure-ink notebooks (no source PDF to anchor
//! marks to). For a straight export — e.g. migrating handwritten reMarkable
//! notebooks off the cloud — we instead flatten every page's ink onto a blank
//! white canvas so the notebook becomes an ordinary, viewable PDF.

use anyhow::{bail, Result};
use rmfiles::{Bundle, Stroke};

use crate::ink::{render_strokes_on_canvas, Background, InkOpts};
use crate::render::compile;

/// reMarkable scene units are ~226 dpi (1 unit ≈ 1 device pixel). Render at this
/// many pixels per scene unit: 2.0 ≈ 452 dpi, crisp without bloating the PDF.
const RENDER_SCALE: f32 = 2.0;
/// Scene-unit resolution, used to convert the canvas into PDF points.
const SCENE_DPI: f32 = 226.0;

/// Render every page of `bundle` to a white-background PDF and return its bytes.
/// Pages with no ink come out blank so pagination is preserved.
pub fn render_bundle_pdf(bundle: &Bundle) -> Result<Vec<u8>> {
    let pages = bundle.pages();
    if pages.is_empty() {
        bail!("notebook bundle has no pages");
    }

    // Canvas in scene units (device px @226dpi); defaults to (1404, 1872).
    let (cw, ch) = bundle.canvas_size();
    let scale = RENDER_SCALE;
    let w_px = ((cw as f32) * scale).round().max(1.0) as u32;
    let h_px = ((ch as f32) * scale).round().max(1.0) as u32;
    // Scene x is centered on 0 (px = scene_x*scale + W/2), y runs downward from
    // 0, so the scene point mapping to the canvas top-left is (-(W_px/2)/scale, 0).
    let origin = (-(w_px as f32 / 2.0) / scale, 0.0);
    // Physical page size in PDF points (72/in) from the scene-unit canvas.
    let page_w_pt = (cw as f32) / SCENE_DPI * 72.0;
    let page_h_pt = (ch as f32) / SCENE_DPI * 72.0;

    let mut assets: Vec<(String, Vec<u8>)> = Vec::with_capacity(pages.len());
    let mut src =
        format!("#set page(width: {page_w_pt}pt, height: {page_h_pt}pt, margin: 0pt)\n");

    for (i, page) in pages.iter().enumerate() {
        // Hold the Scene in a local so the borrowed `&Stroke`s outlive the render.
        let scene = page.scene()?;
        let strokes: Vec<&Stroke> = scene.as_ref().map(|s| s.strokes()).unwrap_or_default();
        let opts = InkOpts {
            background: Background::White,
            scale,
            margin_px: 0,
        };
        let png = render_strokes_on_canvas(&strokes, origin, w_px, h_px, &opts)?;
        let name = format!("/assets/page-{i}.png");
        assets.push((name.clone(), png));
        if i > 0 {
            src.push_str("#pagebreak()\n");
        }
        src.push_str(&format!("#image(\"{name}\", width: 100%, height: 100%)\n"));
    }

    compile(&src, &assets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stamped-labels.rmdoc")
    }

    #[test]
    fn renders_bundle_to_multipage_pdf() {
        let bundle = Bundle::open(&fixture()).expect("open fixture");
        let n_pages = bundle.pages().len();
        assert!(n_pages >= 1, "fixture should have pages");

        let pdf = render_bundle_pdf(&bundle).expect("render");
        assert!(pdf.starts_with(b"%PDF"), "output should be a PDF");

        let doc = lopdf::Document::load_mem(&pdf).expect("valid PDF");
        assert_eq!(
            doc.get_pages().len(),
            n_pages,
            "one PDF page per bundle page"
        );
    }
}
