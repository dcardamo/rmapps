# Typst Readback Spike — Go/No-Go Findings

**Date:** 2026-05-22
**Spike crate:** `spikes/typst-readback` (Typst 0.14.2)

---

## Verdict

**GO on Typst.**

All automated must-bars passed. The make-or-break bar (region rects, Bar 1) matched exactly — zero delta on all four edges. No blocker was found. The only unverified item is on-device visual quality (Bar 4), which requires hardware and is marked pending. Typst is adopted as the render engine for inkapp.

---

## Per-Bar Results

| Bar | Description                                | Result                  |
|-----|--------------------------------------------|-------------------------|
| 1   | Region rects in PDF coords (make-or-break) | PASS                    |
| 2   | Tappable links                             | PASS                    |
| 3   | HTML → Typst converter                     | PASS (documented limits)|
| 4   | On-device visual quality                   | PENDING MANUAL CHECK    |
| 5   | Content-only leading-page invariant        | PASS (structure half)   |
| 6   | Full loop (stretch)                        | NOT ATTEMPTED           |

---

## Typst Integration

Typst is used as a Rust library — crates `typst`/`typst-pdf`/`typst-kit` 0.14.2, `comemo` 0.5.1. A minimal `World` implementation (in-memory main source + system fonts via `typst-kit`) compiles source to PDF bytes entirely in-process. No CLI subprocess, no headless browser.

---

## Bar 1 — Region Rects in PDF Coords (MAKE-OR-BREAK): PASS

A Typst doc emits labelled region geometry via `metadata` + a `<region>` label. Recovery path from the compiled `PagedDocument`:

1. `Selector::Label(Label::new(PicoStr::intern("region")))` → `document.introspector.query(&selector)`
2. `elem.to_packed::<MetadataElem>()` → `packed.value` (a Typst `Value` implementing `serde::Serialize`)
3. `serde_json::to_value` → `serde_json::from_value::<TypstRegion>`

The recovered rect for the test region matched the expected PDF-coordinate rect with **0.0 pt delta on all four edges**. Cross-checked by rendering to PNG (pdftoppm) and confirming a dark border pixel falls inside the recovered rect.

**Coordinate conversion:** Typst uses top-left origin (y down); PDF user space uses bottom-left origin (y up):

```
y0 = page_h - (y + h)
y1 = page_h - y
```

---

## Bar 2 — Tappable Links: PASS

- Internal links emit as a `/Link` annotation with a named `/Dest` (e.g. `(p2)`) — not a GoTo action dict.
- External links emit as a `/Link` with an `/A` action dict `{ S = /URI, URI = "https://example.com" }`.

Both verified via `lopdf`.

---

## Bar 3 — HTML → Typst: PASS (with documented limits)

A `scraper`-based DOM walk converts a representative article (headings, paragraphs, bold/italic, links, flat lists) to Typst markup that compiles. Extracted PDF text contained the expected strings.

**Lossy/unsupported elements (silently dropped or flattened):**

| Element       | Behaviour                                         |
|---------------|---------------------------------------------------|
| Images        | Silently dropped                                  |
| Tables        | Flattened to inline text                          |
| Nested lists  | Flattened                                         |
| Ordered lists | Rendered unordered (no numbering)                 |
| `code`/`pre`  | Plain text                                        |
| Blockquotes   | Plain paragraph                                   |

Workable for CMS article bodies using the common subset; not suitable for general web HTML.

**Robustness note for productionising:** `href` and link-text are not escaped inside `#link("…")[…]`. An odd URL or label could break compilation. Fix before shipping.

---

## Bar 4 — On-Device Visual Quality: PENDING MANUAL CHECK

Not verified on hardware in this spike. An `#[ignore]`d `on_device` integration test compiles a sample doc and pushes it via `rmapi --content-only` for visual inspection. Run it with a paired tablet to complete this bar:

```bash
cargo test -p typst-readback --test on_device -- --ignored --nocapture
```

---

## Bar 5 — Content-Only Leading-Page Invariant: PASS (structure half)

Re-rendering with an appended trailing page (`v1` = 2 pages, `v2` = 3 pages) left the leading pages' `MediaBox` entries byte-identical and the leading region's recovered rect unchanged (0.0 pt delta). Typst does not re-flow leading pages when trailing content is appended.

The device half — pushing v2 over v1 and confirming that existing ink is preserved — is a pending manual check, to be done alongside Bar 4.

---

## Bar 6 — Full Loop (Stretch): NOT ATTEMPTED

Requires a paired tablet. Deferred. The `.rm` read path is independently proven by the `rmfiles` crate.

---

## Coordinate Conversion Reference

All region rects must be converted from Typst space to PDF user space before being stored in the manifest or written as PDF annotations. Centralise this in one utility function:

```rust
/// Convert a Typst-space rect (origin top-left, y down) to PDF user space (origin bottom-left, y up).
pub fn typst_to_pdf_rect(x: f64, y: f64, w: f64, h: f64, page_h: f64) -> (f64, f64, f64, f64) {
    let x0 = x;
    let y0 = page_h - (y + h);
    let x1 = x + w;
    let y1 = page_h - y;
    (x0, y0, x1, y1)
}
```

---

## Recommendations for the Framework

- **Adopt Typst as the render engine.** All automated bars passed; the integration is clean and entirely in-process.
- **Build the manifest's region-rect extraction on the introspector + metadata pattern** proven here (`Selector::Label` → `MetadataElem` → serde round-trip). This is the stable API surface.
- **Centralise the Typst top-left → PDF bottom-left coordinate conversion** in one function (see above). Every other component (manifest builder, annotation writer, region hit-testing) must call it rather than inline the formula.
- **Treat HTML → Typst as a constrained converter for known content sources.** It is not a general web renderer. Enumerate the supported element set; add proper escaping of link `href` and label text before production.
- **Schedule the on-device manual checks** (Bars 4, 5-device, 6) when hardware is available. These are the remaining unknowns before committing to the full implementation.

### Notes from the final code review (carry into the framework extraction)

- **Centralise the compile path.** The spike's `compile_pdf` and `compile_with_regions` duplicate the `typst::compile` + `typst_pdf::pdf` + diagnostic-mapping logic. The framework should expose one `compile_to_document(world) -> Result<PagedDocument>` that both PDF export and region recovery consume.
- **Per-page height, not page-0 height.** `compile_with_regions` converts every region with the first page's height, and the recovered `TypstRegion.page` field is currently unused. This is correct only because the spike's pages are uniform. The framework's `typst_to_pdf_rect` must look up the height of `region.page` so multi-page documents convert correctly.
- **Font determinism.** The spike loads system fonts (`include_system_fonts(true)`), reproducible only because the flake pins dejavu/noto. Since content-only pushes lean on deterministic output (mechanics doc §11), the framework should embed/pin its fonts rather than search the host.
- **`World::font` should not panic.** The spike indexes `self.fonts[index]`; the framework's `World` should return `self.fonts.get(index).and_then(FontSlot::get)`.
