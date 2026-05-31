//! Build a standalone "Digest" PDF from a set of [`Mark`]s.
//!
//! The output is a typographic document sized for the reMarkable Paper Pro Move
//! (107×191 mm). It opens with a cover page (title, author, kicker, counts,
//! hairline rule) followed by one block per mark:
//!
//! - `Highlight` → colored PAGE kicker + curly-quoted block quote in Newsreader
//!   with a thin colored underline rule.
//! - `Note` / `InsertedPage` → PAGE·NOTE kicker + framed ink image.
//!
//! Every kicker is emitted via a level-2 `#heading` so the reMarkable nav panel
//! populates an outline (bookmark list) for quick navigation.

use crate::extract::Mark;
use crate::theme::pen_rgb;

/// Metadata shown on the digest cover page.
pub struct DigestMeta {
    pub title: String,
    pub author: String,
    pub n_highlights: usize,
    pub n_notes: usize,
    pub date_range: String,
}

// ---------------------------------------------------------------------------
// Escaping helpers (identical semantics to rmreader typst_doc)
// ---------------------------------------------------------------------------

/// Escape a string for inclusion inside a Typst double-quoted string literal.
fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Escape arbitrary text for Typst *markup* content mode. Characters that carry
/// special meaning in Typst markup are prefixed with a backslash.
fn esc_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | '#' | '$' | '*' | '_' | '`' | '<' | '>' | '@' | '=' | '~' | '"' | '\'' | '['
            | ']' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Public builder
// ---------------------------------------------------------------------------

