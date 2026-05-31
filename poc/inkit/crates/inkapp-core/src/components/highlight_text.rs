use crate::component::RenderCx;
use crate::components::{esc_typst_str, highlighted_token_indices, token_region};
use crate::ink::RegionInk;
use crate::manifest::Manifest;

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

impl HighlightableText {
    /// Emit per-token Typst, each token wrapped so its laid-out rect recovers as
    /// a region named `tok-<i>` (so a highlight maps back to a specific span).
    pub fn render(&self, _cx: &mut RenderCx) -> String {
        let mut s = String::new();
        for (i, tok) in self.tokens.iter().enumerate() {
            let esc = esc_typst_str(tok);
            let highlighted = self.highlights.iter().any(|h| h == tok);
            s.push_str(&token_region(i, &format!("\"{esc}\""), highlighted));
        }
        s.push('\n');
        s
    }

    /// The set of highlighted token strings (tokens whose region was overlapped
    /// by a highlighter stroke).
    pub fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<String> {
        highlighted_token_indices(self.tokens.len(), ink, manifest)
            .into_iter()
            .map(|i| self.tokens[i].clone())
            .collect()
    }
}
