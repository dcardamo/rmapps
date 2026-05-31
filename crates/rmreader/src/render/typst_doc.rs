//! Build the full Typst document for a reader collection, reproducing the
//! fulgur/CSS look: an index page (Fraunces masthead + numbered rows) followed
//! by full articles, with a per-page chrome header (indigo nav bar + action
//! band) on every article page.
//!
//! The builder emits one self-contained Typst source string (helper `#let`s
//! inlined at the top — no external `.typ` files), plus the list of image
//! assets the source references via `#image("/assets/…")`.
use crate::device::Device;
use crate::theme::Palette;

/// One index row: tap-target into a full article.
pub struct Row {
    pub num: String,
    pub title: String,
    pub author: String,
    pub reading_time: String,
    pub anchor: String, // article id, used for <art-{id}> link target
}

/// One full article. `body` is already-converted Typst markup (see
/// `html2typst`); `title`/`byline` are plain text.
pub struct Article {
    pub anchor: String,
    pub title: String,
    pub byline: String,
    pub body: String,
}

/// The four action labels stamped in the per-page action band, in column order.
pub const ACTION_LABELS: [&str; 4] = ["INBOX", "ARCHIVE", "LATER", "DELETE"];

/// Build a valid Typst string-array literal from items: `()` when empty,
/// `("a", "b", )` otherwise. The trailing comma keeps a single-element literal
/// from parsing as grouping parens rather than a 1-element array.
fn typst_array(items: impl Iterator<Item = String>) -> String {
    let inner = items
        .map(|s| format!("\"{}\"", esc_str(&s)))
        .collect::<Vec<_>>()
        .join(", ");
    if inner.is_empty() {
        "()".to_string()
    } else {
        format!("({inner}, )")
    }
}

fn color(theme: &Palette, key: &str, fallback: &str) -> String {
    let hex = theme.get(key).map(|s| s.as_str()).unwrap_or(fallback);
    format!("rgb(\"{hex}\")")
}

/// Escape a Rust string for inclusion inside a Typst double-quoted string
/// literal (used for ids and link targets).
pub fn esc_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Escape arbitrary text for Typst *markup* (content mode). Backslash and the
/// markup-significant characters are escaped so titles/bylines render verbatim.
pub fn esc_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | '#' | '$' | '*' | '_' | '`' | '<' | '>' | '@' | '=' | '~' | '"' | '\'' | '['
            | ']' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Build the complete Typst source for a collection.
pub fn build(
    device: &Device,
    theme: &Palette,
    collection: &str,
    rows: &[Row],
    articles: &[Article],
) -> String {
    let w = device.width_pt();
    let h = device.height_pt();

    let paper = color(theme, "paper", "#F3F1EA");
    let ink = color(theme, "ink", "#1A1A18");
    let heading = color(theme, "heading", "#2A2F6B");
    let accent = color(theme, "accent", "#CF3A2B");
    let muted = color(theme, "muted", "#5E6166");
    let byline_c = color(theme, "byline", "#9C3A1B");
    let rule = color(theme, "rule", "#E0DDD2");
    let faint = color(theme, "faint", "#9D9C93");
    let navbg = color(theme, "navbg", "#2A2F6B");
    let navfg = color(theme, "navfg", "#F4F1E8");

    // Ordered article ids, baked once for the nav band's Prev/Next resolution.
    // Emit a valid Typst array literal — `()` when empty (a bare `(, )` is a
    // syntax error), `("a", "b", )` otherwise (trailing comma forces array, not
    // grouping parens, for the single-element case).
    let order = typst_array(articles.iter().map(|a| esc_str(&a.anchor)));
    let action_arr = typst_array(ACTION_LABELS.iter().map(|l| l.to_string()));

    let mut s = String::new();

    // ---- Preamble: colours + helpers --------------------------------------
    s.push_str(&format!(
        r#"#let paper = {paper}
#let ink = {ink}
#let heading-col = {heading}
#let accent = {accent}
#let muted = {muted}
#let byline-col = {byline_c}
#let rule-col = {rule}
#let faint-col = {faint}
#let navbg = {navbg}
#let navfg = {navfg}
#let order = {order}
#let action-labels = {action_arr}

// Current-article state: set at each article's start so the per-page header
// knows which page belongs to which article (Prev/Next + action targets).
#let section-state = state("rmreader.section", "")

// Emit a recoverable region rectangle as <region>-labelled metadata.
#let region(name, body) = box[
  #context [
    #metadata((
      name: name,
      page: here().position().page - 1,
      x: here().position().x / 1pt,
      y: here().position().y / 1pt,
      w: measure(body).width / 1pt,
      h: measure(body).height / 1pt,
    )) <region>
  ]
  #body
]

// Indigo filled nav bar: < Prev | Home | Next >. Inert cells are dimmed.
#let nav-bar() = context {{
  let sid = section-state.at(here())
  let cur = if sid == "" {{ none }} else {{ order.position(s => s == sid) }}
  let prev = if cur == none or cur == 0 {{ none }} else {{ order.at(cur - 1) }}
  let next = if cur == none or cur + 1 >= order.len() {{ none }} else {{ order.at(cur + 1) }}
  let cell(txt, target) = align(center + horizon, if target == none {{
    text(font: "Hanken Grotesk", size: 8pt, weight: "semibold", tracking: 0.04em,
      fill: navfg.transparentize(55%), txt)
  }} else {{
    link(label("art-" + target),
      text(font: "Hanken Grotesk", size: 8pt, weight: "semibold", tracking: 0.04em,
        fill: navfg, txt))
  }})
  let home = align(center + horizon, link(<index-home>,
    text(font: "Hanken Grotesk", size: 8pt, weight: "semibold", tracking: 0.04em,
      fill: navfg, "Home")))
  // align(horizon, …) centres the row vertically in the fixed-height bar so the
  // labels aren't clipped at the bottom edge.
  block(width: 100%, height: 24pt, fill: navbg, inset: (x: 10pt),
    align(horizon, grid(columns: (1fr, 1fr, 1fr),
      cell([< Prev], prev), home, cell([Next >], next))))
}}