/// Build typst source + image assets for the standalone Digest PDF.
///
/// Returns `(typst_source, image_assets)` where every entry in `image_assets`
/// is `("/assets/img-N.png", png_bytes)` matching the path referenced in the
/// typst source.
pub fn build_digest(meta: &DigestMeta, marks: &[Mark]) -> (String, Vec<(String, Vec<u8>)>) {
    let mut assets: Vec<(String, Vec<u8>)> = Vec::new();
    let mut s = String::new();

    // ------------------------------------------------------------------
    // Preamble: page geometry, text defaults, helper functions, show rules
    // ------------------------------------------------------------------
    // Notes on design choices:
    //   - `#set heading(outlined: true)` ensures level-2 headings land in the
    //     PDF /Outlines (bookmarks), which the reMarkable nav panel reads.
    //   - The show rule for `heading.where(level: 2)` re-styles the outline
    //     entry as a small Hanken kicker so it looks editorial, not like a
    //     chapter header.
    //   - Level-1 heading is used only for the invisible "cover" bookmark.
    //   - `bookmarked: true` on headings is the Typst 0.13+ opt-in for
    //     outline population; `outlined: true` is the parameter name in
    //     Typst 0.14. We pass it explicitly to be safe.
    s.push_str(&format!(
        r##"// ─── rmdigest Digest PDF ───────────────────────────────────────────────────
#set document(title: "{title}", author: "{author}")
#set page(
  width: 107mm, height: 191mm,
  margin: (x: 8mm, y: 10mm),
  fill: rgb(250, 249, 246),
)
#set text(font: "Newsreader", size: 11pt, fill: rgb(26, 26, 26), lang: "en", hyphenate: false)
#set par(leading: 0.65em, spacing: 0.8em, justify: false)
#set heading(outlined: true)

// Kicker helper: tracked uppercase Hanken Grotesk in the given colour.
#let kick(t, c) = text(
  font: "Hanken Grotesk", size: 7pt,
  weight: "semibold", tracking: 2pt, fill: c,
)[#upper(t)]

// Re-style level-2 headings (highlight kickers) as inline kicker text with spacing.
// The heading registers in the PDF outline; only the visual rendering changes.
#show heading.where(level: 2): it => block(
  above: 24pt, below: 6pt,
  width: 100%,
)[#it.body]

// Level-3 headings are used for note kickers.
#show heading.where(level: 3): it => block(
  above: 20pt, below: 5pt,
  width: 100%,
)[#it.body]

"##,
        title = esc(&meta.title),
        author = esc(&meta.author),
    ));

    // ------------------------------------------------------------------
    // Cover / index page
    // ------------------------------------------------------------------
    s.push_str(&format!(
        r##"// ─── Cover page ──────────────────────────────────────────────────────────────
#align(center + horizon)[
  #text(font: "Hanken Grotesk", size: 7.5pt, weight: "semibold",
        tracking: 3pt, fill: rgb(120, 120, 120))[DIGEST]
  #v(14pt)
  #text(font: "Newsreader", size: 22pt, weight: "semibold",
        fill: rgb(26, 26, 26), hyphenate: false)[{title}]
  #v(6pt)
  #text(font: "Newsreader", size: 12pt, style: "italic",
        fill: rgb(80, 80, 80))[{author}]
  #v(14pt)
  #line(length: 38%, stroke: 0.5pt + rgb(180, 180, 180))
  #v(10pt)
  #text(font: "Hanken Grotesk", size: 8pt, fill: rgb(100, 100, 100))[{nh} {hl_word} · {nn} {note_word} --- {range}]
]
#pagebreak()
"##,
        title = esc_markup(&meta.title),
        author = esc_markup(&meta.author),
        nh = meta.n_highlights,
        hl_word = if meta.n_highlights == 1 { "highlight" } else { "highlights" },
        nn = meta.n_notes,
        note_word = if meta.n_notes == 1 { "note" } else { "notes" },
        range = esc_markup(&meta.date_range),
    ));

    // ------------------------------------------------------------------
    // One block per mark
    // ------------------------------------------------------------------
    let mut img_idx = 0usize;

    for m in marks {
        match m {
            Mark::Highlight { page, text, color } => {
                let (r, g, b) = pen_rgb(*color);
                // Darken bright highlight colors so the kicker text is legible
                // on the warm paper background.
                let (kr, kg, kb) = darken_for_text(r, g, b);
                // Curly quotes as raw chars (Rust \u{} escapes clash with format braces).
                let lq = '\u{201C}';
                let rq = '\u{201D}';
                // The block-quote uses a colored left bar (3pt thick) for visual
                // identity, with the quote indented inside it. Below the block a
                // short separator line (25% width) marks the end of the entry.
                s.push_str(&format!(
                    "// Highlight — page {pg}\n\
                     #heading(level: 2, outlined: true)[#kick(\"page {pg}\", rgb({kr},{kg},{kb}))]\n\
                     #block(\n\
                       width: 100%,\n\
                       inset: (left: 11pt, right: 4pt, top: 4pt, bottom: 4pt),\n\
                       stroke: (left: 3pt + rgb({r},{g},{b}).lighten(20%)),\n\
                     )[#par(leading: 0.7em)[#text(font: \"Newsreader\", size: 12pt,\n\
                         fill: rgb(26, 26, 26))[{lq}{text}{rq}]]]\n\
                     #v(10pt)\n\
                     #line(length: 22%, stroke: 0.4pt + rgb(200, 200, 200))\n\
                     #v(2pt)\n",
                    pg = page + 1,
                    text = esc_markup(text),
                    r = r, g = g, b = b,
                    kr = kr, kg = kg, kb = kb,
                ));
            }

            Mark::Note { page, png }
            | Mark::InsertedPage {
                after_page: page,
                png,
            } => {
                let is_inserted = matches!(m, Mark::InsertedPage { .. });
                let kicker_label = if is_inserted {
                    format!("page {} · inserted", page + 1)
                } else {
                    format!("page {} · note", page + 1)
                };
                let asset_path = format!("/assets/img-{img_idx}.png");
                assets.push((asset_path.clone(), png.clone()));
                img_idx += 1;

                s.push_str(&format!(
                    r##"// Note — page {pg}
#heading(level: 3, outlined: true)[#kick("{kicker}", rgb(110, 110, 110))]
#block(
  stroke: 0.5pt + rgb(210, 210, 210),
  inset: (x: 5pt, y: 5pt),
  radius: 2pt,
  width: 100%,
)[#image("{path}", width: 100%)]
#v(6pt)
"##,
                    pg = page + 1,
                    kicker = esc(&kicker_label),
                    path = asset_path,
                ));
            }
        }
    }

    (s, assets)
}

