# HTML→Typst Article Content Pipeline — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A reusable `crates/inkapp-content` crate that converts Readwise `html_content` into structured, sanitized Typst with per-token highlight regions, plus an `Article` component that decodes highlighter ink back to coalesced span strings — and rewire the reading-queue app onto it.

**Architecture:** A pure `convert(html, &highlights) -> Converted` walks an html5ever DOM (via `scraper`), emitting Typst for a whitelist of structural/inline elements while dropping dangerous subtrees by construction. Prose text is tokenized into per-token `tok-<i>` `<region>` boxes reusing inkapp-core's proven span recipe (factored into shared helpers). `Article<M>` wraps the converter output, renders the stored Typst, and decodes via block-aware contiguous coalescing. Images are emitted as `#image("/assets/{key}.png")` with `key = sha256(url)[..16]`; the `(key, url)` pairs are returned as the only seam with the parallel image worktree (no fetching here).

**Tech Stack:** Rust, Typst 0.14, `scraper` (html5ever + ego-tree), `sha2`, inkapp-core component/manifest/readback APIs, inkapp-harness simulator.

**Spec:** `docs/superpowers/specs/2026-05-25-html-to-typst-content-design.md`

**Repo conventions (apply to every task):**
- Commit with the hooks-path form so the task-gate hook doesn't block:
  `git -c core.hooksPath=.githooks commit -m "…"` (the literal substring "git commit" must not appear).
- **Never `git add Cargo.lock`** — stage only source + manifests. (A dep change updates the lockfile; leave it unstaged; the controller sweeps it at the very end.)
- Track progress in the co-located `.tasks.json`; keep the native task list empty during execution so the commit hook gate stays clear.
- Tests are network-free. Any test that *compiles* Typst must use image-free HTML (no `/assets/*` is served until the image worktree lands).

---

### Task 1: Shared per-token region helpers in inkapp-core

**Goal:** Factor the load-bearing per-token region recipe and highlight-detection into three reusable `inkapp-core` functions, refactor `HighlightableText` onto them byte-identically.

**Files:**
- Modify: `crates/inkapp-core/src/components/mod.rs` (make `esc_typst_str` pub; add `token_region`, `highlighted_token_indices`)
- Modify: `crates/inkapp-core/src/components/highlight_text.rs` (use the helpers)
- Test: existing `crates/inkapp-core/tests/highlight_text.rs` + `crates/inkapp-harness/tests/exercisers.rs` goldens guard byte-identical output; add a unit test for `highlighted_token_indices`

**Acceptance Criteria:**
- [ ] `esc_typst_str`, `token_region`, `highlighted_token_indices` are `pub` in `inkapp_core::components`
- [ ] `HighlightableText::render`/`read` delegate to the helpers and produce byte-identical Typst and identical `read` results
- [ ] `cargo test -p inkapp-core` and the harness golden tests pass unchanged

**Verify:** `cargo test -p inkapp-core && cargo test -p inkapp-harness --test exercisers` → all pass

**Steps:**

- [ ] **Step 1: Add the helpers to `components/mod.rs`**

Replace the existing `esc_typst_str` definition (make it `pub`) and append the two new helpers:

```rust
use crate::ink::RegionInk;
use crate::manifest::Manifest;

/// Escape a string for a Typst string literal (`"..."`): only `\` and `"` need
/// escaping — other markup chars (`[`, `]`, `#`) are literal inside a string.
pub fn esc_typst_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Emit one inline `#box`ed token whose laid-out rect recovers as region
/// `tok-<index>`. `t_let_expr` is the Typst expression bound to `t` and used for
/// BOTH `measure(t)` (region size) and display, so inline styling
/// (`strong("x")`, `emph("x")`, `raw("x")`, `underline("x")`, or a plain
/// `"x"`) measures and renders correctly. When `highlighted`, the token renders
/// pre-marked via `#highlight`. This is the span-level recipe proven in Typst
/// 0.14.2 (see the `span-level-regions-work-in-typst` memory).
pub fn token_region(index: usize, t_let_expr: &str, highlighted: bool) -> String {
    let disp = if highlighted { "#highlight[#t]" } else { "#t" };
    format!(
        "#box[#let t = {t_let_expr}; #context [#metadata((name: \"tok-{index}\", \
           page: here().position().page - 1, x: here().position().x / 1pt, \
           y: here().position().y / 1pt, w: measure(t).width / 1pt, \
           h: measure(t).height / 1pt)) <region>]{disp}] "
    )
}

