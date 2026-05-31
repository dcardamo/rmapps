# rmfiles test fixtures

Real reMarkable Paper Pro captures (committed) used as ground truth for the parser.

## `stamped-labels.rmdoc`

A `.rmdoc` (zip) pulled via `rmapi get` after annotating, on a Paper Pro (software
3.x, May 2026), a single-page PDF that carried:
- the four action labels `INBOX  ARCHIVE  LATER  DELETE` stamped as real text,
- the body sentence "The quick brown fox jumps over the lazy dog near the riverbank.",
- an embedded manifest under the PDF Catalog key `RMReaderManifest`.

On device, with the **highlighter + snap-to-text ON**, the word `ARCHIVE` and the body
sentence were highlighted (twice each across two sessions → 4 strokes total).

**Key finding (the spike):** even with snap-to-text on, the Paper Pro stored the
highlights as **highlighter ink `Line` items (geometry only), not `GlyphRange` text**.
The highlighted text is NOT in the bundle (confirmed with rmscene, the reference
parser: 4 `SceneLineItemBlock`, zero `SceneGlyphItemBlock`). The embedded manifest in
the source PDF survived the cloud round trip byte-for-byte. See
`rmreader/docs/superpowers/spikes/2026-05-22-snap-and-embed.md`.

Expectations for assertions are in `stamped-labels.expected.json`. rmscene also warns
"some data has not been read … newer format" — the Line geometry parses fine; the
unread bytes are extra fields we don't need.
