//! Render reMarkable ink strokes to PNG images (tiny-skia).
use anyhow::anyhow;
use rmfiles::Stroke;
use tiny_skia::{
    Color, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke as SkStroke, Transform,
};

use crate::theme::pen_rgb;

/// Background for the rendered canvas.
pub enum Background {
    /// Transparent — for overlaying notes/highlights on a page.
    Transparent,
    /// Opaque white — for a standalone inserted note-page.
    White,
}

/// Rendering options.
pub struct InkOpts {
    /// Canvas background.
    pub background: Background,
    /// Pixels per scene unit (1.0 renders scene units 1:1 as pixels). A value
    /// around 1.0–2.0 gives crisp output; higher = larger image.
    pub scale: f32,
    /// Transparent margin (in pixels) added around the ink bbox when cropping.
    pub margin_px: u32,
}

impl Default for InkOpts {
    fn default() -> Self {
        Self {
            background: Background::Transparent,
            scale: 1.0,
            margin_px: 16,
        }
    }
}

/// Scene-space bounding box (min_x, min_y, max_x, max_y) over all stroke points.
/// `None` if there are no points.
pub fn ink_bbox(strokes: &[&Stroke]) -> Option<(f32, f32, f32, f32)> {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    let mut any = false;
    for stroke in strokes {
        for pt in &stroke.points {
            min_x = min_x.min(pt.x);
            min_y = min_y.min(pt.y);
            max_x = max_x.max(pt.x);
            max_y = max_y.max(pt.y);
            any = true;
        }
    }

    if any {
        Some((min_x, min_y, max_x, max_y))
    } else {
        None
    }
}

/// Render `strokes` cropped to their ink bbox (+ margin) and return PNG bytes.
/// Used for margin notes / highlights. Returns an error if there is nothing to draw.
pub fn render_strokes(strokes: &[&Stroke], opts: &InkOpts) -> anyhow::Result<Vec<u8>> {
    let (min_x, min_y, max_x, max_y) =
        ink_bbox(strokes).ok_or_else(|| anyhow!("no points to render"))?;

    let w = ((max_x - min_x) * opts.scale).ceil() as u32 + 2 * opts.margin_px;
    let h = ((max_y - min_y) * opts.scale).ceil() as u32 + 2 * opts.margin_px;

    if w == 0 || h == 0 {
        anyhow::bail!("canvas dimensions are zero");
    }

    let origin = (
        min_x - opts.margin_px as f32 / opts.scale,
        min_y - opts.margin_px as f32 / opts.scale,
    );

    render_strokes_on_canvas(strokes, origin, w, h, opts)
}

