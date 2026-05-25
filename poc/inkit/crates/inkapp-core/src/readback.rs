use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::ink::{RegionInk, Stroke};
use crate::manifest::Manifest;

/// Attribute per-page strokes to regions, then stitch each logical region's ink
/// across the pages it spans into one `RegionInk`. `pages[p]` holds page p's
/// strokes (that page's PDF space). A stroke on page p is tested ONLY against
/// regions with `region.page == p`, so same-rect regions on different pages never
/// cross-attribute. A stroke matches a region if any of its points lies in the
/// region's rect; a stroke may match (and be added to) multiple regions on its
/// page. Output order is the first-seen order of region names.
pub fn attribute(pages: &[Vec<Stroke>], manifest: &Manifest) -> Vec<RegionInk> {
    let mut order: Vec<String> = Vec::new();
    let mut by_name: HashMap<String, Vec<Stroke>> = HashMap::new();
    for region in &manifest.regions {
        let Some(strokes) = pages.get(region.page) else {
            continue;
        };
        for s in strokes {
            if s.points.iter().any(|p| region.rect.contains(p.x, p.y)) {
                if !by_name.contains_key(&region.name) {
                    order.push(region.name.clone());
                }
                by_name
                    .entry(region.name.clone())
                    .or_default()
                    .push(s.clone());
            }
        }
    }
    order
        .into_iter()
        .map(|name| {
            let strokes = by_name.remove(&name).unwrap_or_default();
            RegionInk {
                region: name,
                strokes,
            }
        })
        .collect()
}

/// Single-page convenience: attribute one page's strokes (the common case for
/// single-page tests and the harness `simulate`).
pub fn attribute_page(strokes: &[Stroke], manifest: &Manifest) -> Vec<RegionInk> {
    attribute(&[strokes.to_vec()], manifest)
}

/// Return strokes present in `current` that are not in `prev` (by value).
///
/// Comparison is exact-equality over `f64` point coordinates, so both slices
/// must originate from the same coordinate path (e.g. both PDF-space, or both
/// post-`read_ink`) — comparing pre- and post-device-round-trip strokes would
/// not de-duplicate due to f32 quantization in the device transform.
pub fn diff_new(prev: &[Stroke], current: &[Stroke]) -> Vec<Stroke> {
    current
        .iter()
        .filter(|s| !prev.contains(s))
        .cloned()
        .collect()
}

/// Reject ink whose source version doesn't match the current manifest version.
pub fn guard_version(ink_version: u64, manifest: &Manifest) -> Result<()> {
    if ink_version == manifest.version {
        Ok(())
    } else {
        Err(Error::Readback(format!(
            "stale ink: written against version {ink_version}, manifest is {}",
            manifest.version
        )))
    }
}
