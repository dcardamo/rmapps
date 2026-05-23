use crate::error::{Error, Result};
use crate::ink::{RegionInk, Stroke};
use crate::manifest::Manifest;

/// Group strokes by the region that contains them. A stroke is attributed to a
/// region if any of its points falls inside that region's rect. A stroke may
/// match multiple regions; it is added to each. Strokes matching no region are
/// dropped. Output preserves manifest region order and only includes regions
/// that received at least one stroke.
pub fn attribute(strokes: &[Stroke], manifest: &Manifest) -> Vec<RegionInk> {
    let mut out: Vec<RegionInk> = Vec::new();
    for region in &manifest.regions {
        let mut matched = Vec::new();
        for s in strokes {
            if s.points.iter().any(|p| region.rect.contains(p.x, p.y)) {
                matched.push(s.clone());
            }
        }
        if !matched.is_empty() {
            out.push(RegionInk {
                region: region.name.clone(),
                strokes: matched,
            });
        }
    }
    out
}

/// Return strokes present in `current` that are not in `prev` (by value).
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
        Err(Error::Manifest(format!(
            "stale ink: written against version {ink_version}, manifest is {}",
            manifest.version
        )))
    }
}
