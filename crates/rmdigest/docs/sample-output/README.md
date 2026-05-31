# rmdigest sample output

Rendered from the real `tests/fixtures/stamped-labels.rmdoc` capture via
`cargo run --example full_sample` (writes to `/tmp/rmdigest-sample/`).

- `digest-cover.png` — the standalone **Digest** PDF cover (Newsreader title,
  tracked "DIGEST" kicker, italic byline, counts line).
- `digest-highlights.png` — the Digest content page: each highlight as a
  Newsreader block quote with a colored `PAGE n` kicker + left bar tinted by the
  reMarkable pen color (`Highlight` → highlighter yellow).
- `annotated-book-page.png` — a book page from the **Annotated** PDF with the
  highlighter ink flattened on as a translucent yellow overlay, registered over
  the source text (vector text preserved).

Note: this fixture was highlighted across two sessions, so its 4 strokes yield
duplicate entries ("ARCHIVE" ×2, the body sentence ×2). Real books rarely produce
exact duplicates; collapsing identical (page, text) highlights is a possible
future polish.
