#import "/inkapp/region.typ": region

// One section. Sets the `inkapp.section` state to `id` and forces a weak page
// break, then lays out `body`. A per-page header that reads
// `state("inkapp.section").at(here().position())` will see this id on every page
// covered by this section.
#let section-state = state("inkapp.section", "")

#let section(id, body) = {
  // Force a fresh page (weak: no blank pages for the very first section).
  pagebreak(weak: true)
  // Update the section state — observable from any later read on this & subsequent pages.
  section-state.update(id)
  body
}
