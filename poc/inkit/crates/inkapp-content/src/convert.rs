//! Pure HTML→Typst converter.

use std::collections::HashSet;

use ego_tree::NodeRef;
use inkapp_core::components::{esc_typst_str, token_region};
use scraper::{Html, Node};
use sha2::{Digest, Sha256};

/// One highlightable prose token. `block` is a per-block-element id so highlight
/// coalescing never merges tokens across structural boundaries.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub text: String,
    pub block: usize,
}

/// The converter's output: Typst markup, the ordered token list (index i ↔
/// region `tok-<i>`), and `(key, url)` for every referenced image (deduped by key).
#[derive(Debug, Clone, Default)]
pub struct Converted {
    pub typst: String,
    pub tokens: Vec<Token>,
    pub images: Vec<(String, String)>,
}

/// Content-addressed image key: first 16 hex chars of `sha256(url)`.
pub fn image_key(url: &str) -> String {
    let digest = Sha256::digest(url.as_bytes());
    let mut s = String::with_capacity(16);
    for b in digest.iter().take(8) {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Subtrees dropped entirely (including their text): the Pass-2 threat set.
const DROP: &[&str] = &[
    "script", "style", "iframe", "noscript", "object", "embed", "form",
];

/// Inline styling carried down the walk.
#[derive(Clone, Copy, Default)]
struct Style {
    bold: bool,
    italic: bool,
    code: bool,
    link: bool,
}

/// Build the `#let t = …` expression so `measure(t)` and display both reflect
/// the inline style. Innermost is `raw("…")` for inline code else a plain string.
fn style_expr(esc: &str, s: Style) -> String {
    let mut e = if s.code {
        format!("raw(\"{esc}\")")
    } else {
        format!("\"{esc}\"")
    };
    if s.italic {
        e = format!("emph({e})");
    }
    if s.bold {
        e = format!("strong({e})");
    }
    if s.link {
        e = format!("underline({e})");
    }
    e
}

struct Conv<'a> {
    out: String,
    tokens: Vec<Token>,
    images: Vec<(String, String)>,
    seen_keys: HashSet<String>,
    highlights: &'a [String],
    block: usize,
}

impl<'a> Conv<'a> {
    fn new(highlights: &'a [String]) -> Self {
        Self {
            out: String::new(),
            tokens: Vec::new(),
            images: Vec::new(),
            seen_keys: HashSet::new(),
            highlights,
            block: 0,
        }
    }

    /// Enter a new block-level element: bump the block id so its tokens can't
    /// coalesce with the previous block's.
    fn start_block(&mut self) {
        self.block += 1;
    }

    fn push_token(&mut self, text: &str, style: Style) {
        let esc = esc_typst_str(text);
        let expr = style_expr(&esc, style);
        let highlighted = self.highlights.iter().any(|h| h == text);
        let i = self.tokens.len();
        self.out.push_str(&token_region(i, &expr, highlighted));
        self.tokens.push(Token {
            text: text.to_string(),
            block: self.block,
        });
    }

    fn push_text(&mut self, text: &str, style: Style) {
        for word in text.split_whitespace() {
            self.push_token(word, style);
        }
    }

    /// Register an http(s) image: emit `#image(...)` markup and (once per key)
    /// record the `(key, url)` pair. Non-http srcs are dropped.
    fn push_image(&mut self, src: &str) -> Option<String> {
        if !(src.starts_with("http://") || src.starts_with("https://")) {
            return None;
        }
        let key = image_key(src);
        if self.seen_keys.insert(key.clone()) {
            self.images.push((key.clone(), src.to_string()));
        }
        Some(key)
    }

    /// Walk a node's children, applying the current inline style.
    fn walk_children(&mut self, node: NodeRef<Node>, style: Style) {
        for child in node.children() {
            match child.value() {
                Node::Text(t) => self.push_text(&t.text, style),
                Node::Element(el) => {
                    let name = el.name();
                    if DROP.contains(&name) {
                        continue; // drop subtree incl. text
                    }
                    self.element(child, name, style);
                }
                _ => {}
            }
        }
    }

    fn element(&mut self, node: NodeRef<Node>, name: &str, style: Style) {
        match name {
            "strong" | "b" => self.walk_children(
                node,
                Style {
                    bold: true,
                    ..style
                },
            ),
            "em" | "i" => self.walk_children(
                node,
                Style {
                    italic: true,
                    ..style
                },
            ),
            "code" => self.walk_children(
                node,
                Style {
                    code: true,
                    ..style
                },
            ),
            "a" => self.walk_children(
                node,
                Style {
                    link: true,
                    ..style
                },
            ),
            "br" => self.out.push_str(" #linebreak() "),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = name[1..].parse::<usize>().unwrap_or(1).clamp(1, 6);
                self.start_block();
                self.out.push_str(&format!("#heading(level: {level})["));
                self.walk_children(node, style);
                self.out.push_str("]\n\n");
            }
            "p" | "div" | "section" | "article" => {
                self.start_block();
                self.walk_children(node, style);
                self.out.push_str("\n\n");
            }
            "blockquote" => {
                self.start_block();
                self.out.push_str("#quote(block: true)[");
                self.walk_children(node, style);
                self.out.push_str("]\n\n");
            }
            "ul" => self.list(node, style, false),
            "ol" => self.list(node, style, true),
            "pre" => self.pre(node),
            "figure" => self.figure(node, style),
            "img" => {
                if let Some(src) = element_attr(node, "src") {
                    if let Some(key) = self.push_image(&src) {
                        self.out
                            .push_str(&format!("#image(\"/assets/{key}.png\", width: 100%)\n"));
                    }
                }
            }
            // Unknown/transparent element: recurse, emit its text (browser-like).
            _ => self.walk_children(node, style),
        }
    }

    fn list(&mut self, node: NodeRef<Node>, style: Style, ordered: bool) {
        self.out.push_str(if ordered { "#enum(" } else { "#list(" });
        for li in node.children() {
            if let Node::Element(el) = li.value() {
                if el.name() == "li" {
                    self.start_block();
                    self.out.push('[');
                    self.walk_children(li, style);
                    self.out.push_str("], ");
                }
            }
        }
        self.out.push_str(")\n\n");
    }

    fn pre(&mut self, node: NodeRef<Node>) {
        self.start_block();
        let text = collect_text(node);
        let esc = esc_typst_str(&text);
        self.out
            .push_str(&format!("#raw(block: true, \"{esc}\")\n\n"));
    }

    fn figure(&mut self, node: NodeRef<Node>, style: Style) {
        self.start_block();
        let img = descend_find(node, "img").and_then(|n| element_attr(n, "src"));
        let key = img.as_deref().and_then(|src| self.push_image(src));
        let caption = descend_find(node, "figcaption").map(|n| {
            collect_text(n)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        });
        match (key, caption) {
            (Some(key), Some(cap)) if !cap.is_empty() => self.out.push_str(&format!(
                "#figure(image(\"/assets/{key}.png\", width: 100%), caption: [#\"{}\"])\n\n",
                esc_typst_str(&cap)
            )),
            (Some(key), _) => self.out.push_str(&format!(
                "#figure(image(\"/assets/{key}.png\", width: 100%))\n\n"
            )),
            // No usable image: recurse so any caption text still appears.
            (None, _) => {
                self.walk_children(node, style);
                self.out.push_str("\n\n");
            }
        }
    }

    fn finish(mut self) -> Converted {
        // A single trailing newline keeps output tidy without affecting layout.
        if !self.out.ends_with('\n') {
            self.out.push('\n');
        }
        Converted {
            typst: self.out,
            tokens: self.tokens,
            images: self.images,
        }
    }
}

