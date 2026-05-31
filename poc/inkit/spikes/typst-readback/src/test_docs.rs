/// Test documents for the content-only leading-page invariant.
///
/// `LEADING` is a fixed 2-page leading section whose second page ends with a
/// labelled region.  `v1()` returns exactly those leading pages; `v2()` returns
/// the same leading pages followed by an appended trailing page.  Because both
/// functions derive the leading content from the same `LEADING` constant, the
/// leading bytes are guaranteed identical — there is no duplication risk.
pub const LEADING: &str = r#"#set page(width: 200pt, height: 300pt, margin: 0pt)
#let region(name, w, h) = context {
  let pos = here().position()
  [#metadata((
    name: name,
    page: pos.page,
    x: pos.x.pt(),
    y: pos.y.pt(),
    w: w.pt(),
    h: h.pt(),
  )) <region>]
  box(stroke: 1pt, width: w, height: h)
}
Leading page one.
#pagebreak()
Leading page two.
#place(top + left, dx: 20pt, dy: 40pt, region("lead", 60pt, 24pt))"#;

/// v1: exactly the leading section, no trailing pages.
pub fn v1() -> String {
    LEADING.to_string()
}

/// v2: leading section with one appended trailing page.
pub fn v2() -> String {
    format!("{LEADING}\n#pagebreak()\nTrailing page\n")
}
