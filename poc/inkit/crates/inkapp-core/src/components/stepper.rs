use crate::component::Component;
use crate::component::RenderCx;
use crate::ink::RegionInk;
use crate::manifest::Manifest;
use crate::render::region_metadata;

/// A counter whose state lives ONLY in the document (no connector). It renders
/// its current count and an increment region; on readback it adds the number of
/// increment strokes to the **carried base** (the count it was rendered with),
/// not to its own current prop — proving decode interprets ink against the base
/// the document was rendered against.
pub struct Stepper {
    name: String,
    count: u64,
}

impl Stepper {
    pub fn new(name: &str, count: u64) -> Self {
        Self {
            name: name.to_string(),
            count,
        }
    }

    fn region_name(&self) -> String {
        format!("stepper:{}", self.name)
    }

    /// The base this document was rendered with (0 if none carried).
    fn carried_base(&self, manifest: &Manifest) -> u64 {
        manifest
            .state
            .components
            .get(&self.region_name())
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    }

    /// Count strokes attributed to this stepper's region with a point inside it.
    fn increments(&self, ink: &[RegionInk], manifest: &Manifest) -> u64 {
        let name = self.region_name();
        let Some(region) = manifest.regions.iter().find(|r| r.name == name) else {
            return 0;
        };
        ink.iter()
            .filter(|ri| ri.region == name)
            .flat_map(|ri| &ri.strokes)
            .filter(|s| s.points.iter().any(|p| region.rect.contains(p.x, p.y)))
            .count() as u64
    }

    /// The new count: the carried base plus the increment strokes (idle = base).
    pub fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> u64 {
        self.carried_base(manifest) + self.increments(ink, manifest)
    }
}

impl Component for Stepper {
    type Msg = u64;

    fn render(&self, cx: &mut RenderCx) -> String {
        let name = self.region_name();
        let (x, y, w, h) = (20.0_f64, 40.0_f64, 16.0_f64, 16.0_f64);
        let mut s = region_metadata(&name, cx.page, x, y, w, h);
        s.push_str(&format!(
            "#place(top + left, dx: {x}pt, dy: {y}pt, rect(width: {w}pt, height: {h}pt, stroke: 0.5pt)[#align(center + horizon)[+]])\n"
        ));
        s.push_str(&format!("#text[{}]\n", self.count));
        s
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<u64> {
        let increments = self.increments(ink, manifest);
        if increments > 0 {
            vec![self.carried_base(manifest) + increments]
        } else {
            vec![]
        }
    }

    fn state_key(&self) -> Option<String> {
        Some(self.region_name())
    }

    fn render_state(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!(self.count))
    }
}
