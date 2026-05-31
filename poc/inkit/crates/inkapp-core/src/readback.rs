use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::ink::{RegionInk, Stroke};
use crate::manifest::Manifest;

/// Attribute per-page strokes to regions, then stitch each logical region's ink
/// across the pages it spans into one `RegionInk`. `pages[p]` holds page p's
/// strokes (that page's PDF space). A stroke on page p is tested ONLY against
/// regions with `region.page == p`, so same-rect regions on different pages never
/// cross-attribute. A stroke matches a region if its polyline **passes through**
/// the region's rect — either a stored point lies inside, or a segment between
/// consecutive points crosses the rect. A stroke may match (and be added to)
/// multiple regions on its page. Output order is the first-seen order of
/// region names.
///
/// The segment-crossing check is load-bearing for sparse strokes. The
/// reMarkable cloud sometimes stores a freehand highlight as just two points
/// (start + end); a point-in-rect-only check catches only the two tokens at
/// the endpoints and misses every token in between — a user's highlight of
/// "has announced Chimera: a nine-driver flagship IEM" would silently decode
/// as just two words ("has", "IEM"). Sampling along each segment at ~1pt
/// resolution closes that gap without changing behavior for dense strokes
/// (snap-to-text already arrives with 17 sample points per rect).
pub fn attribute(pages: &[Vec<Stroke>], manifest: &Manifest) -> Vec<RegionInk> {
    let mut order: Vec<String> = Vec::new();
    let mut by_name: HashMap<String, Vec<Stroke>> = HashMap::new();
    for region in &manifest.regions {
        let Some(strokes) = pages.get(region.page) else {
            continue;
        };
        for s in strokes {
            if stroke_hits_rect(s, &region.rect) {
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

/// Whether `stroke`'s polyline passes through `rect`. True if any stored point
/// is inside, or any segment between consecutive points crosses the rect
/// (sampled at ~1pt along the segment — coarser than necessary for sub-pt
/// accuracy but ample for token-sized boxes).
fn stroke_hits_rect(stroke: &Stroke, rect: &crate::geometry::PdfRect) -> bool {
    if stroke.points.iter().any(|p| rect.contains(p.x, p.y)) {
        return true;
    }
    for w in stroke.points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let len = (dx * dx + dy * dy).sqrt();
        // 1pt-spaced samples between the endpoints; min one interior sample so
        // a zero-length segment doesn't loop, but also doesn't silently skip.
        let n = (len.ceil() as usize).max(2);
        for i in 1..n {
            let t = i as f64 / n as f64;
            let x = a.x + dx * t;
            let y = a.y + dy * t;
            if rect.contains(x, y) {
                return true;
            }
        }
    }
    false
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
