//! Real-ink gesture fixtures: unit-box normalized strokes plus the transplant
//! math that maps them into a target region. Device-agnostic and hardware-free.

use inkapp_core::geometry::{PdfPoint, PdfRect};
use inkapp_core::ink::Stroke;
use serde::{Deserialize, Serialize};

/// How a fixture maps into a target region rect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fit {
    /// Preserve `native_aspect`, fit inside the target, center.
    AspectFit,
    /// Fill the target on both axes (ignores shape).
    Stretch,
    /// Fill width; height = target_w / native_aspect; center vertically.
    StretchX,
}

/// The drawing tool a fixture was recorded with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tool {
    Pen,
    Highlighter,
}

impl Tool {
    /// Whether this tool maps to a highlighter stroke.
    pub fn is_highlighter(self) -> bool {
        matches!(self, Tool::Highlighter)
    }
}

/// One stroke in unit-box coordinates (`[0,1]^2`, PDF y-up).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitStroke {
    pub points: Vec<[f64; 2]>,
}

/// One recorded sample of a gesture: its native aspect plus unit-box strokes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub native_aspect: f64,
    pub strokes: Vec<UnitStroke>,
}

/// Provenance of a fixture's samples.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub recording: String,
    pub device: String,
    /// ISO 8601 date the gesture was recorded.
    pub recorded: String,
}

/// A gesture fixture: catalog identity plus its banked samples.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GestureFixture {
    pub name: String,
    pub tool: Tool,
    pub fit: Fit,
    pub default: usize,
    pub samples: Vec<Sample>,
    pub source: Source,
}

impl GestureFixture {
    /// Load a fixture from JSON bytes.
    pub fn from_json(bytes: &[u8]) -> serde_json::Result<GestureFixture> {
        serde_json::from_slice(bytes)
    }

    /// Serialize to pretty JSON (field order follows struct declaration order, the serde_json default).
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    /// Transplant the default sample into `target` using this fixture's fit/tool.
    pub fn transplant_default(&self, target: PdfRect) -> Vec<Stroke> {
        let sample = self.samples.get(self.default).unwrap_or_else(|| {
            panic!(
                "fixture '{}': default index {} out of range ({} samples)",
                self.name,
                self.default,
                self.samples.len()
            )
        });
        transplant(sample, target, self.fit, self.tool.is_highlighter())
    }
}

/// Normalize PDF-space strokes to a single unit-box [`Sample`] over their
/// combined bounding box. `native_aspect = bbox_w / bbox_h`. Degenerate spans
/// (zero width or height, e.g. a tap) are guarded to aspect 1.0.
pub fn normalize(strokes: &[Stroke]) -> Sample {
    let mut x0 = f64::INFINITY;
    let mut y0 = f64::INFINITY;
    let mut x1 = f64::NEG_INFINITY;
    let mut y1 = f64::NEG_INFINITY;
    for s in strokes {
        for p in &s.points {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
    }
    let w = (x1 - x0).max(f64::EPSILON);
    let h = (y1 - y0).max(f64::EPSILON);
    let native_aspect = if (x1 - x0) <= f64::EPSILON || (y1 - y0) <= f64::EPSILON {
        1.0
    } else {
        w / h
    };
    let out = strokes
        .iter()
        .map(|s| UnitStroke {
            points: s
                .points
                .iter()
                .map(|p| [(p.x - x0) / w, (p.y - y0) / h])
                .collect(),
        })
        .collect();
    Sample {
        native_aspect,
        strokes: out,
    }
}

/// Transplant a unit-box sample into `target` per `fit`. `highlighter` sets the
/// tool flag on every produced stroke.
pub fn transplant(sample: &Sample, target: PdfRect, fit: Fit, highlighter: bool) -> Vec<Stroke> {
    let tw = target.x1 - target.x0;
    let th = target.y1 - target.y0;
    let a = sample.native_aspect.max(f64::EPSILON);

    let (ox, oy, w, h) = match fit {
        Fit::Stretch => (target.x0, target.y0, tw, th),
        Fit::StretchX => {
            let h = tw / a;
            (target.x0, target.y0 + (th - h) / 2.0, tw, h)
        }
        Fit::AspectFit => {
            let w = tw.min(a * th);
            let h = w / a;
            (target.x0 + (tw - w) / 2.0, target.y0 + (th - h) / 2.0, w, h)
        }
    };

    sample
        .strokes
        .iter()
        .map(|us| Stroke {
            points: us
                .points
                .iter()
                .map(|[u, v]| PdfPoint {
                    x: ox + u * w,
                    y: oy + v * h,
                })
                .collect(),
            highlighter,
        })
        .collect()
}
