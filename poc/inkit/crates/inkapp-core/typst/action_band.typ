#import "/inkapp/region.typ": region
#import "/inkapp/section.typ": section-state

// One row of action cells; each cell is a labelled region named
// `action-{label}-{section_id}`. `labels` is an array of strings.
// `section_id` is read per-page from the inkapp.section state using
// `here().position()` so each page sees its own section (not the final value).
#let action-band(labels) = context {
  let sid = section-state.at(here())
  if sid == "" {
    // No section yet (e.g. the index page): render an inert band with no regions.
    block(height: 18pt, [])
  } else {
    grid(
      columns: (1fr,) * labels.len(),
      column-gutter: 6pt,
      ..labels.map(label => layout(size => region(
        "action-" + label + "-" + sid,
        box(width: size.width, height: 18pt, stroke: 0.5pt, inset: 3pt, align(center + horizon, text(size: 9pt, label)))
      ))),
    )
  }
}