/// Indices `0..n` of tokens whose `tok-<i>` region was overlapped by a
/// highlighter stroke. Only highlighter strokes count; a stroke matches when its
/// bbox overlaps the region rect. Ascending order.
pub fn highlighted_token_indices(n: usize, ink: &[RegionInk], manifest: &Manifest) -> Vec<usize> {
    (0..n)
        .filter(|i| {
            let name = format!("tok-{i}");
            let Some(region) = manifest.regions.iter().find(|r| r.name == name) else {
                return false;
            };
            ink.iter()
                .filter(|ri| ri.region == name)
                .flat_map(|ri| &ri.strokes)
                .filter(|s| s.highlighter)
                .any(|s| s.bbox().is_some_and(|b| region.rect.overlaps(&b)))
        })
        .collect()
}
```

- [ ] **Step 2: Refactor `HighlightableText` onto the helpers**

In `crates/inkapp-core/src/components/highlight_text.rs`, change the imports and replace the bodies of `render` and `read` (keep struct/constructors and doc comments):

```rust
use crate::component::RenderCx;
use crate::components::{esc_typst_str, highlighted_token_indices, token_region};
use crate::ink::RegionInk;
use crate::manifest::Manifest;

// ... struct + new/with_highlights unchanged ...

impl HighlightableText {
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

    pub fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<String> {
        highlighted_token_indices(self.tokens.len(), ink, manifest)
            .into_iter()
            .map(|i| self.tokens[i].clone())
            .collect()
    }
}
```

- [ ] **Step 3: Run the existing tests to verify byte-identical behavior**

Run: `cargo test -p inkapp-core --test highlight_text && cargo test -p inkapp-harness --test exercisers`
Expected: PASS. The harness `highlight_lazy_dog` golden is the byte-for-byte guard — if it fails, the refactor changed output and must be corrected to match the original recipe string exactly.

- [ ] **Step 4: Add a focused unit test for `highlighted_token_indices`**

Append to `crates/inkapp-core/tests/highlight_text.rs`:

```rust
#[test]
fn highlighted_token_indices_reports_only_overlapped_highlighter() {
    use inkapp_core::components::highlighted_token_indices;
    use inkapp_core::geometry::PdfRect;
    use inkapp_core::manifest::{Manifest, Region};

    let manifest = Manifest {
        regions: vec![Region {
            name: "tok-1".into(),
            page: 0,
            rect: PdfRect { x0: 0.0, y0: 0.0, x1: 20.0, y1: 10.0 },
        }],
        ..Default::default()
    };
    let hit = vec![RegionInk {
        region: "tok-1".into(),
        strokes: vec![Stroke {
            points: vec![PdfPoint { x: 1.0, y: 1.0 }, PdfPoint { x: 19.0, y: 9.0 }],
            highlighter: true,
        }],
    }];
    assert_eq!(highlighted_token_indices(3, &hit, &manifest), vec![1]);

    let pen = vec![RegionInk {
        region: "tok-1".into(),
        strokes: vec![Stroke {
            points: vec![PdfPoint { x: 1.0, y: 1.0 }],
            highlighter: false,
        }],
    }];
    assert!(highlighted_token_indices(3, &pen, &manifest).is_empty());
}
```

- [ ] **Step 5: Run and commit**

Run: `cargo test -p inkapp-core && cargo test -p inkapp-harness --test exercisers`
Expected: PASS.

```bash
git add crates/inkapp-core/src/components/mod.rs crates/inkapp-core/src/components/highlight_text.rs crates/inkapp-core/tests/highlight_text.rs
git -c core.hooksPath=.githooks commit -m "inkapp-core: factor shared per-token region helpers"
```

---

### Task 2: Scaffold `inkapp-content` crate + image-key helper

**Goal:** Create the crate, register it in the workspace, and implement the content-addressed image key.

**Files:**
- Modify: `Cargo.toml` (add `crates/inkapp-content` to `members`)
- Create: `crates/inkapp-content/Cargo.toml`
- Create: `crates/inkapp-content/src/lib.rs`
- Create: `crates/inkapp-content/src/convert.rs` (image_key only this task)

**Acceptance Criteria:**
- [ ] `crates/inkapp-content` builds as a workspace member
- [ ] `convert::image_key(url)` returns the first 16 hex chars of `sha256(url)`

**Verify:** `cargo test -p inkapp-content` → passes

**Steps:**

- [ ] **Step 1: Add the crate to the workspace**

In the root `Cargo.toml`, add to the `members` array (after `"crates/inkapp"`):

```toml
    "crates/inkapp-content",
```

- [ ] **Step 2: Create `crates/inkapp-content/Cargo.toml`**

```toml
[package]
name = "inkapp-content"
version = "0.1.0"
edition = "2021"
license = "MIT"
description = "HTML→Typst article content pipeline for inkapp: sanitize + structured render + highlightable Article component"

[dependencies]
inkapp-core = { path = "../inkapp-core" }
scraper = "0.20"
sha2 = "0.10"
```

(`scraper`, `sha2`, and their transitive deps are already resolved in `Cargo.lock`. Do **not** `git add Cargo.lock`.)

- [ ] **Step 3: Create `crates/inkapp-content/src/lib.rs`**

```rust
//! HTML→Typst article content pipeline. `convert` is the pure transform;
//! `Article` is the highlightable component apps render.

pub mod article;
pub mod convert;