/// Render `strokes` onto a fixed canvas of `width_px` x `height_px` (no crop),
/// mapping scene coordinates through `origin` (the scene-space point that maps to
/// the canvas top-left) and `opts.scale`. Used for full-page inserted pages and
/// for page-absolute overlays (Task 10). Returns PNG bytes.
pub fn render_strokes_on_canvas(
    strokes: &[&Stroke],
    origin: (f32, f32),
    width_px: u32,
    height_px: u32,
    opts: &InkOpts,
) -> anyhow::Result<Vec<u8>> {
    let mut pixmap = Pixmap::new(width_px, height_px)
        .ok_or_else(|| anyhow!("failed to create pixmap ({width_px}x{height_px})"))?;

    if matches!(opts.background, Background::White) {
        pixmap.fill(Color::WHITE);
    }

    for stroke in strokes {
        if stroke.points.len() < 2 {
            continue;
        }

        // Map scene→pixel. y already increases downward in both scene and image space.
        let map_x = |x: f32| (x - origin.0) * opts.scale;
        let map_y = |y: f32| (y - origin.1) * opts.scale;

        let mut pb = PathBuilder::new();
        let first = &stroke.points[0];
        pb.move_to(map_x(first.x), map_y(first.y));
        for pt in &stroke.points[1..] {
            pb.line_to(map_x(pt.x), map_y(pt.y));
        }
        let path = match pb.finish() {
            Some(p) => p,
            None => continue,
        };

        let is_hl = stroke.is_highlighter();

        // Alpha: translucent for highlighter so overlapped text shows through.
        let alpha: u8 = if is_hl { 110 } else { 255 };

        let (r, g, b) = pen_rgb(stroke.color);
        let mut paint = Paint::default();
        paint.set_color_rgba8(r, g, b, alpha);
        paint.anti_alias = true;

        // Stroke width: max of sampled widths, else per-tool default.
        let max_sampled = stroke
            .points
            .iter()
            .filter_map(|p| p.width)
            .fold(0.0f32, f32::max);
        let base_width = if max_sampled > 0.0 {
            max_sampled
        } else if is_hl {
            18.0
        } else {
            3.0
        };
        let width = (base_width * opts.scale).max(1.0);

        let sk_stroke = SkStroke {
            width,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Default::default()
        };

        pixmap.stroke_path(&path, &paint, &sk_stroke, Transform::identity(), None);
    }

    pixmap.encode_png().map_err(|e| anyhow!("png encode: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmfiles::{Pen, PenColor, Point};

    /// Build a deterministic set of two strokes: one highlighter (yellow), one pen (black).
    fn make_test_strokes() -> Vec<Stroke> {
        vec![
            Stroke {
                tool: Pen::Highlighter1,
                color: PenColor::Yellow,
                points: vec![
                    Point {
                        x: 10.0,
                        y: 20.0,
                        speed: None,
                        direction: None,
                        width: Some(18.0),
                        pressure: None,
                    },
                    Point {
                        x: 50.0,
                        y: 20.0,
                        speed: None,
                        direction: None,
                        width: Some(18.0),
                        pressure: None,
                    },
                    Point {
                        x: 90.0,
                        y: 20.0,
                        speed: None,
                        direction: None,
                        width: Some(18.0),
                        pressure: None,
                    },
                ],
            },
            Stroke {
                tool: Pen::Ballpoint1,
                color: PenColor::Black,
                points: vec![
                    Point {
                        x: 10.0,
                        y: 10.0,
                        speed: None,
                        direction: None,
                        width: Some(3.0),
                        pressure: None,
                    },
                    Point {
                        x: 50.0,
                        y: 50.0,
                        speed: None,
                        direction: None,
                        width: Some(3.0),
                        pressure: None,
                    },
                    Point {
                        x: 90.0,
                        y: 30.0,
                        speed: None,
                        direction: None,
                        width: Some(3.0),
                        pressure: None,
                    },
                ],
            },
        ]
    }

    #[test]
    fn ink_bbox_correct() {
        let strokes = make_test_strokes();
        let refs: Vec<&Stroke> = strokes.iter().collect();
        let bbox = ink_bbox(&refs).expect("bbox should exist");
        // min_x=10, min_y=10, max_x=90, max_y=50
        assert!((bbox.0 - 10.0).abs() < 1e-4, "min_x={}", bbox.0);
        assert!((bbox.1 - 10.0).abs() < 1e-4, "min_y={}", bbox.1);
        assert!((bbox.2 - 90.0).abs() < 1e-4, "max_x={}", bbox.2);
        assert!((bbox.3 - 50.0).abs() < 1e-4, "max_y={}", bbox.3);
    }

    #[test]
    fn ink_bbox_empty() {
        assert!(ink_bbox(&[]).is_none());
    }

    #[test]
    fn render_strokes_correct_dims() {
        let strokes = make_test_strokes();
        let refs: Vec<&Stroke> = strokes.iter().collect();
        let opts = InkOpts {
            scale: 1.0,
            margin_px: 16,
            ..Default::default()
        };

        let png = render_strokes(&refs, &opts).expect("render should succeed");
        assert!(!png.is_empty(), "PNG bytes should be non-empty");

        let img = image::load_from_memory(&png).expect("should decode as image");
        let (w, h) = (img.width(), img.height());

        // bbox is (10,10)-(90,50): width=80, height=40; + 2*16 margin each = 112 x 72
        assert_eq!(w, 112, "expected width 112, got {w}");
        assert_eq!(h, 72, "expected height 72, got {h}");

        // Verify that the bbox region has non-transparent pixels (RGBA).
        let rgba = img.to_rgba8();
        let has_ink = rgba.pixels().any(|p| p.0[3] > 0);
        assert!(has_ink, "rendered image has no ink pixels");
    }

    /// Write or compare to the golden image.
    fn assert_golden(name: &str, png_bytes: &[u8]) {
        let golden_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/goldens")
            .join(format!("{name}.png"));

        if std::env::var("RMDIGEST_UPDATE_GOLDENS").is_ok() {
            std::fs::create_dir_all(golden_path.parent().unwrap()).expect("create golden dirs");
            std::fs::write(&golden_path, png_bytes).expect("write golden");
            println!("Updated golden: {}", golden_path.display());
            return;
        }

        let golden_bytes = std::fs::read(&golden_path).unwrap_or_else(|_| {
            panic!(
                "golden missing: {}. Run with RMDIGEST_UPDATE_GOLDENS=1 to create.",
                golden_path.display()
            )
        });

        let actual = image::load_from_memory(png_bytes)
            .expect("decode actual")
            .to_rgba8();
        let expected = image::load_from_memory(&golden_bytes)
            .expect("decode golden")
            .to_rgba8();

        assert_eq!(
            (actual.width(), actual.height()),
            (expected.width(), expected.height()),
            "golden dimensions mismatch"
        );

        // Mean per-channel absolute difference < 2/255.
        let total_samples = (actual.width() * actual.height() * 4) as f64;
        let sum_diff: f64 = actual
            .pixels()
            .zip(expected.pixels())
            .map(|(a, e)| {
                a.0.iter()
                    .zip(e.0.iter())
                    .map(|(&ac, &ec)| (ac as i32 - ec as i32).unsigned_abs() as f64)
                    .sum::<f64>()
            })
            .sum();
        let mean_diff = sum_diff / total_samples;
        assert!(
            mean_diff < 2.0,
            "golden mean diff {mean_diff:.4} >= 2.0/255 threshold"
        );
    }

    #[test]
    fn golden_ink_note() {
        let strokes = make_test_strokes();
        let refs: Vec<&Stroke> = strokes.iter().collect();
        let opts = InkOpts {
            background: Background::Transparent,
            scale: 2.0,
            margin_px: 8,
        };
        let png = render_strokes(&refs, &opts).expect("render should succeed");
        assert_golden("ink_note", &png);
    }
}
