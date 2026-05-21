# Spike: trailing page growth + tappable links under content-only refresh

**Date:** 2026-05-21
**Conclusion: GREEN on both.** On the Paper Pro Move + official v4 cloud:
1. **Appending pages at the END of a PDF survives `rmapi put --content-only`** — the
   new pages appear, and annotations on the existing (leading) pages are preserved
   on their original pages.
2. **fulgur-emitted internal links (`<a href="#id">` → `id="id"`) are tappable on
   the device** and keep working after a content-only refresh.

This **overturns the pessimistic assumption** in
`2026-05-21-rmapi-lifecycle-spike.md` (which only verified *same* page count and
flagged page-count change as unsafe/unverified).

## Why it matters

It validates the "embrace change" calendar design: keep the pages the user writes on
(monthly view, per-day daily log) as a **fixed leading section**, and put the
volatile, event-driven **calendar/agenda pages at the end**, free to grow/shrink on
each `--refresh-feeds` + content-only re-sync — without ever disturbing handwriting.
Internal links make monthly↔daily↔agenda navigation viable.

## Method

`examples/spike_pages_links.rs` renders two PDFs via rmbujo's `render_pdf`, both
named `spike.pdf` (so a content-only push targets the same cloud doc by name):
- **v1:** 5 pages; page 1 has `<a href="#target">` → `id="target"` on page 4.
- **v2:** 7 pages — identical to v1 with **two pages appended at the end**.

Steps:
1. `rmapi -ni put spike.pdf /rmbujo-pagespike` (v1).
2. On device: tap the page-1 link → **jumped to page 4** ✅. Annotated page 2, synced.
3. `rmapi -ni put --content-only <v2>/spike.pdf /rmbujo-pagespike`.
4. On device, after sync:
   - Pages 6 & 7 **present** ✅ (trailing count grew 5 → 7).
   - Page-2 annotation **intact, still on page 2** ✅.
   - Page-1 → page-4 link **still works** ✅.

PDF-level checks: v1 contains one `/Link` annotation with an `/XYZ` destination
(fulgur `src/link.rs` → krilla `LinkAnnotation` + `XyzDestination`).

## Not yet tested (edges)

- **Shrinking** (fewer trailing pages) — expected fine; the agenda section will
  shrink when events are removed; verify when convenient.
- **Middle-insert** — expected to break (shifts existing page indices); the design
  deliberately never does this (all volatile content is appended at the end).

## Artifacts / cleanup

- `examples/spike_pages_links.rs` (kept as a re-runnable record).
- Cloud throwaway: `/rmbujo-pagespike/spike` — delete when done:
  `rmapi -ni rm /rmbujo-pagespike/spike`.
