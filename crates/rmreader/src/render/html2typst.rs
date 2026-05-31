//! Convert sanitized article-body HTML (the output of `content::assemble_processed`)
//! into Typst markup. Inputs are well-behaved: dangerous nodes/attrs are already
//! stripped and `<img src>` is rewritten to a local asset key. We map the common
//! reading subset (paragraphs, headings, bold/italic, links, lists, blockquotes,
//! images, rules) and flatten the rest, matching the fulgur look.
use scraper::{ElementRef, Html, Node};

/// Convert a body HTML fragment to Typst markup. `img` src keys are referenced
/// as `#image("/assets/{key}")` — the caller serves them under `/assets/` in the
/// Typst world.
pub fn convert(html: &str) -> String {
    let doc = Html::parse_fragment(html);
    let mut out = String::new();
    let root = doc.root_element();
    let mut w = Walker { out: &mut out };
    w.children(root, ListCtx::None);
    normalize_blank_lines(out.trim())
}

#[derive(Clone, Copy)]
enum ListCtx {
    None,
    Unordered,
    Ordered,
}

struct Walker<'a> {
    out: &'a mut String,
}

impl Walker<'_> {
    fn children(&mut self, el: ElementRef, list: ListCtx) {
        for child in el.children() {
            match child.value() {
                Node::Text(t) => self.text(&t.text),
                Node::Element(_) => {
                    if let Some(ce) = ElementRef::wrap(child) {
                        self.element(ce, list);
                    }
                }
                _ => {}
            }
        }
    }

    /// Append source text with HTML whitespace collapsing + Typst escaping. A run
    /// of whitespace (incl. newlines) becomes a single space so source line wraps
    /// never turn into Typst paragraph breaks.
    fn text(&mut self, s: &str) {
        let mut prev_ws = self.out.ends_with([' ', '\n']);
        for c in s.chars() {
            if c.is_whitespace() {
                if !prev_ws {
                    self.out.push(' ');
                    prev_ws = true;
                }
            } else {
                push_escaped(self.out, c);
                prev_ws = false;
            }
        }
    }

    fn block_gap(&mut self) {
        let t = self.out.trim_end();
        self.out.truncate(t.len());
        if !self.out.is_empty() {
            self.out.push_str("\n\n");
        }
    }

    fn element(&mut self, el: ElementRef, list: ListCtx) {
        let tag = el.value().name();
        match tag {
            "p" | "div" | "section" | "article" => {
                self.block_gap();
                self.children(el, ListCtx::None);
                self.block_gap();
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let size = match tag {
                    "h1" | "h2" => "12pt",
                    "h3" => "11pt",
                    _ => "10pt",
                };
                self.block_gap();
                self.out.push_str(&format!(
                    "#text(font: \"Lora\", weight: \"semibold\", size: {size}, fill: heading-col)["
                ));
                self.children(el, ListCtx::None);
                self.out.push(']');
                self.block_gap();
            }
            // Use the function form (#strong/#emph), not `*`/`_` markup: Typst's
            // markup emphasis only forms at word boundaries, so a tag that wraps
            // a word fragment (e.g. <em>Moulin Roug</em>e) would leave an
            // "unclosed delimiter". The function form is always balanced.
            "strong" | "b" => {
                self.out.push_str("#strong[");
                self.children(el, list);
                self.out.push(']');
            }
            "em" | "i" => {
                self.out.push_str("#emph[");
                self.children(el, list);
                self.out.push(']');
            }
            "a" => {
                let href = el.value().attr("href").unwrap_or("");
                if href.is_empty() {
                    self.children(el, list);
                } else {
                    self.out
                        .push_str(&format!("#link(\"{}\")[", esc_typst_str(href)));
                    self.children(el, list);
                    self.out.push(']');
                }
            }
            "ul" => {
                self.block_gap();
                self.children(el, ListCtx::Unordered);
                self.block_gap();
            }
            "ol" => {
                self.block_gap();
                self.children(el, ListCtx::Ordered);
                self.block_gap();
            }
            "li" => {
                let t = self.out.trim_end();
                self.out.truncate(t.len());
                self.out.push_str(match list {
                    ListCtx::Ordered => "\n+ ",
                    _ => "\n- ",
                });
                self.children(el, ListCtx::None);
            }
            "blockquote" => {
                self.block_gap();
                self.out.push_str("#quote(block: true)[");
                self.children(el, ListCtx::None);
                self.out.push(']');
                self.block_gap();
            }
            "img" => {
                if let Some(src) = el.value().attr("src") {
                    self.block_gap();
                    self.out.push_str(&format!(
                        "#block(image(\"/assets/{}\", width: 100%))",
                        esc_typst_str(src)
                    ));
                    self.block_gap();
                }
            }
            "figure" => {
                self.block_gap();
                self.children(el, ListCtx::None);
                self.block_gap();
            }
            "figcaption" => {
                self.block_gap();
                self.out
                    .push_str("#text(font: \"Hanken Grotesk\", size: 8pt, fill: muted)[");
                self.children(el, ListCtx::None);
                self.out.push(']');
                self.block_gap();
            }
            "br" => self.out.push_str(" \\\n"),
            "hr" => {
                self.block_gap();
                self.out
                    .push_str("#line(length: 100%, stroke: 0.5pt + rule-col)");
                self.block_gap();
            }
            "code" | "tt" | "kbd" | "samp" => {
                let txt: String = el.text().collect();
                self.out
                    .push_str(&format!("#raw(\"{}\")", esc_typst_str(&txt)));
            }
            "pre" => {
                self.block_gap();
                let txt: String = el.text().collect();
                self.out
                    .push_str(&format!("#raw(block: true, \"{}\")", esc_typst_str(&txt)));
                self.block_gap();
            }
            // Unknown/structural: flatten to children.
            _ => self.children(el, list),
        }
    }
}

