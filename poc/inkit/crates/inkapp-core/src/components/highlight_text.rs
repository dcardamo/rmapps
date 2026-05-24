use crate::components::esc_typst_str;
use crate::ink::RegionInk;
use crate::manifest::Manifest;
use crate::widget::{RenderCx, Widget};

/// A run of words, each individually highlightable. Each token is wrapped so its
/// laid-out rect is recovered as a region named `tok-<i>`. Tokens listed in
/// `highlights` render pre-marked (so a prior highlight shows on re-render).
pub struct HighlightableText {
    tokens: Vec<String>,
    highlights: Vec<String>,
}

impl HighlightableText {
    pub fn new(tokens: &[&str]) -> Self {
        Self {
            tokens: tokens.iter().map(|t| t.to_string()).collect(),
            highlights: Vec::new(),
        }
    }

    /// Like `new`, but `highlighted` tokens render pre-marked. Matching is by
    /// token *string*: every token whose text appears in `highlighted` is marked,
    /// so duplicate token strings are all marked together (acceptable for prose;
    /// `read` still distinguishes them by per-token region).
    pub fn with_highlights(tokens: &[&str], highlighted: &[String]) -> Self {
        Self {
            tokens: tokens.iter().map(|t| t.to_string()).collect(),
            highlights: highlighted.to_vec(),
        }
    }
}

impl Widget for HighlightableText {
    /// The set of highlighted token strings.
    type Output = Vec<String>;

    fn render(&self, _cx: &mut RenderCx) -> String {
        // Each token is laid inline inside a #box. A #context block captures the
        // token's own laid-out position via here().position() and its measured
        // size via measure(), then emits <region>-labelled metadata so
        // recover_regions can read back the per-token rect.
        //
        // Key constraints:
        // - The <region> label must attach to the metadata element itself
        //   (recover_regions downcasts every <region> hit to MetadataElem).
        // - here().position() gives 1-based page + x/y lengths; we convert to
        //   0-based page and divide lengths by 1pt to get unitless floats.
        // - measure() inside #context returns a dict with .width and .height.
        // - Tokens are wrapped in #box[...] so they flow inline left-to-right.
        // - The page index comes from Typst introspection (here().position().page),
        //   not from the RenderCx page hint, which is why _cx is unused here.
        //
        // `t` is used for both measuring (measure(t)) and inline display (#t or
        // #highlight[#t]), so the #let binding is kept regardless of highlight state.
        let mut s = String::new();
        for (i, tok) in self.tokens.iter().enumerate() {
            // Escape for a Typst string literal (shared helper): only `\` and `"`
            // need escaping; other markup chars (], [, #) are literal inside a
            // string. The string is bound to `t` and used for both measuring and
            // inline display, so arbitrary token text is safe.
            let esc = esc_typst_str(tok);
            // Tokens already in `highlights` render wrapped in #highlight so they
            // show as pre-marked on re-render. The `new` path (empty highlights)
            // always uses the plain `#t` branch, keeping output byte-identical
            // to the previous implementation so harness goldens are unaffected.
            let disp = if self.highlights.iter().any(|h| h == tok) {
                "#highlight[#t]"
            } else {
                "#t"
            };
            s.push_str(&format!(
                "#box[#let t = \"{esc}\"; #context [#metadata((name: \"tok-{i}\", \
                   page: here().position().page - 1, x: here().position().x / 1pt, \
                   y: here().position().y / 1pt, w: measure(t).width / 1pt, \
                   h: measure(t).height / 1pt)) <region>]{disp}] "
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
