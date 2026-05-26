// One section. Sets the `inkapp.section` state to `id` and forces a weak page
// break, then lays out `body`. A per-page header that reads
// `state("inkapp.section", "").at(here().position())` (re-declare the state in
// the consumer — Typst state is keyed by name string, so a fresh handle works)
// will see this id on every page covered by this section.
#let section-state = state("inkapp.section", "")

#let section(id, body) = {
  // Update the state BEFORE the pagebreak so the per-page header on the NEXT
  // page can read the correct section id via `section-state.at(here())`. The
  // header is placed at the top of the page before any body content, so
  // updating after the break would leave the header one step behind.
  section-state.update(id)
  // Force a fresh page (weak: no blank pages for the very first section).
  pagebreak(weak: true)
  body
}
