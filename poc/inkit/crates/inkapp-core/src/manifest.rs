use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use typst::foundations::{Label, Selector};
use typst::introspection::MetadataElem;
use typst::layout::PagedDocument;
use typst::utils::PicoStr;

use crate::error::{Error, Result};
use crate::geometry::{typst_to_pdf_rect, PdfRect};

/// The raw metadata a component emits next to a `<region>` label. Coordinates are
/// Typst-space (top-left origin), in points. `role` distinguishes an atomic region
/// (`"box"` or absent — carries `w`/`h`) from the bounds of a breakable region
/// (`"flow-start"` carries `w`; `"flow-end"` carries only its position).
#[derive(Debug, Clone, Deserialize)]
struct RawRegion {
    name: String,
    page: usize,
    x: f64,
    y: f64,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    w: Option<f64>,
    #[serde(default)]
    h: Option<f64>,
}

/// A labelled rectangle on a page, in PDF-point coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub name: String,
    pub page: usize,
    pub rect: PdfRect,
}

/// App-defined state carried inside the (sealed) manifest. The framework only
/// encrypts and carries it; the app/component owns the contents.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DocState {
    /// Document-level, app-owned. Set by the app in `view`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<serde_json::Value>,
    /// Component-level, keyed by each component's `state_key()`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub components: BTreeMap<String, serde_json::Value>,
}

/// The document's self-describing layout: regions plus a version marker.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u64,
    pub regions: Vec<Region>,
    #[serde(default)]
    pub state: DocState,
}

/// One end of a breakable region's vertical extent, in Typst space (top-left
/// origin), points. `split_rects` uses the start bound's `x`/`w` for every frame
/// (a flowing full-column region has constant left edge and width); the end
/// bound's `x`/`w` are unused (only its `page`/`y` bound the last frame).
#[derive(Debug, Clone, Copy)]
struct FlowBound {
    page: usize,
    x: f64,
    y: f64,
    w: f64,
}

/// Reconstruct the per-frame PDF rects of a breakable region from its start/end
/// bounds. Emits one `Region` per page in `start.page..=end.page`: the start page
/// runs from `start.y` to its bottom, interior pages are full height, the end page
/// runs from the top to `end.y`. Each rect is flipped to PDF space with its own
/// page height. `start.page == end.page` degenerates to a single `start.y..end.y`
/// rect (the no-break case). Degenerate zero-height frames (a bound landing exactly
/// on a page edge) are skipped so they don't clutter the manifest.
fn split_rects(
    name: &str,
    start: FlowBound,
    end: FlowBound,
    page_heights: &[f64],
) -> Result<Vec<Region>> {
    let mut out = Vec::new();
    for p in start.page..=end.page {
        let page_h = *page_heights.get(p).ok_or_else(|| {
            Error::Region(format!("flow region '{name}' references missing page {p}"))
        })?;
        let top = if p == start.page { start.y } else { 0.0 };
        let bottom = if p == end.page { end.y } else { page_h };
        if bottom > top {
            out.push(Region {
                name: name.to_string(),
                page: p,
                rect: typst_to_pdf_rect(start.x, top, start.w, bottom - top, page_h),
            });
        }
    }
    Ok(out)
}

