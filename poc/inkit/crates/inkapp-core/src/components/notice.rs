//! `Notice` — a Display-mode component: it renders one or more lines of notice
//! text and decodes nothing. It's the first Display component (alongside the
//! Capture `HighlightableText` and Control `Checkbox`), and the reusable way an
//! app surfaces things like connector write failures (`failed_writes()`): the
//! app reads the content in `view` and composes a `Notice` — the component never
//! touches connectors. Generic over the app's `Msg` (which it never emits) so it
//! drops into any `view` flow.

use std::marker::PhantomData;

use crate::component::Component;
use crate::component::RenderCx;
use crate::components::esc_typst_str;
use crate::ink::RegionInk;
use crate::manifest::Manifest;

/// A display-only notice. `M` is the app message type — `Notice` never emits one,
/// so it's a phantom; `Notice<()>` works when no surrounding `Msg` is needed.
pub struct Notice<M = ()> {
    lines: Vec<String>,
    _msg: PhantomData<fn() -> M>,
}

impl<M> Notice<M> {
    /// A notice rendering each string on its own line.
    pub fn new(lines: Vec<String>) -> Self {
        Self {
            lines,
            _msg: PhantomData,
        }
    }

    /// Convenience for a single-line notice.
    pub fn line(text: &str) -> Self {
        Self::new(vec![text.to_string()])
    }
}

impl<M> Component for Notice<M> {
    type Msg = M;

    fn render(&self, _cx: &mut RenderCx) -> String {
        // Each line as red text. The text is injected as a Typst *string
        // expression* (`[#"..."]`), not raw content, so `[`, `]`, `#` in the
        // text stay literal — only `\` and `"` need escaping for the string
        // literal. This keeps arbitrary notice text from breaking the document.
        let mut s = String::new();
        for line in &self.lines {
            let t = esc_typst_str(line);
            s.push_str(&format!("#text(fill: red)[#\"{t}\"]\n\n"));
        }
        s
    }

    fn decode(&self, _ink: &[RegionInk], _manifest: &Manifest) -> Vec<M> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Manifest;

    #[test]
    fn renders_each_line_as_red_text() {
        let n = Notice::<()>::new(vec!["first".into(), "second".into()]);
        let src = n.render(&mut RenderCx::new(0));
        assert!(src.contains("#text(fill: red)[#\"first\"]"));
        assert!(src.contains("#text(fill: red)[#\"second\"]"));
    }

    #[test]
    fn escapes_quote_and_backslash() {
        let n = Notice::<()>::line(r#"a "quote" and a \ backslash"#);
        let src = n.render(&mut RenderCx::new(0));
        // The quote and backslash are escaped for the Typst string literal.
        assert!(src.contains(r#"a \"quote\" and a \\ backslash"#));
    }

    #[test]
    fn brackets_in_text_are_safe_inside_the_string_expr() {
        // `]` would close a raw content block; inside the string expression it's
        // literal, so it appears verbatim and the markup stays balanced.
        let n = Notice::<()>::line("danger ] here");
        let src = n.render(&mut RenderCx::new(0));
        assert!(src.contains("danger ] here"));
        assert!(src.starts_with("#text(fill: red)[#\""));
    }

    #[test]
    fn decode_is_always_empty() {
        let n = Notice::<u8>::line("nothing to decode");
        let manifest = Manifest {
            version: 1,
            regions: vec![],
            ..Default::default()
        };
        assert!(n.decode(&[], &manifest).is_empty());
    }
}
