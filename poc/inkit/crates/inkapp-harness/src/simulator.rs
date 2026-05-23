use inkapp_core::device::Device;
use inkapp_core::error::Result;
use inkapp_core::geometry::PdfPoint;
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::Manifest;
use inkapp_core::readback::attribute;
use inkapp_core::render::compile_to_document;

use crate::inspector::inspect;

/// A synthesized user gesture targeted at a region.
#[derive(Debug, Clone)]
pub enum Gesture {
    /// A single dot in the center of the region (pen).
    Tap,
    /// A horizontal highlighter swipe across the full region width.
    Swipe,
}

/// A script of gestures, each bound to a region name.
#[derive(Debug, Default)]
pub struct Scenario {
    steps: Vec<(String, Gesture)>,
}

impl Scenario {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a gesture targeting `region`.
    pub fn mark(mut self, region: &str, g: Gesture) -> Self {
        self.steps.push((region.to_string(), g));
        self
    }
}

/// The result of one simulated cycle.
pub struct StepTrace {
    /// All synthesized strokes (PDF space).
    pub strokes: Vec<Stroke>,
    /// Strokes attributed to regions.
    pub readback: Vec<RegionInk>,
    /// The composited inspector image (PNG bytes).
    pub inspector_png: Vec<u8>,
}

/// Synthesize strokes for a scenario against a manifest's regions.
fn synthesize(manifest: &Manifest, scenario: &Scenario) -> Vec<Stroke> {
    let mut strokes = Vec::new();
    for (region_name, gesture) in &scenario.steps {
        let Some(region) = manifest.regions.iter().find(|r| &r.name == region_name) else {
            continue;
        };
        let r = &region.rect;
        let cx = (r.x0 + r.x1) / 2.0;
        let cy = (r.y0 + r.y1) / 2.0;
        match gesture {
            Gesture::Tap => strokes.push(Stroke {
                points: vec![PdfPoint { x: cx, y: cy }],
                highlighter: false,
            }),
            Gesture::Swipe => strokes.push(Stroke {
                points: vec![PdfPoint { x: r.x0, y: cy }, PdfPoint { x: r.x1, y: cy }],
                highlighter: true,
            }),
        }
    }
    strokes
}

/// Run one loop cycle entirely in software, through the real writer->parse path.
pub fn simulate(
    render_src: &str,
    manifest: &Manifest,
    device: &dyn Device,
    scenario: &Scenario,
) -> Result<StepTrace> {
    let doc = compile_to_document(render_src)?;
    let page_h_pt = doc
        .pages
        .first()
        .map(|p| p.frame.height().to_pt())
        .unwrap_or(0.0);

    // Synthesize the user's ink (PDF space), then round-trip it through the
    // device's real .rm write+read so the test exercises the byte path.
    let synthesized = synthesize(manifest, scenario);
    let bytes = device.write_ink(&synthesized, page_h_pt)?;
    let strokes = device.read_ink(&bytes, page_h_pt)?;

    let readback = attribute(&strokes, manifest);
    let inspector_png = inspect(&doc, manifest, &strokes)?;

    Ok(StepTrace {
        strokes,
        readback,
        inspector_png,
    })
}
