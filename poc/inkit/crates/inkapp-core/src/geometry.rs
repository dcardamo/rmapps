use serde::{Deserialize, Serialize};

/// A rectangle in PDF user space (bottom-left origin, y up), in points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PdfRect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// Sub-point tolerance used for boundary tests against region rects.
/// Device write→read paths quantise coordinates to f32, introducing up to
/// ~1e-5 pt of error; this absorbs that without meaningfully widening the rect.
/// 1e-4 pt is well below a pixel at any screen resolution and well above the
/// maximum observed f32 round-trip error (~5e-6 pt).
const CONTAINS_EPSILON: f64 = 1e-4;

impl PdfRect {
    /// Whether a point (PDF space) lies within this rect (inclusive, with a
    /// small sub-point tolerance to absorb f32 quantisation from device round-trips).
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x0 - CONTAINS_EPSILON
            && x <= self.x1 + CONTAINS_EPSILON
            && y >= self.y0 - CONTAINS_EPSILON
            && y <= self.y1 + CONTAINS_EPSILON
    }

    /// Whether this rect overlaps `other` (with a small sub-point tolerance to
    /// absorb f32 quantisation from device round-trips, matching [`contains`]).
    ///
    /// Note: the tolerance effectively widens each rect by `CONTAINS_EPSILON` on
    /// every side, so two rects within `2·CONTAINS_EPSILON` of each other are
    /// treated as overlapping. This is correct for adjacent/edge-sharing regions,
    /// but do not rely on `overlaps` to resolve sub-`2ε` gaps.
    pub fn overlaps(&self, other: &PdfRect) -> bool {
        self.x0 <= other.x1 + CONTAINS_EPSILON
            && self.x1 >= other.x0 - CONTAINS_EPSILON
            && self.y0 <= other.y1 + CONTAINS_EPSILON
            && self.y1 >= other.y0 - CONTAINS_EPSILON
    }
}

/// A point in PDF user space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PdfPoint {
    pub x: f64,
    pub y: f64,
}

/// A point in a device's native ink space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DevicePoint {
    pub x: f64,
    pub y: f64,
}

/// A document's page geometry, in points. Drives Typst `#set page` and lets the
/// content column width be computed for full-width regions. A *device profile* in
/// inkapp is a `PageGeom` paired with a `Device`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGeom {
    pub w: f64,
    pub h: f64,
    pub margin: f64,
}

impl Default for PageGeom {
    /// The standard 3:4-ish e-ink profile (the former DOC_PAGE_W/H + 16pt margin).
    fn default() -> Self {
        Self {
            w: 420.0,
            h: 560.0,
            margin: 16.0,
        }
    }
}

impl PageGeom {
    /// The content column width (page width minus both margins).
    pub fn content_w(&self) -> f64 {
        self.w - 2.0 * self.margin
    }
}

/// The `[page]` config section. Converts to `PageGeom` (the runtime type).
#[derive(Debug, Clone, Copy, serde::Deserialize, inkapp_config::Config)]
#[serde(default)]
#[config(kind = "page", namespace = "framework")]
pub struct PageConfig {
    /// Page width in points.
    #[config(default = 420.0)]
    pub width: f64,
    /// Page height in points.
    #[config(default = 560.0)]
    pub height: f64,
    /// Uniform margin in points.
    #[config(default = 16.0)]
    pub margin: f64,
}

impl From<PageConfig> for PageGeom {
    fn from(c: PageConfig) -> Self {
        PageGeom {
            w: c.width,
            h: c.height,
            margin: c.margin,
        }
    }
}

/// The `[device]` config section — which device backend to deploy to, and the
/// polling cadence the `serve` loop uses between sync cycles. The per-app target
/// folder lives in each app's own config section, not here.
#[derive(Debug, Clone, serde::Deserialize, inkapp_config::Config)]
#[serde(default)]
#[config(kind = "device", namespace = "framework")]
pub struct DeviceConfig {
    /// Device backend identifier (e.g. "remarkable").
    #[config(default = String::from("remarkable"))]
    pub backend: String,
    /// Seconds between sync cycles when running `serve`. The `--interval` CLI
    /// flag, when provided, overrides this.
    #[config(default = 30u64)]
    pub sync_interval_secs: u64,
}

/// Convert a Typst top-left-origin rect to a PDF bottom-left-origin rect using
/// the height of the rect's own page.
pub fn typst_to_pdf_rect(x: f64, y: f64, w: f64, h: f64, page_height_pt: f64) -> PdfRect {
    PdfRect {
        x0: x,
        y0: page_height_pt - (y + h),
        x1: x + w,
        y1: page_height_pt - y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: PdfRect = PdfRect {
        x0: 10.0,
        y0: 10.0,
        x1: 30.0,
        y1: 30.0,
    };

    #[test]
    fn contains_interior_and_boundary_inclusive() {
        assert!(R.contains(20.0, 20.0), "interior point");
        assert!(R.contains(10.0, 10.0), "bottom-left corner is inclusive");
        assert!(R.contains(30.0, 30.0), "top-right corner is inclusive");
        assert!(R.contains(10.0, 20.0), "left edge is inclusive");
    }

    #[test]
    fn contains_rejects_outside() {
        assert!(!R.contains(9.999, 20.0), "just left of x0");
        assert!(!R.contains(30.001, 20.0), "just right of x1");
        assert!(!R.contains(20.0, 9.999), "just below y0");
        assert!(!R.contains(20.0, 30.001), "just above y1");
    }

    #[test]
    fn overlaps_intersecting_touching_and_disjoint() {
        // Clearly intersecting.
        assert!(R.overlaps(&PdfRect {
            x0: 20.0,
            y0: 20.0,
            x1: 40.0,
            y1: 40.0
        }));
        // Sharing only a corner counts as overlap (inclusive bounds).
        assert!(R.overlaps(&PdfRect {
            x0: 30.0,
            y0: 30.0,
            x1: 50.0,
            y1: 50.0
        }));
        // Fully separate on x.
        assert!(!R.overlaps(&PdfRect {
            x0: 31.0,
            y0: 10.0,
            x1: 40.0,
            y1: 30.0
        }));
        // Fully separate on y.
        assert!(!R.overlaps(&PdfRect {
            x0: 10.0,
            y0: 31.0,
            x1: 30.0,
            y1: 40.0
        }));
    }

    #[test]
    fn page_config_defaults_match_pagegeom_default() {
        let c = PageConfig::default();
        assert_eq!(PageGeom::from(c), PageGeom::default());
    }

    #[test]
    fn device_config_defaults_to_remarkable() {
        assert_eq!(DeviceConfig::default().backend, "remarkable");
    }

    #[test]
    fn device_config_registers() {
        use inkapp_config::{Namespace, Registry};
        assert!(Registry::find(Namespace::Framework, "device").is_some());
    }

    #[test]
    fn page_config_overrides_map_through() {
        let c = PageConfig {
            width: 300.0,
            height: 400.0,
            margin: 8.0,
        };
        let g = PageGeom::from(c);
        assert_eq!((g.w, g.h, g.margin), (300.0, 400.0, 8.0));
    }
}
