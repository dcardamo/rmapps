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

## Root cause: what `--content-only` actually does (from rmapi source)

`shell/put.go` → `ApiCtx.ReplaceDocumentFile` (`api/sync15/apictx.go`): it finds the
document's `.pdf` file entry, uploads the **new PDF blob**, updates that entry's
hash/size, rehashes the doc + tree, and re-uploads the doc index. **It never touches
`.content` (page order / inserted-page positions) or the `.rm` annotation files** —
those keep their exact hashes. So content-only is purely a PDF-blob swap at the
storage level.

Consequences (now provable, not just observed):
- Annotations (`.rm`, keyed by page-UUID) and page order / user-inserted pages
  (`.content`) are **preserved byte-for-byte**; only the PDF backing changes.
- New **trailing** PDF pages surface on the device (it shows PDF pages beyond what
  `.content` references); existing inserted pages stay where `.content` put them.
- A user-inserted page therefore **cannot be moved by our push** (we don't write
  `.content`). The "after page 3 → after page 4" observation was a mis-insert, not
  drift.

**Design invariant (precise):** we may freely change *trailing* PDF pages
(append/grow/shrink the agenda); we must never change the *meaning of leading PDF
page indices* the device references (no middle-insert/reorder in our generated PDF).

## Conflicts / sync ordering

`ReplaceDocumentFile` runs inside `Sync(...)`, which fetches the current cloud hash
tree and handles generation conflicts (the "remote tree has changed, refresh"
messages). rmapi rewrites only the PDF blob — a different file from `.rm`/`.content` —
so it won't clobber device-side ink or inserted pages **provided the device synced
those changes to the cloud first** (rmapi can't see un-synced device edits). Rule:
**sync device → push → sync device.** No manual download-first needed on our side.

## Not yet tested (edges)

- **Shrinking** (fewer trailing pages) — expected fine; the agenda section will
  shrink when events are removed; verify when convenient.
- **Middle-insert** — expected to break (shifts existing page indices); the design
  deliberately never does this (all volatile content is appended at the end).

## Artifacts / cleanup

- `examples/spike_pages_links.rs` (kept as a re-runnable record).
- Cloud throwaway: `/rmbujo-pagespike/spike` — delete when done:
  `rmapi -ni rm /rmbujo-pagespike/spike`.