/// Recover all `<region>`-labelled metadata from a compiled document and convert
/// each rect to PDF coordinates using its own page's height. `version` defaults
/// to 0; callers can set it via [`Manifest::with_version`].
pub fn recover_regions(doc: &PagedDocument) -> Result<Manifest> {
    let page_heights: Vec<f64> = doc.pages.iter().map(|p| p.frame.height().to_pt()).collect();

    let label = Label::new(PicoStr::intern("region"))
        .ok_or_else(|| Error::Region("empty region label".into()))?;
    let elems = doc.introspector.query(&Selector::Label(label));

    // Parse every <region> metadata row first.
    let mut raws: Vec<RawRegion> = Vec::with_capacity(elems.len());
    for elem in &elems {
        let packed = elem
            .to_packed::<MetadataElem>()
            .ok_or_else(|| Error::Region("labelled element is not metadata".into()))?;
        let json = serde_json::to_value(&packed.value).map_err(|e| Error::Region(e.to_string()))?;
        raws.push(serde_json::from_value(json).map_err(|e| Error::Region(e.to_string()))?);
    }

    // Atomic rows convert directly (preserving query order); flow rows pair by name.
    let mut regions: Vec<Region> = Vec::new();
    let mut flow_starts: Vec<&RawRegion> = Vec::new();
    let mut flow_ends: BTreeMap<String, &RawRegion> = BTreeMap::new();

    for raw in &raws {
        match raw.role.as_deref() {
            Some("flow-start") => flow_starts.push(raw),
            Some("flow-end") => {
                flow_ends.insert(raw.name.clone(), raw);
            }
            _ => {
                let w = raw.w.ok_or_else(|| {
                    Error::Region(format!("atomic region '{}' missing w", raw.name))
                })?;
                let h = raw.h.ok_or_else(|| {
                    Error::Region(format!("atomic region '{}' missing h", raw.name))
                })?;
                let page_h = *page_heights.get(raw.page).ok_or_else(|| {
                    Error::Region(format!(
                        "region '{}' references missing page {}",
                        raw.name, raw.page
                    ))
                })?;
                regions.push(Region {
                    name: raw.name.clone(),
                    page: raw.page,
                    rect: typst_to_pdf_rect(raw.x, raw.y, w, h, page_h),
                });
            }
        }
    }

    // Symmetric to the missing-end check: a flow-end with no matching flow-start would
    // otherwise silently drop the region.
    for name in flow_ends.keys() {
        if !flow_starts.iter().any(|s| &s.name == name) {
            return Err(Error::Region(format!(
                "flow region '{name}' has an end marker but no start marker"
            )));
        }
    }

    for start in flow_starts {
        let end = flow_ends.get(&start.name).ok_or_else(|| {
            Error::Region(format!("flow region '{}' has no end marker", start.name))
        })?;
        let w = start.w.ok_or_else(|| {
            Error::Region(format!("flow region '{}' start missing w", start.name))
        })?;
        regions.extend(split_rects(
            &start.name,
            FlowBound {
                page: start.page,
                x: start.x,
                y: start.y,
                w,
            },
            FlowBound {
                page: end.page,
                x: end.x,
                y: end.y,
                w: 0.0,
            },
            &page_heights,
        )?);
    }

    Ok(Manifest {
        version: 0,
        regions,
        state: DocState::default(),
    })
}

impl Manifest {
    /// Set the version marker (builder style).
    pub fn with_version(mut self, version: u64) -> Self {
        self.version = version;
        self
    }
}

#[cfg(test)]
mod split_tests {
    use super::*;

    #[test]
    fn single_page_is_one_rect() {
        let rs = split_rects(
            "p",
            FlowBound {
                page: 0,
                x: 10.0,
                y: 100.0,
                w: 50.0,
            },
            FlowBound {
                page: 0,
                x: 10.0,
                y: 140.0,
                w: 0.0,
            },
            &[560.0],
        )
        .unwrap();
        assert_eq!(rs.len(), 1);
        assert_eq!(rs[0].page, 0);
        assert_eq!(
            rs[0].rect,
            typst_to_pdf_rect(10.0, 100.0, 50.0, 40.0, 560.0)
        );
    }

    #[test]
    fn two_pages_split_at_break() {
        let rs = split_rects(
            "p",
            FlowBound {
                page: 0,
                x: 10.0,
                y: 500.0,
                w: 50.0,
            },
            FlowBound {
                page: 1,
                x: 10.0,
                y: 30.0,
                w: 0.0,
            },
            &[560.0, 560.0],
        )
        .unwrap();
        assert_eq!(rs.len(), 2);
        assert_eq!(
            rs[0],
            Region {
                name: "p".into(),
                page: 0,
                rect: typst_to_pdf_rect(10.0, 500.0, 50.0, 60.0, 560.0)
            }
        );
        assert_eq!(
            rs[1],
            Region {
                name: "p".into(),
                page: 1,
                rect: typst_to_pdf_rect(10.0, 0.0, 50.0, 30.0, 560.0)
            }
        );
    }

    #[test]
    fn three_pages_middle_is_full_height() {
        let rs = split_rects(
            "p",
            FlowBound {
                page: 0,
                x: 10.0,
                y: 500.0,
                w: 50.0,
            },
            FlowBound {
                page: 2,
                x: 10.0,
                y: 20.0,
                w: 0.0,
            },
            &[560.0, 560.0, 560.0],
        )
        .unwrap();
        assert_eq!(rs.len(), 3);
        assert_eq!(
            rs[0].rect,
            typst_to_pdf_rect(10.0, 500.0, 50.0, 60.0, 560.0)
        ); // start page: 500..560
        assert_eq!(rs[1].rect, typst_to_pdf_rect(10.0, 0.0, 50.0, 560.0, 560.0));
        assert_eq!(rs[2].rect, typst_to_pdf_rect(10.0, 0.0, 50.0, 20.0, 560.0));
    }

    #[test]
    fn missing_page_errors() {
        assert!(split_rects(
            "p",
            FlowBound {
                page: 0,
                x: 0.0,
                y: 0.0,
                w: 1.0
            },
            FlowBound {
                page: 5,
                x: 0.0,
                y: 0.0,
                w: 0.0
            },
            &[560.0],
        )
        .is_err());
    }
}
