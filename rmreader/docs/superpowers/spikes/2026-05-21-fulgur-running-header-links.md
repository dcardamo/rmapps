# Spike: fulgur running-header link support

**Date:** 2026-05-21
**Question:** Does fulgur emit clickable PDF `/Link` annotations for an `<a>` element
placed inside a GCPM running element (`position: running(nav)` via `@top-center { content:
element(nav) }`), on pages that are NOT the first?

## Method

`tests/spike_running_header.rs` (now deleted) rendered a 2-page A4 PDF using the default
`Engine::builder().build()` with inline `<style>` GCPM CSS placing a `<div class="nav"><a
href="#home">Home</a></div>` in `@top-center`. A 900pt spacer forced the body to overflow
onto a second page. After rendering, `lopdf` walked every page's `/Annots` array and
counted annotations with `/Subtype /Link`.

## Result

```
SPIKE RESULT: link annotations found on 0 page(s) (0 total link annotations)
PDF has 2 page(s)
```

Raw `strings` inspection of the PDF confirmed no `/Link`, `/Annots`, `/URI`, or `/Dest`
entries anywhere in the file. The `<a>` inside the running element generates zero link
annotations — not on any page, including page 1.

Note: fulgur's own `link_integration.rs` confirms that links placed in the normal body flow
DO emit `/Link` annotations through the GCPM render path. The gap is specific to links
inside running elements: fulgur copies the element's visual content into each margin box
but does not wire up the annotation.

## Interpretation

The per-page running-header nav design is **not viable**. Even though the visual text
"Home" appears in the margin box on every page, no clickable annotation is attached.
The running-header mechanism is visual-only for links.

## Decision for Task 6

Use **per-section nav block + native bookmark outline**:

- Place a small nav block (`<div class="nav">`) at the top of each major section (index,
  each article card, each article body). These are in the normal body flow so their `<a>`
  links produce real PDF annotations.
- Enable `bookmarks(true)` / bookmark CSS (`bookmark-level`, `bookmark-label`) on section
  headings to generate the device-native PDF outline (the reMarkable sidebar TOC).

This fallback is architecturally sound: section-level nav covers jump-to-top and
cross-section links, while the native outline gives page-level navigation without
requiring annotations on every page.