// Action band: INBOX | ARCHIVE | LATER | DELETE, indigo on paper. Each cell is a
// full outlined box (adjacent boxes share edges → a continuous bordered row) and
// a recoverable region. Article pages only.
#let action-band() = context {{
  let sid = section-state.at(here())
  if sid == "" {{ none }} else {{
    let cell(lbl, w) = box(width: w, height: 28pt, stroke: 0.8pt + faint-col, inset: 4pt,
      align(center + horizon,
        text(font: "Hanken Grotesk", size: 9pt, weight: "semibold", tracking: 0.12em,
          fill: heading-col, lbl)))
    grid(columns: (1fr,) * action-labels.len(),
      ..action-labels.map(lbl => layout(size =>
        region("action-" + lbl, cell(lbl, size.width)))))
  }}
}}

// Per-page chrome: nav bar + action band. Article pages only — on the index
// (no active section) the top margin is reserved (device toolbar) but left blank.
#let page-header() = context {{
  if section-state.at(here()) == "" {{ none }} else {{
    block(width: 100%, height: 112pt)[
      #v(37pt, weak: false)
      #nav-bar()
      #v(20pt, weak: false)
      #action-band()
    ]
  }}
}}

// One article: mark the section, force a page, then headline/byline/rule/body.
// Emits a <region> metadata recording this article's first page (for page_range
// recovery) and attaches the article link target to the headline.
#let article(id, title-text, byline-text, body) = {{
  section-state.update(id)
  pagebreak(weak: true)
  context [#metadata((name: "art-" + id, page: here().position().page - 1)) <region>]
  [#block(below: 6pt, text(font: "Lora", weight: "semibold", size: 16pt,
    fill: heading-col, hyphenate: false, title-text)) #label("art-" + id)]
  block(below: 8pt, text(font: "Hanken Grotesk", weight: "semibold", size: 9pt,
    fill: byline-col, byline-text))
  block(below: 8pt, line(length: 100%, stroke: 0.5pt + rule-col))
  body
}}

#set page(
  width: {w}pt, height: {h}pt,
  margin: (top: 120pt, right: 16pt, bottom: 30pt, left: 16pt),
  fill: paper,
  header-ascent: 8pt,
  header: page-header(),
  footer: none,
)
#set text(font: "Lora", size: 9.5pt, fill: ink, lang: "en", hyphenate: false)
#set par(leading: 0.5em, spacing: 0.65em, justify: false)
"#
    ));

    // ---- Index page -------------------------------------------------------
    s.push_str(&build_index(collection, rows));

    // ---- Articles ---------------------------------------------------------
    for a in articles {
        s.push_str(&format!(
            "#article(\"{id}\", [{title}], [{byline}])[\n{body}\n]\n",
            id = esc_str(&a.anchor),
            title = esc_markup(&a.title),
            byline = esc_markup(&a.byline),
            body = a.body,
        ));
    }

    s
}

fn build_index(collection: &str, rows: &[Row]) -> String {
    let mut s = String::new();
    // Masthead. The <index-home> label anchors the nav bar's Home link. The
    // subhead/reading-times are lowercase to match the deployed look: the old
    // CSS asked for text-transform:uppercase but fulgur/Blitz ignored it, so the
    // real output was never uppercased.
    s.push_str(&format!(
        "#block(above: 14pt, below: 2pt, text(font: \"Fraunces\", weight: \"semibold\", \
         size: 25pt, fill: heading-col, [{title}])) #label(\"index-home\")\n\
         #block(below: 12pt, text(font: \"Hanken Grotesk\", size: 7.5pt, tracking: 0.12em, \
         fill: muted, [{count} articles · newest first]))\n",
        title = esc_markup(collection),
        count = rows.len(),
    ));

    for r in rows {
        // Row: tomato number | serif title — author | muted reading-time,
        // hairline underline, tapping the row jumps to the article.
        let title_line = format!("{} — {}", esc_markup(&r.title), esc_markup(&r.author));
        s.push_str(&format!(
            "#link(label(\"art-{anchor}\"))[#block(below: 0pt, inset: (y: 4pt), \
             stroke: (bottom: 0.5pt + rule-col))[#grid(columns: (14pt, 1fr, auto), \
             column-gutter: 8pt, align: (left + top, left + top, right + top),\n\
             text(font: \"Lora\", weight: \"semibold\", size: 9pt, fill: accent, \"{num}\"),\n\
             text(font: \"Lora\", size: 9.5pt, fill: ink, [{title_line}]),\n\
             text(font: \"Hanken Grotesk\", size: 7.5pt, fill: muted, \"{rt}\"))]]\n",
            anchor = esc_str(&r.anchor),
            num = esc_str(&r.num),
            rt = esc_str(&r.reading_time),
        ));
    }
    s
}
