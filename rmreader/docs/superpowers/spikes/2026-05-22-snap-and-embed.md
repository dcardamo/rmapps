# Spike: snap-to-text storage + embedded-manifest round trip

**Date:** 2026-05-22
**Device:** reMarkable Paper Pro, software 3.x.
**Method:** Generated a 1-page PDF (`examples/spike_stamp.rs`) with the four action
labels + a known body sentence stamped as real text, and an embedded manifest under
the Catalog key `RMReaderManifest`. Uploaded via `rmapi put`, highlighted on-device
with the **highlighter + snap-to-text ON** (verified by the user, two sessions),
pulled back with `rmapi get`, and inspected the `.rm` (raw + via rmscene, the
reference parser) and the source PDF.

## Result 1 — snap-to-text does NOT yield recoverable text on this device ❌

Even with snap-to-text on, the Paper Pro stored the highlights as **highlighter ink
`Line` items (geometry), not `GlyphRange` text**. Confirmed three ways:

- `strings`/`grep` of the page `.rm`: none of the highlighted words appear.
- rmscene (current, the library every highlight tool uses) parses the file as **4
  `SceneLineItemBlock` (tool `HIGHLIGHTER_2`, color `HIGHLIGHT`), zero
  `SceneGlyphItemBlock`**. It also warns *"Some data has not been read … newer
  format than this reader supports"* — the Paper Pro writes a format slightly newer
  than rmscene fully reads; the `Line` geometry comes through correctly.
- The stroke coordinates are clean and meaningful (device space, 1404×1872, X
  centered, y top-down):
  - label `ARCHIVE`: `y≈154`, `x∈[-588,-339]` (top band)
  - body sentence: `y≈313`, `x∈[-786,402]`

This **invalidates** the spec's "Confirmed external facts" assumption that snap-to-text
yields a `GlyphRange` with the verbatim text. (That holds for older devices / EPUBs,
not this Paper Pro + PDF.)

## Result 2 — embedded manifest survives the round trip ✅

The downloaded source PDF inside the `.rmdoc` is **byte-for-byte identical** to the
uploaded one (`cmp` clean), with the `RMReaderManifest` Catalog key intact. The
PDF-as-single-source-of-truth design is sound: reMarkable does not rewrite the source
PDF.

## Decision — pivot read-back to stroke GEOMETRY (user UX unchanged)

The interaction the user performs is unchanged (highlight the `ARCHIVE` label; highlight
body text). Only the read-back changes from text-matching to geometry:

- **Actions:** map each highlighter stroke device→PDF coords; a stroke hitting a known
  stamped action-label rect → that action.
- **Content highlights:** we generate the PDF, so we own its text layer. Intersect a
  body stroke's region with the PDF's word boxes (poppler `pdftotext -bbox` on the
  downloaded source PDF) to reconstruct the highlighted text → push to Readwise. No
  OCR, no device-stored text needed.

New machinery required: a device→PDF coordinate transform (validated against this
fixture's ground truth — the `ARCHIVE` stroke must land in the stamped `ARCHIVE` rect)
and PDF text-layer word extraction. `rmfiles` targets highlighter `Line` strokes
(not `GlyphRange`).

Fixture committed at `rmfiles/tests/fixtures/stamped-labels.rmdoc` (+ `.expected.json`).
