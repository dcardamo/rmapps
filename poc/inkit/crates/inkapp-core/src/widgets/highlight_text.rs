use crate::ink::RegionInk;
use crate::manifest::Manifest;
use crate::widget::{RenderCx, Widget};

/// A run of words, each individually highlightable. Each token is wrapped so its
/// laid-out rect is recovered as a region named `tok-<i>`.
pub struct HighlightableText {
    tokens: Vec<String>,
}

impl HighlightableText {
    pub fn new(tokens: &[&str]) -> Self {
        Self {
            tokens: tokens.iter().map(|t| t.to_string()).collect(),
        }
    }
}

impl Widget for HighlightableText {
    /// The set of highlighted token strings.
    type Output = Vec<String>;

    fn render(&self, _cx: &mut RenderCx) -> String {
        // Each token is laid inline inside a #box. A #context block captures the
        // token's own laid-out position via here().position() and its measured size
        // via measure(), then emits <region>-labelled metadata so recover_regions
        // can read back the per-token rect.
        //
        // Key constraints:
        // - The <region> label must attach to the metadata element itself
        //   (recover_regions downcasts every <region> hit to MetadataElem).
        // - here().position() gives 1-based page + x/y lengths; we convert to
        //   0-based page and divide lengths by 1pt to get unitless floats.
        // - measure() inside #context returns a dict with .width and .height.
        // - Tokens are wrapped in #box[...] so they flow inline left-to-right.
        let mut s = String::new();
        for (i, tok) in self.tokens.iter().enumerate() {
            s.push_str(&format!(
                "#box[#context [#metadata((name: \"tok-{i}\", page: here().position().page - 1, \
                   x: here().position().x / 1pt, y: here().position().y / 1pt, \
                   w: measure[\"{tok}\"].width / 1pt, h: measure[\"{tok}\"].height / 1pt)) <region>]{tok}] "
            ));
        }
        s.push('\n');
        s
    }

    fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<String> {
        let mut out = Vec::new();
        for (i, tok) in self.tokens.iter().enumerate() {
            let name = format!("tok-{i}");
            let Some(region) = manifest.regions.iter().find(|r| r.name == name) else {
                continue;
            };
            // Only highlighter strokes count; check if the stroke bbox overlaps
            // this token's region rect.
            let highlighted = ink
                .iter()
                .filter(|ri| ri.region == name)
                .flat_map(|ri| &ri.strokes)
                .filter(|s| s.highlighter)
                .any(|s| s.bbox().is_some_and(|b| region.rect.overlaps(&b)));
            if highlighted {
                out.push(tok.clone());
            }
        }
        out
    }
}