/// First descendant element named `name` (depth-first), if any.
fn descend_find<'a>(node: NodeRef<'a, Node>, name: &str) -> Option<NodeRef<'a, Node>> {
    node.descendants().find(|n| match n.value() {
        Node::Element(el) => el.name() == name,
        _ => false,
    })
}

/// Get an attribute off an element node.
fn element_attr(node: NodeRef<Node>, attr: &str) -> Option<String> {
    match node.value() {
        Node::Element(el) => el.attr(attr).map(|s| s.to_string()),
        _ => None,
    }
}

/// Concatenate descendant text, skipping DROP subtrees entirely — so a dangerous
/// element nested inside `<pre>`/`<figure>` never leaks its body as visible text
/// (the flat `descendants()` walk this replaces did not honor the DROP set).
fn collect_text(node: NodeRef<Node>) -> String {
    let mut s = String::new();
    collect_text_into(node, &mut s);
    s
}

fn collect_text_into(node: NodeRef<Node>, out: &mut String) {
    for child in node.children() {
        match child.value() {
            Node::Text(t) => out.push_str(&t.text),
            Node::Element(el) => {
                if DROP.contains(&el.name()) {
                    continue;
                }
                collect_text_into(child, out);
            }
            _ => {}
        }
    }
}

/// Convert sanitized, structured HTML into Typst. `highlights` (matched by token
/// string) renders matching tokens pre-marked.
pub fn convert(html: &str, highlights: &[String]) -> Converted {
    let doc = Html::parse_fragment(html);
    let mut conv = Conv::new(highlights);
    conv.walk_children(doc.tree.root(), Style::default());
    conv.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_key_is_first_16_hex_of_sha256() {
        let k = image_key("https://example.com/cat.jpg");
        assert_eq!(k.len(), 16);
        assert!(k.chars().all(|c| c.is_ascii_hexdigit()));
        let full = format!("{:x}", Sha256::digest(b"https://example.com/cat.jpg"));
        assert_eq!(k, &full[..16]);
    }

    #[test]
    fn heading_emits_typst_heading_with_token() {
        let c = convert("<h2>Hello World</h2>", &[]);
        assert!(c.typst.contains("#heading(level: 2)["));
        assert_eq!(
            c.tokens.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(),
            ["Hello", "World"]
        );
    }

    #[test]
    fn paragraph_tokenizes_words() {
        let c = convert("<p>one two three</p>", &[]);
        assert_eq!(c.tokens.len(), 3);
        assert!(c.typst.contains("name: \"tok-0\""));
        assert!(c.typst.contains("name: \"tok-2\""));
    }

    #[test]
    fn bold_and_italic_fold_into_token_exprs() {
        let c = convert("<p><strong>a</strong> <em>b</em></p>", &[]);
        assert!(c.typst.contains("#let t = strong(\"a\")"));
        assert!(c.typst.contains("#let t = emph(\"b\")"));
    }

    #[test]
    fn link_is_underlined_text_with_no_href() {
        let c = convert("<p><a href=\"https://x.com/evil\">click</a></p>", &[]);
        assert!(c.typst.contains("#let t = underline(\"click\")"));
        assert!(!c.typst.contains("x.com"), "no href/nav leaks");
    }

    #[test]
    fn inline_code_uses_raw() {
        let c = convert("<p>run <code>cargo test</code></p>", &[]);
        assert!(c.typst.contains("#let t = raw(\"cargo\")"));
    }

    #[test]
    fn unordered_and_ordered_lists() {
        let ul = convert("<ul><li>a</li><li>b</li></ul>", &[]);
        assert!(ul.typst.contains("#list("));
        let ol = convert("<ol><li>a</li><li>b</li></ol>", &[]);
        assert!(ol.typst.contains("#enum("));
    }

    #[test]
    fn blockquote_uses_quote_block() {
        let c = convert("<blockquote>wise words</blockquote>", &[]);
        assert!(c.typst.contains("#quote(block: true)["));
    }

    #[test]
    fn pre_is_literal_raw_block_not_tokenized() {
        let c = convert("<pre>let x = 1;\nlet y = 2;</pre>", &[]);
        assert!(c.typst.contains("#raw(block: true,"));
        assert!(c.tokens.is_empty(), "code block is not highlightable");
    }

    #[test]
    fn img_emits_keyed_image_and_pair() {
        let url = "https://example.com/cat.jpg";
        let c = convert(&format!("<p><img src=\"{url}\"></p>"), &[]);
        let key = image_key(url);
        assert!(c
            .typst
            .contains(&format!("#image(\"/assets/{key}.png\", width: 100%)")));
        assert_eq!(c.images, vec![(key, url.to_string())]);
    }

    #[test]
    fn non_http_img_is_dropped() {
        let c = convert("<p><img src=\"data:image/png;base64,AAAA\"></p>", &[]);
        assert!(!c.typst.contains("#image"));
        assert!(c.images.is_empty());
    }

    #[test]
    fn duplicate_image_url_dedupes_pairs() {
        let url = "https://example.com/cat.jpg";
        let c = convert(
            &format!("<p><img src=\"{url}\"><img src=\"{url}\"></p>"),
            &[],
        );
        assert_eq!(c.images.len(), 1, "pairs deduped by key");
        assert_eq!(
            c.typst.matches("#image(").count(),
            2,
            "markup emitted per occurrence"
        );
    }

    #[test]
    fn figure_with_caption() {
        let url = "https://example.com/c.png";
        let c = convert(
            &format!("<figure><img src=\"{url}\"><figcaption>A cat</figcaption></figure>"),
            &[],
        );
        let key = image_key(url);
        assert!(c.typst.contains(&format!(
            "#figure(image(\"/assets/{key}.png\", width: 100%), caption: [#\"A cat\"])"
        )));
    }

    #[test]
    fn block_ids_increment_per_block() {
        let c = convert("<p>a</p><p>b</p>", &[]);
        assert_eq!(c.tokens[0].text, "a");
        assert_eq!(c.tokens[1].text, "b");
        assert_ne!(
            c.tokens[0].block, c.tokens[1].block,
            "different paragraphs are different blocks"
        );
    }

    #[test]
    fn inline_style_does_not_start_a_new_block() {
        let c = convert("<p>plain <strong>bold</strong> end</p>", &[]);
        let blocks: Vec<usize> = c.tokens.iter().map(|t| t.block).collect();
        assert!(
            blocks.windows(2).all(|w| w[0] == w[1]),
            "one paragraph = one block across inline styling"
        );
    }

    #[test]
    fn prior_highlight_renders_pre_marked() {
        let c = convert("<p>keep this</p>", &["this".to_string()]);
        assert!(
            c.typst.contains("#highlight[#t]"),
            "matched token renders pre-marked"
        );
    }

    #[test]
    fn script_subtree_is_dropped_including_text() {
        let c = convert("<p>before<script>alert('xss')</script>after</p>", &[]);
        assert!(!c.typst.contains("alert"));
        assert!(!c.typst.contains("script"));
        assert!(c.typst.contains("#let t = \"before\""));
        assert!(c.typst.contains("#let t = \"after\""));
    }

    #[test]
    fn style_subtree_is_dropped() {
        let c = convert("<style>.x{color:red}</style><p>hi</p>", &[]);
        assert!(!c.typst.contains("color:red"));
        assert!(c.typst.contains("#let t = \"hi\""));
    }

    #[test]
    fn event_handlers_and_inline_styles_never_appear() {
        let c = convert(
            "<p onclick=\"steal()\" style=\"font-family: Comic Sans\">hi</p>",
            &[],
        );
        assert!(!c.typst.contains("onclick"));
        assert!(!c.typst.contains("steal"));
        assert!(!c.typst.contains("font-family"));
        assert!(!c.typst.contains("Comic Sans"));
        assert!(c.typst.contains("#let t = \"hi\""));
    }

    #[test]
    fn iframe_object_embed_form_noscript_dropped() {
        let c = convert(
            "<iframe src=\"https://evil\"></iframe><object>o</object><embed><form>f</form><noscript>n</noscript><p>ok</p>",
            &[],
        );
        assert!(!c.typst.contains("evil"));
        for leak in ["iframe", "object", "embed", "<form", "noscript"] {
            assert!(!c.typst.contains(leak), "{leak} must not leak");
        }
        assert!(c.typst.contains("#let t = \"ok\""));
    }

    #[test]
    fn pre_drops_nested_dangerous_subtree_text() {
        let c = convert("<pre>ok<script>alert(1)</script></pre>", &[]);
        assert!(c.typst.contains("#raw(block: true,"));
        assert!(
            !c.typst.contains("alert"),
            "script body inside pre must not leak"
        );
        assert!(c.typst.contains("ok"));
    }

    #[test]
    fn figcaption_drops_nested_dangerous_subtree_text() {
        let url = "https://example.com/c.png";
        let c = convert(
            &format!("<figure><img src=\"{url}\"><figcaption>cap<style>.x{{}}</style></figcaption></figure>"),
            &[],
        );
        assert!(
            !c.typst.contains(".x{"),
            "style body inside figcaption must not leak"
        );
        assert!(c.typst.contains("cap"));
    }
}