/// Darken a highlight color so the kicker text is legible on warm paper.
/// Very light/bright colors (high sum of channels) are darkened to ~40% value.
fn darken_for_text(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let brightness = (r as u32 + g as u32 + b as u32) / 3;
    if brightness > 180 {
        // Bright color (e.g. yellow) — darken significantly for text legibility.
        let scale = 0.55_f32;
        (
            (r as f32 * scale) as u8,
            (g as f32 * scale) as u8,
            (b as f32 * scale) as u8,
        )
    } else if brightness > 120 {
        // Medium brightness — darken slightly.
        let scale = 0.75_f32;
        (
            (r as f32 * scale) as u8,
            (g as f32 * scale) as u8,
            (b as f32 * scale) as u8,
        )
    } else {
        // Already dark enough.
        (r, g, b)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::compile;
    use rmfiles::{Pen, PenColor, Point, Stroke};

    /// Build a minimal valid PNG for test note images (two-point pen stroke).
    fn tiny_png() -> Vec<u8> {
        let strokes = [Stroke {
            tool: Pen::Ballpoint1,
            color: PenColor::Black,
            points: vec![
                Point {
                    x: 10.0,
                    y: 10.0,
                    speed: None,
                    direction: None,
                    width: Some(3.0),
                    pressure: None,
                },
                Point {
                    x: 50.0,
                    y: 30.0,
                    speed: None,
                    direction: None,
                    width: Some(3.0),
                    pressure: None,
                },
            ],
        }];
        let refs: Vec<&Stroke> = strokes.iter().collect();
        crate::ink::render_strokes(
            &refs,
            &crate::ink::InkOpts {
                background: crate::ink::Background::White,
                ..Default::default()
            },
        )
        .expect("render tiny test stroke")
    }

    #[test]
    fn digest_smoke_valid_pdf_and_outlines() {
        let meta = DigestMeta {
            title: "Test Book".into(),
            author: "Test Author".into(),
            n_highlights: 1,
            n_notes: 1,
            date_range: "May 2026".into(),
        };
        let png = tiny_png();
        let marks = vec![
            Mark::Highlight {
                page: 0,
                text: "The quick brown fox jumps over the lazy dog.".into(),
                color: PenColor::Yellow,
            },
            Mark::Note { page: 1, png },
        ];

        let (src, assets) = build_digest(&meta, &marks);
        let pdf_bytes = compile(&src, &assets).expect("compile should succeed");

        // Must be a valid lopdf document.
        let doc = lopdf::Document::load_mem(&pdf_bytes).expect("valid PDF");

        // Cover page + at least 1 content page = ≥2 pages.
        assert!(
            doc.get_pages().len() >= 2,
            "expected ≥2 pages, got {}",
            doc.get_pages().len()
        );

        // /Outlines must exist in the catalog — reMarkable nav panel reads this.
        let catalog = doc
            .trailer
            .get(b"Root")
            .and_then(|o| o.as_reference())
            .and_then(|id| doc.get_dictionary(id))
            .expect("catalog must be a dictionary");

        assert!(
            catalog.get(b"Outlines").is_ok(),
            "PDF catalog must contain /Outlines for reMarkable nav panel"
        );
    }

    #[test]
    fn digest_cover_stats_appear_in_source() {
        let meta = DigestMeta {
            title: "My Highlights".into(),
            author: "Jane Doe".into(),
            n_highlights: 5,
            n_notes: 2,
            date_range: "Jan–Mar 2026".into(),
        };
        let (src, _) = build_digest(&meta, &[]);
        assert!(src.contains("5 highlights"), "highlights count missing");
        assert!(src.contains("2 notes"), "notes count missing");
        assert!(src.contains("My Highlights"), "title missing");
        assert!(src.contains("Jane Doe"), "author missing");
    }

    #[test]
    fn digest_image_assets_indexed_correctly() {
        let png = tiny_png();
        let meta = DigestMeta {
            title: "X".into(),
            author: "Y".into(),
            n_highlights: 0,
            n_notes: 2,
            date_range: "2026".into(),
        };
        let marks = vec![
            Mark::Note {
                page: 0,
                png: png.clone(),
            },
            Mark::InsertedPage {
                after_page: 1,
                png: png.clone(),
            },
        ];
        let (src, assets) = build_digest(&meta, &marks);
        assert_eq!(assets.len(), 2);
        assert_eq!(assets[0].0, "/assets/img-0.png");
        assert_eq!(assets[1].0, "/assets/img-1.png");
        assert!(src.contains("/assets/img-0.png"));
        assert!(src.contains("/assets/img-1.png"));
    }

    /// Adversarial injection test: highlight text containing every typst markup-special
    /// character must compile to a valid PDF without errors or mis-renders.
    #[test]
    fn digest_typst_injection_adversarial_text_compiles() {
        // This string exercises every character that esc_markup escapes:
        // #  [ ]  *  _  `  <  >  @  =  ~  "  '  $  \
        let adversarial = r#"C# tips: [see *note*] a_b `code` <tag> @ref =heading ~tilde~ "quote" 'apos' $x$ \ end"#;
        let meta = DigestMeta {
            title: r#"C# Book [v2] *Best* _Ever_ $math$ @ref <end>"#.into(),
            author: r#"O'Brien & "Smith" \ escape"#.into(),
            n_highlights: 1,
            n_notes: 0,
            date_range: r#"Jan~Mar [2026] #special"#.into(),
        };
        let marks = vec![Mark::Highlight {
            page: 0,
            text: adversarial.into(),
            color: PenColor::Yellow,
        }];

        let (src, assets) = build_digest(&meta, &marks);
        // The source must compile to a valid PDF — any escaping failure causes a
        // typst compile error here.
        let pdf_bytes = compile(&src, &assets)
            .expect("adversarial text must compile: esc_markup covers all typst-special chars");
        let doc = lopdf::Document::load_mem(&pdf_bytes).expect("valid PDF");
        assert!(doc.get_pages().len() >= 2, "expected ≥2 pages");
    }
}
