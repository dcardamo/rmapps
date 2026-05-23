//! reMarkable implementation of the inkapp `Device` seam.
//!
//! The PDF<->scene transform here is a *self-consistent model* used symmetrically
//! by `write_ink`/`read_ink`. Fidelity to a real device is validated separately
//! by Spec 3 (gesture fixtures + on-device acceptance); the deterministic harness
//! only relies on `write_ink`/`read_ink` being mutual inverses.

use inkapp_core::device::Device;
use inkapp_core::error::{Error, Result};
use inkapp_core::geometry::{DevicePoint, PdfPoint};
use inkapp_core::ink::Stroke;
use rm_files::{Pen, PenColor, Point, Scene, SceneItem};

/// Default reMarkable Paper Pro canvas width/height in pixels.
const CANVAS_W: f64 = 1404.0;
const CANVAS_H: f64 = 1872.0;

/// A reMarkable device with a fit-to-width coordinate model.
pub struct Remarkable {
    canvas_w: f64,
    canvas_h: f64,
}

impl Remarkable {
    pub fn new() -> Self {
        Self {
            canvas_w: CANVAS_W,
            canvas_h: CANVAS_H,
        }
    }

    /// Pixels-per-point: the page is fit to the canvas width. Scene space shares
    /// this scale on both axes; x is centered on the page, y runs from the top.
    fn scale(&self, page_w_pt: f64) -> f64 {
        self.canvas_w / page_w_pt
    }

    /// Page width in points implied by a page height, assuming the canvas aspect.
    /// The harness passes page height; width is derived so the model is fully
    /// determined by (page_h, canvas aspect).
    fn page_w_pt(&self, page_h_pt: f64) -> f64 {
        page_h_pt * (self.canvas_w / self.canvas_h)
    }
}

impl Default for Remarkable {
    fn default() -> Self {
        Remarkable::new()
    }
}

impl Device for Remarkable {
    fn pdf_to_device(&self, p: PdfPoint, page_h_pt: f64) -> DevicePoint {
        let page_w = self.page_w_pt(page_h_pt);
        let scale = self.scale(page_w);
        DevicePoint {
            x: (p.x - page_w / 2.0) * scale,
            y: (page_h_pt - p.y) * scale,
        }
    }

    fn device_to_pdf(&self, p: DevicePoint, page_h_pt: f64) -> PdfPoint {
        let page_w = self.page_w_pt(page_h_pt);
        let scale = self.scale(page_w);
        PdfPoint {
            x: p.x / scale + page_w / 2.0,
            y: page_h_pt - p.y / scale,
        }
    }

    fn read_ink(&self, bytes: &[u8], page_h_pt: f64) -> Result<Vec<Stroke>> {
        let scene = Scene::parse(bytes).map_err(|e| Error::Readback(format!("rm parse: {e}")))?;
        let mut out = Vec::new();
        for s in scene.strokes() {
            let points = s
                .points
                .iter()
                .map(|pt| {
                    self.device_to_pdf(
                        DevicePoint {
                            x: pt.x as f64,
                            y: pt.y as f64,
                        },
                        page_h_pt,
                    )
                })
                .collect();
            out.push(Stroke {
                points,
                highlighter: s.is_highlighter(),
            });
        }
        Ok(out)
    }

    fn write_ink(&self, strokes: &[Stroke], page_h_pt: f64) -> Result<Vec<u8>> {
        let items: Vec<SceneItem> = strokes
            .iter()
            .map(|s| {
                let points = s
                    .points
                    .iter()
                    .map(|p| {
                        let d = self.pdf_to_device(*p, page_h_pt);
                        Point {
                            x: d.x as f32,
                            y: d.y as f32,
                            speed: Some(0.0),
                            direction: Some(0.0),
                            width: Some(2.0),
                            pressure: Some(0.0),
                        }
                    })
                    .collect();
                let (tool, color) = if s.highlighter {
                    (Pen::Highlighter2, PenColor::Highlight)
                } else {
                    (Pen::Fineliner1, PenColor::Black)
                };
                SceneItem::Line(rm_files::Stroke {
                    tool,
                    color,
                    points,
                })
            })
            .collect();
        Ok(rm_files::write_scene(6, &items))
    }
}
