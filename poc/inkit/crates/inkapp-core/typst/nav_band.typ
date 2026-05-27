#import "/inkapp/section.typ": section-state

// A three-cell page-header strip: `< Prev | Home | Next >`. Each cell is a
// `#link` annotation to a Typst label.
//
//   * Home links to `<index-home>` — the Index component (when used) emits
//     this anchor at the top of its first row. Always rendered as a clickable
//     link; falls back to plain text when no `<index-home>` label exists in
//     the document (Typst silently drops dangling links).
//
//   * Prev / Next are computed at render-time from `order` (the ordered list
//     of section ids the app builds) and the current `inkapp.section` state.
//     When there's no prev/next (first/last article, or on an Index page
//     where the section state is ""), the cell renders inert (muted, no link).
//
// `order` is a `(string, ...)` array passed in from the Rust side. Bake it once
// per Document; the cost is small even for hundreds of sections.
#let nav-band(order) = context {
  let sid = section-state.at(here())

  // Index of the current section in `order`, or `none` on Index/cover pages.
  let cur = if sid == "" { none } else { order.position(s => s == sid) }

  let prev = if cur == none or cur == 0 { none } else { order.at(cur - 1) }
  let next = if cur == none or cur + 1 >= order.len() { none } else { order.at(cur + 1) }

  // `txt` is the cell's display string. Don't shadow the Typst built-in
  // `label(...)` function — we need it for the link target below.
  let nav-cell(txt, target) = box(
    width: 100%,
    height: 18pt,
    inset: 4pt,
    align(center + horizon, if target == none {
      // Inert: muted, no link annotation.
      text(size: 8pt, weight: "medium", tracking: 0.1em, fill: luma(180), upper(txt))
    } else {
      // Live link to <art-{target}>.
      link(label("art-" + target),
        text(size: 8pt, weight: "medium", tracking: 0.1em, fill: luma(60), upper(txt)))
    }),
  )

  // Home: always a link to <index-home>. If the document has no such label
  // (e.g. an app that doesn't use Index), Typst drops the link silently —
  // the cell still renders as visible text via the underlying box.
  let home-cell = box(
    width: 100%,
    height: 18pt,
    inset: 4pt,
    align(center + horizon, link(<index-home>,
      text(size: 8pt, weight: "medium", tracking: 0.1em, fill: luma(60), upper("Home")))),
  )

  grid(
    columns: (1fr, 1fr, 1fr),
    column-gutter: 6pt,
    nav-cell("< Prev", prev),
    home-cell,
    nav-cell("Next >", next),
  )
}
