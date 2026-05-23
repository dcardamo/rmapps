// Conversion limits
// =================
// This converter handles the common subset of HTML needed for article bodies.
// The following cases are NOT handled or are lossy:
//
// - Nested lists: only flat <ul>/<ol> with direct <li> children are supported.
//   A <ul> inside an <li> is emitted as inline text without indentation.
//   Typst supports nested lists, but the DOM walk would need explicit depth
//   tracking to emit the right number of leading spaces/plus signs.
//
// - Ordered lists (<ol>): mapped to unordered Typst list items (`- …`) with
//   no numbering. Typst's `+` prefix for numbered lists is not emitted.
//
// - Tables (<table>/<tr>/<td>/<th>): no mapping; children are traversed and
//   their text is emitted inline, losing all tabular structure.
//
// - Images (<img>): no mapping; the `alt` attribute text is NOT emitted
//   (the element is silently skipped). Typst would need an embedded image
//   reference which requires a file on disk.
//
// - Blockquotes (<blockquote>): no visual distinction; rendered as a plain
//   paragraph. Typst has no built-in blockquote primitive, though one could
//   be composed with `pad`.
//
// - Inline code (<code>/<kbd>/<samp>): not mapped to Typst backtick literals;
//   emitted as plain text. Mapping would require `` `text` `` escaping, which
//   conflicts with the existing backtick escape in plain text nodes.
//
// - Code blocks (<pre>): emitted as inline text. Typst raw blocks (``` ... ```)
//   would need the content extracted verbatim without HTML-entity decoding.
//
// - Definition lists (<dl>/<dt>/<dd>): treated as generic containers; text
//   emitted inline.
//
// - Heading levels h2–h6: mapped to `==`–`======` but Typst's default style
//   only distinguishes a few levels visually.
//
// - Attributes other than `href` on <a>: silently ignored. `title`, `target`,
//   `rel`, etc. are dropped.
//
// - HTML entities: handled by html5ever (the scraper back-end) during parsing,
//   so &amp;, &lt;, &gt;, &nbsp; etc. arrive as their Unicode equivalents and
//   are then escaped for Typst. Non-breaking spaces become U+00A0 and are
//   emitted literally; Typst treats them as regular spaces.
//
// - Whitespace normalization: consecutive whitespace in text nodes is
//   collapsed to a single space (matching browser behaviour), but leading/
//   trailing whitespace inside block elements may produce extra blank lines.
//
// - Inline styles and CSS classes: ignored entirely.
//
// - <br> line breaks: emitted as `\` (Typst line-break) followed by a newline,
//   which works inside paragraphs but may produce odd output inside headings.
//
// - Semantic HTML5 elements (<article>, <section>, <aside>, <header>,
//   <footer>, <nav>, <main>): treated as generic containers; children are
//   traversed without adding any block-level spacing.
//
// - <figure>/<figcaption>: treated as generic containers.
//
// - <span> with formatting roles (e.g. class="math"): ignored; text emitted
//   as-is. MathML or LaTeX math is not converted.

use scraper::{Html, Node};

/// Characters that are special in Typst markup and must be escaped
/// when they originate from HTML text nodes (not from our own emitted markup).
const TYPST_SPECIAL: &[char] = &['\\', '*', '_', '#', '@', '$', '`', '<', '>'];

fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if TYPST_SPECIAL.contains(&ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Walk a node's children, appending Typst markup to `out`.
///
/// `in_list` is true when we are inside a `<ul>` or `<ol>` element so that
/// `<li>` children know to emit list-item prefix instead of paragraph breaks.
fn walk_children(node: ego_tree::NodeRef<Node>, in_list: bool, out: &mut String) {
    for child in node.children() {
        walk_node(child, in_list, out);
    }
}

fn walk_node(node: ego_tree::NodeRef<Node>, in_list: bool, out: &mut String) {
    match node.value() {
        Node::Text(text) => {
            // Collapse runs of whitespace to a single space (mirrors browser behaviour).
            let collapsed: String = text.chars().fold(String::new(), |mut acc, ch| {
                if ch.is_ascii_whitespace() {
                    if !acc.ends_with(' ') {
                        acc.push(' ');
                    }
                } else {
                    acc.push(ch);
                }
                acc
            });
            // Skip purely-whitespace nodes so inter-block whitespace text
            // nodes don't produce stray output, but preserve surrounding
            // spaces on inline text (e.g. " paragraph " around <strong>).
            if !collapsed.trim().is_empty() {
                // Preserve a leading space if the original had one and the
                // output buffer doesn't already end with whitespace.
                let leading = collapsed.starts_with(' ')
                    && !out.is_empty()
                    && !out.ends_with(|c: char| c.is_whitespace());
                let trailing = collapsed.ends_with(' ');
                let inner = escape_text(collapsed.trim());
                if leading {
                    out.push(' ');
                }
                out.push_str(&inner);
                if trailing && !inner.is_empty() {
                    out.push(' ');
                }
            }
        }
        Node::Element(el) => {
            let tag = el.name().to_ascii_lowercase();
            match tag.as_str() {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    let level = tag[1..].parse::<usize>().unwrap_or(1);
                    // Ensure heading starts on its own line.
                    if !out.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str(&"=".repeat(level));
                    out.push(' ');
                    // Collect inline text for the heading.
                    let mut heading_text = String::new();
                    walk_children(node, false, &mut heading_text);
                    out.push_str(heading_text.trim());
                    out.push('\n');
                }
                "p" => {
                    if !out.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    walk_children(node, false, out);
                    // Two newlines = blank line = paragraph break in Typst.
                    out.push_str("\n\n");
                }
                "ul" | "ol" => {
                    if !out.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    walk_children(node, true, out);
                    out.push('\n');
                }
                "li" if in_list => {
                    out.push_str("- ");
                    let mut item_text = String::new();
                    walk_children(node, false, &mut item_text);
                    out.push_str(item_text.trim());
                    out.push('\n');
                }
                "strong" | "b" => {
                    out.push('*');
                    walk_children(node, in_list, out);
                    out.push('*');
                }
                "em" | "i" => {
                    out.push('_');
                    walk_children(node, in_list, out);
                    out.push('_');
                }
                "a" => {
                    let href = el.attr("href").unwrap_or("");
                    let mut link_text = String::new();
                    walk_children(node, in_list, &mut link_text);
                    // #link("url")[label]
                    out.push_str(&format!("#link(\"{href}\")[{link_text}]"));
                }
                "br" => {
                    out.push_str("\\\n");
                }
                // Structural/semantic containers — just recurse.
                "html" | "head" | "body" | "article" | "section" | "aside" | "header"
                | "footer" | "nav" | "main" | "div" | "span" | "figure" | "figcaption" | "li" => {
                    walk_children(node, in_list, out);
                }
                // Skip elements that produce no useful text output.
                "script" | "style" | "meta" | "link" | "title" | "img" => {}
                // Unknown tags: recurse into children so we don't silently
                // drop content.
                _ => {
                    walk_children(node, in_list, out);
                }
            }
        }
        // Comments, doctypes, processing instructions — skip.
        _ => {}
    }
}

/// Convert an HTML string to Typst markup.
///
/// Only a representative subset of HTML is mapped; see the "Conversion limits"
/// comment block at the top of this file for what is not supported.
pub fn html_to_typst(html: &str) -> String {
    let document = Html::parse_document(html);
    let mut out = String::new();
    walk_children(document.tree.root(), false, &mut out);
    out
}