pub use article::Article;
pub use convert::{convert, Converted, Token};
```

(`article.rs` is created in Task 4. To keep this task building on its own, create a placeholder `crates/inkapp-content/src/article.rs` containing only `//! Article component (implemented in Task 4).` and temporarily comment the `pub mod article;` / `pub use article::Article;` lines, OR create the full module in Task 4 and add those two lines then. Recommended: in THIS task, write `lib.rs` with only the `convert` lines, and add the `article` lines in Task 4.)

For this task, `lib.rs` is:

```rust
//! HTML→Typst article content pipeline. `convert` is the pure transform;
//! `Article` is the highlightable component apps render.

pub mod convert;

pub use convert::{image_key};
```

- [ ] **Step 4: Write the failing test for `image_key`**

Create `crates/inkapp-content/src/convert.rs`:

```rust
//! Pure HTML→Typst converter.

use sha2::{Digest, Sha256};

/// Content-addressed image key: the first 16 hex chars of `sha256(url)`. The
/// converter emits `#image("/assets/{key}.png", …)` and returns `(key, url)` so
/// the image worktree can fetch and serve `/assets/{key}.png`.
pub fn image_key(url: &str) -> String {
    let digest = Sha256::digest(url.as_bytes());
    let mut s = String::with_capacity(16);
    for b in digest.iter().take(8) {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_key_is_first_16_hex_of_sha256() {
        let k = image_key("https://example.com/cat.jpg");
        assert_eq!(k.len(), 16);
        assert!(k.chars().all(|c| c.is_ascii_hexdigit()));
        // Deterministic + first-16-of-full-digest.
        let full = format!("{:x}", Sha256::digest(b"https://example.com/cat.jpg"));
        assert_eq!(k, &full[..16]);
    }
}
```

- [ ] **Step 5: Run and commit**

Run: `cargo test -p inkapp-content`
Expected: PASS.

```bash
git add Cargo.toml crates/inkapp-content/Cargo.toml crates/inkapp-content/src/lib.rs crates/inkapp-content/src/convert.rs
git -c core.hooksPath=.githooks commit -m "inkapp-content: scaffold crate + image_key helper"
```

---

### Task 3: The converter — sanitize + structured HTML→Typst

**Goal:** Implement `convert(html, &highlights) -> Converted`: a whitelist DOM walk emitting Typst for headings, paragraphs, bold/italic, links, lists, blockquotes, inline/block code, and figures/images, dropping dangerous subtrees and never reading dangerous attributes.

**Files:**
- Modify: `crates/inkapp-content/src/convert.rs`
- Modify: `crates/inkapp-content/src/lib.rs` (export `convert`, `Converted`, `Token`)

**Acceptance Criteria:**
- [ ] `convert` returns `Converted { typst, tokens, images }`
- [ ] Each prose token is a `tok-<i>` region box; `tokens[i].block` is unique per block-level element
- [ ] script/style/iframe/noscript/object/embed/form subtrees (incl. text) never appear in output; `on*` and `style` attrs never appear
- [ ] `<img http(s)>` emits `#image("/assets/{key}.png", width: 100%)` and one deduped `(key, url)` pair; non-http srcs are dropped
- [ ] All construct + sanitizer unit tests pass

**Verify:** `cargo test -p inkapp-content` → all pass

**Steps:**

- [ ] **Step 1: Replace `convert.rs` with the full converter (keep `image_key` + its test)**

```rust
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
            "strong" | "b" => self.walk_children(node, Style { bold: true, ..style }),
            "em" | "i" => self.walk_children(node, Style { italic: true, ..style }),
            "code" => self.walk_children(node, Style { code: true, ..style }),
            "a" => self.walk_children(node, Style { link: true, ..style }),
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
            collect_text(n).split_whitespace().collect::<Vec<_>>().join(" ")
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
fn descend_find(node: NodeRef<Node>, name: &str) -> Option<NodeRef<Node>> {
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

/// Concatenate all descendant text (preserving content; used for code/captions).
fn collect_text(node: NodeRef<Node>) -> String {
    let mut s = String::new();
    for d in node.descendants() {
        if let Node::Text(t) = d.value() {
            s.push_str(&t.text);
        }
    }
    s
}

/// Convert sanitized, structured HTML into Typst. `highlights` (matched by token
/// string) renders matching tokens pre-marked.
pub fn convert(html: &str, highlights: &[String]) -> Converted {
    let doc = Html::parse_fragment(html);
    let mut conv = Conv::new(highlights);
    conv.walk_children(doc.tree.root(), Style::default());
    conv.finish()
}
```

- [ ] **Step 2: Export the new types from `lib.rs`**

Set `crates/inkapp-content/src/lib.rs` to:

```rust
//! HTML→Typst article content pipeline. `convert` is the pure transform;
//! `Article` is the highlightable component apps render.

pub mod convert;

pub use convert::{convert, image_key, Converted, Token};
```

(The `article` module + re-export are added in Task 4.)

- [ ] **Step 3: Run to verify it compiles, then add construct tests**

Run: `cargo build -p inkapp-content`
Expected: compiles. If `scraper`/`ego_tree` API names differ in the resolved version, adjust (`el.name()`, `el.attr(..)`, `t.text`, `node.children()`, `node.descendants()` are the surfaces used).

Append to `convert.rs` `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn heading_emits_typst_heading_with_token() {
        let c = convert("<h2>Hello World</h2>", &[]);
        assert!(c.typst.contains("#heading(level: 2)["));
        assert_eq!(c.tokens.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), ["Hello", "World"]);
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
        assert!(c.typst.contains(&format!("#image(\"/assets/{key}.png\", width: 100%)")));
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
        let c = convert(&format!("<p><img src=\"{url}\"><img src=\"{url}\"></p>"), &[]);
        assert_eq!(c.images.len(), 1, "pairs deduped by key");
        assert_eq!(c.typst.matches("#image(").count(), 2, "markup emitted per occurrence");
    }

    #[test]
    fn figure_with_caption() {
        let url = "https://example.com/c.png";
        let c = convert(&format!("<figure><img src=\"{url}\"><figcaption>A cat</figcaption></figure>"), &[]);
        let key = image_key(url);
        assert!(c.typst.contains(&format!("#figure(image(\"/assets/{key}.png\", width: 100%), caption: [#\"A cat\"])")));
    }

    #[test]
    fn block_ids_increment_per_block() {
        let c = convert("<p>a</p><p>b</p>", &[]);
        assert_eq!(c.tokens[0].text, "a");
        assert_eq!(c.tokens[1].text, "b");
        assert_ne!(c.tokens[0].block, c.tokens[1].block, "different paragraphs are different blocks");
    }

    #[test]
    fn inline_style_does_not_start_a_new_block() {
        let c = convert("<p>plain <strong>bold</strong> end</p>", &[]);
        let blocks: Vec<usize> = c.tokens.iter().map(|t| t.block).collect();
        assert!(blocks.windows(2).all(|w| w[0] == w[1]), "one paragraph = one block across inline styling");
    }

    #[test]
    fn prior_highlight_renders_pre_marked() {
        let c = convert("<p>keep this</p>", &["this".to_string()]);
        assert!(c.typst.contains("#highlight[#t]"), "matched token renders pre-marked");
    }
```

- [ ] **Step 4: Add sanitizer tests**

Append to the same test module:

```rust
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
        let c = convert("<p onclick=\"steal()\" style=\"font-family: Comic Sans\">hi</p>", &[]);
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
```

- [ ] **Step 5: Run and commit**

Run: `cargo test -p inkapp-content`
Expected: PASS.

```bash
git add crates/inkapp-content/src/convert.rs crates/inkapp-content/src/lib.rs
git -c core.hooksPath=.githooks commit -m "inkapp-content: whitelist HTML→Typst converter + sanitizer"
```

---

### Task 4: `Article` component + coalescing decode

**Goal:** A reusable `Article<M>` component that renders the converted Typst, decodes highlighter ink to block-aware coalesced span strings, and exposes the image pairs.

**Files:**
- Create: `crates/inkapp-content/src/article.rs`
- Modify: `crates/inkapp-content/src/lib.rs` (add `article` module + re-export)
- Test: in `article.rs` (`#[cfg(test)]`) — coalescing, region-recovery via compile, prior-highlight pre-marking

**Acceptance Criteria:**
- [ ] `Article::new(html, &highlights, on_highlight)` builds once via `convert`
- [ ] `read` returns block-aware contiguous-coalesced span strings (e.g. `["important note"]`, gaps split, no cross-block merge)
- [ ] `impl Component`: `render` returns the stored Typst; `decode` maps spans through `on_highlight`
- [ ] `images()` exposes the `(key, url)` seam
- [ ] Region recovery test: compiling the rendered Typst recovers one `tok-<i>` region per token

**Verify:** `cargo test -p inkapp-content` → all pass

**Steps:**

- [ ] **Step 1: Create `crates/inkapp-content/src/article.rs`**

```rust
//! `Article` — a Capture-mode component for real (HTML) articles. It renders
//! structured, sanitized Typst with per-token highlight regions and decodes
//! highlighter ink into coalesced span strings, each mapped to an app message.

use inkapp_core::component::{Component, RenderCx};
use inkapp_core::components::highlighted_token_indices;
use inkapp_core::ink::RegionInk;
use inkapp_core::manifest::Manifest;

use crate::convert::{convert, Converted};

/// A highlightable article. `M` is the app message; `on_highlight` builds one
/// message per coalesced highlighted span (the appdx-sanctioned escape hatch for
/// a reusable content component whose message depends on what was decoded).
pub struct Article<M> {
    converted: Converted,
    on_highlight: Box<dyn Fn(&str) -> M>,
}

impl<M> Article<M> {
    /// Convert `html` once (rendering tokens in `highlights` pre-marked).
    pub fn new(
        html: &str,
        highlights: &[String],
        on_highlight: impl Fn(&str) -> M + 'static,
    ) -> Self {
        Self {
            converted: convert(html, highlights),
            on_highlight: Box::new(on_highlight),
        }
    }

    /// `(key, url)` for every referenced image — the only seam with the image
    /// worktree (which fetches and serves `/assets/{key}.png`).
    pub fn images(&self) -> &[(String, String)] {
        &self.converted.images
    }

    /// Highlighted spans, in document order, with index-adjacent tokens that
    /// share a block coalesced into one space-joined string.
    pub fn read(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<String> {
        let tokens = &self.converted.tokens;
        let hits = highlighted_token_indices(tokens.len(), ink, manifest);

        let mut spans: Vec<String> = Vec::new();
        let mut run: Vec<usize> = Vec::new();
        let flush = |run: &mut Vec<usize>, spans: &mut Vec<String>| {
            if !run.is_empty() {
                let s = run.iter().map(|&i| tokens[i].text.as_str()).collect::<Vec<_>>().join(" ");
                spans.push(s);
                run.clear();
            }
        };
        for &i in &hits {
            let contiguous = run
                .last()
                .is_some_and(|&p| i == p + 1 && tokens[i].block == tokens[p].block);
            if !contiguous {
                flush(&mut run, &mut spans);
            }
            run.push(i);
        }
        flush(&mut run, &mut spans);
        spans
    }
}

impl<M> Component for Article<M> {
    type Msg = M;

    fn render(&self, _cx: &mut RenderCx) -> String {
        self.converted.typst.clone()
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<M> {
        self.read(ink, manifest)
            .iter()
            .map(|s| (self.on_highlight)(s))
            .collect()
    }
}
```

- [ ] **Step 2: Wire the module into `lib.rs`**

Set `crates/inkapp-content/src/lib.rs` to:

```rust
//! HTML→Typst article content pipeline. `convert` is the pure transform;
//! `Article` is the highlightable component apps render.

pub mod article;
pub mod convert;

pub use article::Article;
pub use convert::{convert, image_key, Converted, Token};
```

- [ ] **Step 3: Add coalescing unit tests (hand-built manifest, no compile)**

Append to `article.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use inkapp_core::geometry::{PdfPoint, PdfRect};
    use inkapp_core::ink::Stroke;
    use inkapp_core::manifest::{Manifest, Region};

    // Build a manifest with one rect per token index, and highlighter ink over
    // the chosen indices, so `read` can be exercised without compiling Typst.
    fn manifest_for(n: usize) -> Manifest {
        Manifest {
            regions: (0..n)
                .map(|i| Region {
                    name: format!("tok-{i}"),
                    page: 0,
                    rect: PdfRect { x0: i as f64 * 10.0, y0: 0.0, x1: i as f64 * 10.0 + 8.0, y1: 10.0 },
                })
                .collect(),
            ..Default::default()
        }
    }

    fn swipe(indices: &[usize], m: &Manifest) -> Vec<RegionInk> {
        indices
            .iter()
            .map(|&i| {
                let r = m.regions.iter().find(|r| r.name == format!("tok-{i}")).unwrap().rect;
                RegionInk {
                    region: format!("tok-{i}"),
                    strokes: vec![Stroke {
                        points: vec![PdfPoint { x: r.x0 + 1.0, y: 5.0 }, PdfPoint { x: r.x1 - 1.0, y: 5.0 }],
                        highlighter: true,
                    }],
                }
            })
            .collect()
    }

    #[test]
    fn contiguous_tokens_coalesce_across_inline_styling() {
        // "very important note" — all one block; highlight "important note".
        let a = Article::new("<p>very <strong>important</strong> note</p>", &[], |s| s.to_string());
        let m = manifest_for(a.converted.tokens.len());
        let got = a.read(&swipe(&[1, 2], &m), &m);
        assert_eq!(got, vec!["important note".to_string()]);
    }

    #[test]
    fn gap_splits_into_separate_spans() {
        let a = Article::new("<p>a b c</p>", &[], |s| s.to_string());
        let m = manifest_for(a.converted.tokens.len());
        let got = a.read(&swipe(&[0, 2], &m), &m);
        assert_eq!(got, vec!["a".to_string(), "c".to_string()]);
    }

    #[test]
    fn block_boundary_prevents_merge() {
        let a = Article::new("<p>a</p><p>b</p>", &[], |s| s.to_string());
        let m = manifest_for(a.converted.tokens.len());
        // tok-0 and tok-1 are index-adjacent but in different blocks.
        let got = a.read(&swipe(&[0, 1], &m), &m);
        assert_eq!(got, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn decode_maps_spans_through_on_highlight() {
        #[derive(Debug, PartialEq)]
        struct Hi(String);
        let a = Article::new("<p>a b</p>", &[], |s| Hi(s.to_string()));
        let m = manifest_for(a.converted.tokens.len());
        assert_eq!(a.decode(&swipe(&[0, 1], &m), &m), vec![Hi("a b".to_string())]);
    }

    #[test]
    fn images_seam_is_exposed() {
        let url = "https://example.com/x.png";
        let a: Article<String> = Article::new(&format!("<p><img src=\"{url}\"></p>"), &[], |s| s.to_string());
        assert_eq!(a.images(), &[(crate::image_key(url), url.to_string())]);
    }
}
```

- [ ] **Step 4: Add a region-recovery test that actually compiles the rendered Typst**

Append to the same test module (proves the per-token recipe survives structural nesting — headings/lists/bold):

```rust
    #[test]
    fn rendered_typst_recovers_one_region_per_token() {
        use inkapp_core::component::RenderCx;
        use inkapp_core::manifest::recover_regions;
        use inkapp_core::render::compile_to_document;

        let a: Article<String> =
            Article::new("<h2>Title</h2><p>the <strong>quick</strong> fox</p><ul><li>one</li></ul>", &[], |s| s.to_string());
        let body = a.render(&mut RenderCx::new(0));
        let src = format!("#set page(width: 400pt, height: 600pt, margin: 16pt)\n{body}");
        let doc = compile_to_document(&src).expect("structured article compiles");
        let m = recover_regions(&doc).unwrap();
        let toks = m.regions.iter().filter(|r| r.name.starts_with("tok-")).count();
        assert_eq!(toks, a.converted.tokens.len(), "every token recovers as a region through headings/lists/bold");
    }
```

- [ ] **Step 5: Run and commit**

Run: `cargo test -p inkapp-content`
Expected: PASS.

```bash
git add crates/inkapp-content/src/article.rs crates/inkapp-content/src/lib.rs
git -c core.hooksPath=.githooks commit -m "inkapp-content: Article component with coalescing decode"
```

---

### Task 5: Harness exerciser — render→recover→decode through the simulator

**Goal:** Prove `Article` decodes a real highlighter swipe end-to-end through the inkapp-harness simulator (like `checkbox_exerciser`/`highlight_exerciser`).

**Files:**
- Modify: `crates/inkapp-harness/Cargo.toml` (add `inkapp-content` dev-dependency)
- Create: `crates/inkapp-harness/tests/article_exerciser.rs`

**Acceptance Criteria:**
- [ ] An image-free structured article renders, recovers regions, a simulated swipe over chosen tokens decodes to the expected coalesced span
- [ ] No dependency cycle (inkapp-content does not depend on inkapp-harness)

**Verify:** `cargo test -p inkapp-harness --test article_exerciser` → passes

**Steps:**

- [ ] **Step 1: Add the dev-dependency**

In `crates/inkapp-harness/Cargo.toml` under `[dev-dependencies]`:

```toml
inkapp-content = { path = "../inkapp-content" }
```

- [ ] **Step 2: Write the exerciser**

Create `crates/inkapp-harness/tests/article_exerciser.rs`:

```rust
use inkapp_content::Article;
use inkapp_core::component::{Component, RenderCx};
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::compile_to_document;
use inkapp_harness::simulator::{simulate, Gesture, Scenario};
use inkapp_remarkable::Remarkable;

#[test]
fn article_decodes_swipe_to_coalesced_span() {
    // Image-free structured HTML so it compiles under today's InkWorld.
    let html = "<h2>Heading</h2><p>the quick brown <strong>fox</strong> jumps</p>\
                <ul><li>alpha</li><li>beta</li></ul>";
    let article: Article<String> = Article::new(html, &[], |s| s.to_string());

    let body = article.render(&mut RenderCx::new(0));
    let src = format!("#set page(width: 400pt, height: 600pt, margin: 16pt)\n{body}");
    let doc = compile_to_document(&src).unwrap();
    let manifest = recover_regions(&doc).unwrap().with_version(1);
    let device = Remarkable::new();

    // Find the token indices for "brown" and "fox" (contiguous, same block) so
    // the swipe should coalesce them into one span.
    let idx = |w: &str| {
        article
            .images(); // no-op touch to keep `article` borrow simple
        let tokens = render_tokens(&article);
        tokens.iter().position(|t| t == w).unwrap()
    };
    let brown = idx("brown");
    let fox = idx("fox");

    let scenario = Scenario::new()
        .mark(&format!("tok-{brown}"), Gesture::Swipe)
        .mark(&format!("tok-{fox}"), Gesture::Swipe);
    let trace = simulate(&src, &manifest, &device, &scenario).unwrap();

    let got = article.read(&trace.readback, &manifest);
    assert_eq!(got, vec!["brown fox".to_string()], "contiguous swipe coalesces");
}

// The token list isn't public; reconstruct it by converting the same HTML.
fn render_tokens(_a: &Article<String>) -> Vec<String> {
    inkapp_content::convert(
        "<h2>Heading</h2><p>the quick brown <strong>fox</strong> jumps</p>\
         <ul><li>alpha</li><li>beta</li></ul>",
        &[],
    )
    .tokens
    .into_iter()
    .map(|t| t.text)
    .collect()
}
```

> Note: `Article`'s token list is intentionally private. The exerciser recovers token indices by calling the public `convert` on the same HTML. If you prefer, instead hardcode the indices after inspecting `convert(html, &[]).tokens` once. Keep the HTML identical between `Article::new` and `render_tokens`.

- [ ] **Step 3: Run and commit**

Run: `cargo test -p inkapp-harness --test article_exerciser`
Expected: PASS. If `brown`/`fox` are not contiguous token indices (they should be: "the","quick","brown","fox","jumps" → 2,3), inspect `convert(html,&[]).tokens` and adjust.

```bash
git add crates/inkapp-harness/Cargo.toml crates/inkapp-harness/tests/article_exerciser.rs
git -c core.hooksPath=.githooks commit -m "inkapp-harness: Article render→recover→decode exerciser"
```

---

### Task 6: Rewire reading-queue onto `Article`

**Goal:** `ArticleBody` renders via `inkapp_content::Article` when `html_content` is present (coalesced spans), else falls back to whitespace-split `HighlightableText`. Keep `--workspace` green.

**Files:**
- Modify: `apps/reading-queue/Cargo.toml` (add `inkapp-content` dep)
- Modify: `apps/reading-queue/src/lib.rs` (`ArticleBody` becomes HTML-or-plaintext)
- Modify: `apps/reading-queue/tests/app.rs` (existing test stays on plaintext path; add an HTML-path test)

**Acceptance Criteria:**
- [ ] `ArticleBody::new` uses the HTML pipeline when `a.html_content` is `Some(non-empty)`, else plaintext
- [ ] HTML path emits `Msg::Highlighted` once per coalesced span; plaintext path unchanged (per token)
- [ ] `cargo test --workspace` passes

**Verify:** `cargo test --workspace` → all pass

**Steps:**

- [ ] **Step 1: Add the dependency**

In `apps/reading-queue/Cargo.toml` under `[dependencies]`:

```toml
inkapp-content = { path = "../../crates/inkapp-content" }
```

- [ ] **Step 2: Rewrite `ArticleBody` in `apps/reading-queue/src/lib.rs`**

Replace the `ArticleBody` struct, its `impl`, and the `Component` impl (lines ~109-143) with:

```rust
/// A bespoke, app-specific content component. Renders real article HTML via the
/// content pipeline when present (decoding to coalesced highlight spans), and
/// falls back to whitespace-split plaintext for articles without `html_content`.
enum Body {
    Html(inkapp_content::Article<Msg>),
    Plain(HighlightableText),
}

pub struct ArticleBody {
    article: ArticleId,
    body: Body,
}

impl ArticleBody {
    pub fn new(a: &Article) -> Self {
        let body = match a.html_content.as_deref() {
            Some(html) if !html.trim().is_empty() => {
                let id = a.id.clone();
                Body::Html(inkapp_content::Article::new(html, &a.highlights, move |s| {
                    Msg::Highlighted { article: id.clone(), text: s.to_string() }
                }))
            }
            _ => {
                let tokens: Vec<&str> = a.body.split_whitespace().collect();
                Body::Plain(HighlightableText::with_highlights(&tokens, &a.highlights))
            }
        };
        Self { article: a.id.clone(), body }
    }
}

impl Component for ArticleBody {
    type Msg = Msg;

    fn render(&self, cx: &mut RenderCx) -> String {
        match &self.body {
            Body::Html(a) => a.render(cx),
            Body::Plain(h) => h.render(cx),
        }
    }

    fn decode(&self, ink: &[RegionInk], manifest: &Manifest) -> Vec<Msg> {
        match &self.body {
            Body::Html(a) => a.decode(ink, manifest),
            Body::Plain(h) => h
                .read(ink, manifest)
                .into_iter()
                .map(|text| Msg::Highlighted { article: self.article.clone(), text })
                .collect(),
        }
    }
}
```

- [ ] **Step 3: Run the existing reading-queue tests (plaintext path must be unchanged)**

Run: `cargo test -p reading-queue`
Expected: PASS — `article_body_decodes_highlight_to_msg` uses `body: "lazy dog"` with no `html_content`, so it stays on the plaintext per-token path and still expects `["lazy"]`.

- [ ] **Step 4: Add an HTML-path test (coalesced span)**

Append to `apps/reading-queue/tests/app.rs`:

```rust
#[test]
fn article_body_html_path_decodes_coalesced_span() {
    use inkapp_readwise_reader::Article;

    let article = Article {
        id: ArticleId::new("a1"),
        title: "T".into(),
        html_content: Some("<p>quick brown fox</p>".into()),
        highlights: vec![],
        ..Article::default()
    };
    let body = ArticleBody::new(&article);

    // tok-0="quick", tok-1="brown" — a swipe over both, in one block, coalesces.
    let manifest = Manifest {
        version: 1,
        regions: vec![
            Region { name: "tok-0".into(), page: 0, rect: PdfRect { x0: 0.0, y0: 0.0, x1: 20.0, y1: 12.0 } },
            Region { name: "tok-1".into(), page: 0, rect: PdfRect { x0: 22.0, y0: 0.0, x1: 45.0, y1: 12.0 } },
        ],
        ..Default::default()
    };
    let stroke = Stroke {
        points: vec![PdfPoint { x: 2.0, y: 6.0 }, PdfPoint { x: 44.0, y: 6.0 }],
        highlighter: true,
    };
    let ink = vec![
        RegionInk { region: "tok-0".into(), strokes: vec![stroke.clone()] },
        RegionInk { region: "tok-1".into(), strokes: vec![stroke] },
    ];

    assert_eq!(
        body.decode(&ink, &manifest),
        vec![Msg::Highlighted { article: ArticleId::new("a1"), text: "quick brown".to_string() }]
    );
}
```

- [ ] **Step 5: Run the whole workspace and commit**

Run: `cargo test --workspace`
Expected: PASS (this is the cross-crate gate per the `workspace-has-apps-dir` memory).

```bash
git add apps/reading-queue/Cargo.toml apps/reading-queue/src/lib.rs apps/reading-queue/tests/app.rs
git -c core.hooksPath=.githooks commit -m "reading-queue: render real articles via inkapp-content Article"
```

---

### Task 7: Mark the capability built + final workspace gate + lockfile sweep

**Goal:** Record the new capability in the definition-of-done doc, confirm the full workspace is green, and commit the accumulated `Cargo.lock` once.

**Files:**
- Modify: `docs/appdx.md`
- Commit: `Cargo.lock` (the only place it is staged in this plan)

**Acceptance Criteria:**
- [ ] `docs/appdx.md` states the HTML→Typst content pipeline is built (image fetch/serve remains the parallel worktree)
- [ ] `cargo test --workspace` passes
- [ ] `cargo build --workspace --locked` passes (lockfile is consistent)

**Verify:** `cargo test --workspace && cargo build --workspace --locked` → pass

**Steps:**

- [ ] **Step 1: Update `docs/appdx.md`**

Find this sentence in the "Beyond the spine" block (near the top):

```
reads. **Pagination** (so apps never think in pages) and the **HTML→Typst content +
image pipeline** are the next worktrees.
```

Replace it with:

```
reads, with **pagination** (so apps never think in pages) already built. The
**HTML→Typst article content pipeline** is now built too: the reusable
`inkapp-content` crate sanitizes Readwise `html_content` and converts it to
structured Typst (headings, bold/italic, links, lists, blockquotes, code, figures)
with per-token highlight regions, and its `Article` component decodes highlighter
ink into coalesced span strings — replacing whitespace-split plaintext in the
reading-queue app. Image **fetching and serving** remains the one parallel
worktree, wired through `Article`'s `(key, url)` image contract.
```

- [ ] **Step 2: Run the final gate**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 3: Sweep the lockfile and confirm a locked build**

The new `scraper`/`sha2` usage may have updated `Cargo.lock` during earlier tasks (where it was deliberately left unstaged). Stage it once here and verify a locked build:

Run: `cargo build --workspace --locked`
Expected: PASS (no "Cargo.lock needs to be updated"). If it fails, run `cargo build --workspace` first to refresh the lockfile, then re-run with `--locked`.

```bash
git add docs/appdx.md Cargo.lock
git -c core.hooksPath=.githooks commit -m "inkapp-content: mark HTML→Typst content pipeline built (appdx) + lockfile"
```

---

## Self-Review

**Spec coverage:**
- Pure HTML→Typst converter → Task 3 ✓
- Sanitizer (Pass-2 threat set, whitelist-by-construction) → Task 3 ✓
- All constructs (headings, bold/italic, links, ul/ol, blockquote, code/pre, paragraphs, figures) → Task 3 ✓
- Per-token `tok-<i>` regions reusing the proven recipe → Tasks 1, 3 ✓
- Prior highlights pre-marked → Tasks 3 (string), 4 (component) ✓
- Image contract (`/assets/{key}.png`, `sha256(url)[..16]`, `(key,url)` pairs, no fetching) → Tasks 2, 3 ✓
- `Article` impl Component, decode shape = coalesced spans → Task 4 ✓
- Converter unit tests + sanitizer tests + Article render→recover→decode through harness → Tasks 3, 4, 5 ✓
- reading-queue rewire → Task 6 ✓
- `cargo test --workspace` green → Tasks 6, 7 ✓
- Mark built in `docs/appdx.md` → Task 7 ✓
- Repo conventions (hooks-path commits, no Cargo.lock staging until the end, `.tasks.json`) → header + every task ✓

**Placeholder scan:** No TBD/TODO; every code step shows full code; every test shows assertions.

**Type consistency:** `Converted{typst,tokens,images}`, `Token{text,block}`, `image_key`, `token_region`, `highlighted_token_indices`, `Article::{new,read,images,render,decode}`, `Body::{Html,Plain}` are used identically across tasks. `convert` and `Token` are public (used by the Task 5 exerciser).

**Known risk flagged:** the central bet is that the per-token `#context`/`measure` recipe survives inside `#heading`/`#list`/`#quote`. Task 4 step 4 and Task 5 compile real nested structures and assert region recovery, so a regression fails fast.