/// Escape one char for Typst *markup* (content mode).
fn push_escaped(out: &mut String, c: char) {
    match c {
        '\\' | '#' | '$' | '*' | '_' | '`' | '<' | '>' | '@' | '=' | '~' | '[' | ']' => {
            out.push('\\');
            out.push(c);
        }
        _ => out.push(c),
    }
}

/// Escape a string for a Typst double-quoted string literal.
fn esc_typst_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Collapse runs of 3+ newlines into exactly two (one blank line).
fn normalize_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut nl = 0;
    for c in s.chars() {
        if c == '\n' {
            nl += 1;
            if nl <= 2 {
                out.push(c);
            }
        } else {
            nl = 0;
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraphs_become_blank_line_separated() {
        let t = convert("<p>First paragraph.</p><p>Second one.</p>");
        assert_eq!(t, "First paragraph.\n\nSecond one.");
    }

    #[test]
    fn bold_and_italic_and_links() {
        let t = convert(
            r#"<p>A <strong>bold</strong> and <em>it</em> <a href="https://x.com">lnk</a>.</p>"#,
        );
        assert_eq!(
            t,
            "A #strong[bold] and #emph[it] #link(\"https://x.com\")[lnk]."
        );
    }

    #[test]
    fn images_reference_asset_path() {
        let t = convert(r#"<p>x</p><img src="img-a-0.png"><p>y</p>"#);
        assert!(
            t.contains("#block(image(\"/assets/img-a-0.png\", width: 100%))"),
            "got: {t}"
        );
    }

    #[test]
    fn unordered_list_items() {
        let t = convert("<ul><li>one</li><li>two</li></ul>");
        assert_eq!(t, "- one\n- two");
    }

    #[test]
    fn ordered_list_uses_plus() {
        let t = convert("<ol><li>one</li><li>two</li></ol>");
        assert_eq!(t, "+ one\n+ two");
    }

    #[test]
    fn special_chars_escaped() {
        let t = convert("<p>C# costs $5 *wow*</p>");
        assert!(t.contains("C\\# costs \\$5 \\*wow\\*"), "got: {t}");
    }

    #[test]
    fn source_newlines_do_not_break_paragraphs() {
        let t = convert("<p>line one\n  line two</p>");
        assert_eq!(t, "line one line two");
    }

    #[test]
    fn blockquote_wraps() {
        let t = convert("<blockquote>quoted</blockquote>");
        assert!(t.contains("#quote(block: true)[quoted]"), "got: {t}");
    }
}
