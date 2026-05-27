#import "/inkapp/region.typ": region
#import "/inkapp/section.typ": section-state

// One row of action cells, drawn on every page as the page header. Each cell
// is a labelled region named `action-{label}-{section_id}` (when a section
// is active) so a pen strike on a cell decodes to (label, section_id). On
// pages with no active section (e.g. an Index page before any Section opens),
// the band is drawn in the same visual frame but emits no regions — the
// labels are inert. This keeps the page header visually consistent across
// the entire document instead of disappearing on the Index pages.
//
// Cells are taller (28pt) than a single text line to read as real buttons
// on an e-ink device, with uppercase letterforms and tight letter-tracking.
#let action-band(labels) = context {
  let sid = section-state.at(here())
  let active = sid != ""
  // Cell takes an explicit width (not 100%) because `region(...)`'s
  // measure(body) ignores percentage widths — `size.width` from the surrounding
  // `layout` is the concrete laid-out cell width, so recovery yields a real
  // (non-zero-width) rect that a pen strike can be classified against.
  let cell(label, w) = box(
    width: w,
    height: 28pt,
    stroke: 0.6pt + luma(60),
    inset: 4pt,
    align(center + horizon, text(
      size: 9pt,
      weight: "medium",
      tracking: 0.15em,
      fill: if active { luma(34) } else { luma(150) },
      upper(label),
    )),
  )
  grid(
    columns: (1fr,) * labels.len(),
    column-gutter: 6pt,
    ..labels.map(label => layout(size => {
      if active {
        // Wrap in a region so a pen strike on the cell can be attributed
        // back to (label, section_id) by the Rust decode.
        region("action-" + label + "-" + sid, cell(label, size.width))
      } else {
        // Same visual cell on Index / cover pages, but no region: the band
        // is inert there (no article to act on). Decode sees nothing.
        cell(label, size.width)
      }
    })),
  )
}
