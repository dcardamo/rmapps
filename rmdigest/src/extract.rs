//! Turn a bundle's changed pages into an ordered list of digest marks.
use rmfiles::{Bundle, Stroke, TextHighlight};

use crate::textlayer::TextLayer;

/// One item destined for the digest.
pub enum Mark {
    /// A highlight (snap-to-text verbatim, or reconstructed from highlighter ink).
    Highlight {
        page: usize,
        text: String,
        /// The highlight color as RGB — the device's exact `color_rgba` when it
        /// recorded one, else the palette color for `color`.
        rgb: (u8, u8, u8),
    },
    /// A handwritten note (pen ink). Carries the raw strokes plus the backing
    /// source PDF page (`None` for an inserted blank page). The digest flattens
    /// the strokes onto the page region so circled/underlined content is included.
    Note {
        page: usize,
        source_page: Option<usize>,
        strokes: Vec<Stroke>,
    },
}

/// Classify one page's marks.
///
/// `reconstruct` maps a highlighter stroke's `PdfRect` to the underlying text
/// (injected so tests can stub it instead of requiring a real PDF).
pub(crate) fn page_marks(
    page_index: usize,
    source_page: Option<usize>,
    text_highlights: &[&TextHighlight],
    strokes: &[&Stroke],
    transform: &rmfiles::coords::Transform,
    reconstruct: &dyn Fn(usize, &rmfiles::coords::PdfRect) -> String,
) -> Vec<Mark> {
    let mut marks = Vec::new();
    let clone_strokes =
        |sel: &[&Stroke]| -> Vec<Stroke> { sel.iter().map(|s| (*s).clone()).collect() };

    // An inserted blank page has no backing PDF page — keep all strokes as an
    // ink-only note (nothing to flatten onto).
    let Some(src) = source_page else {
        if !strokes.is_empty() {
            marks.push(Mark::Note {
                page: page_index,
                source_page: None,
                strokes: clone_strokes(strokes),
            });
        }
        return marks;
    };

    // Snap-to-text highlights: verbatim text from the device. Prefer the exact
    // recorded color (`color_rgba`); fall back to the palette color.
    for h in text_highlights {
        let rgb = h
            .color_rgba
            .map(crate::theme::rgba_to_rgb)
            .unwrap_or_else(|| crate::theme::pen_rgb(h.color));
        marks.push(Mark::Highlight {
            page: page_index,
            text: h.text.clone(),
            rgb,
        });
    }

    // Highlighter ink strokes: reconstruct text from geometry (against the source
    // page). Pen strokes: collect and emit as one flattenable Note.
    let mut pen_strokes: Vec<&Stroke> = Vec::new();
    for &stroke in strokes {
        if stroke.is_highlighter() {
            // Expand the bbox vertically so the highlight band covers the text
            // line it overlays. Matches rmreader readback/mod.rs `detect()`.
            const MAX_HALF_H_PT: f64 = 6.0;
            let ink_half_h_pt = stroke
                .points
                .iter()
                .filter_map(|p| p.width)
                .fold(0.0f32, f32::max) as f64
                / (2.0 * transform.scale());
            let half_h = ink_half_h_pt.min(MAX_HALF_H_PT);

            if let Some(mut bbox) =
                transform.pdf_bbox(stroke.points.iter().map(|p| (p.x as f64, p.y as f64)))
            {
                bbox.y0 -= half_h;
                bbox.y1 += half_h;
                let text = reconstruct(src, &bbox);
                if !text.is_empty() {
                    marks.push(Mark::Highlight {
                        page: page_index,
                        text,
                        rgb: crate::theme::pen_rgb(stroke.color),
                    });
                }
            }
        } else {
            pen_strokes.push(stroke);
        }
    }

    if !pen_strokes.is_empty() {
        marks.push(Mark::Note {
            page: page_index,
            source_page: Some(src),
            strokes: clone_strokes(&pen_strokes),
        });
    }

    marks
}

/// Extract marks for the given changed page indices (in `bundle.pages()` order).
pub fn extract(bundle: &Bundle, changed: &[usize]) -> anyhow::Result<Vec<Mark>> {
    // Determine how many pages the source PDF has.
    // When there is NO source PDF, treat every page as "original" so glyph
    // highlights still emit `Highlight` rather than `InsertedPage`.
    let pdf_bytes_opt: Option<Vec<u8>> = bundle.source_pdf().map(|b| b.to_vec());

    // Build the text layer (for reconstructing highlighter ink → text).
    // If there's no source PDF, use an empty layer (reconstruct always returns "").
    let textlayer: Option<TextLayer> = match &pdf_bytes_opt {
        Some(pdf) => Some(TextLayer::extract(pdf)?),
        None => None,
    };

    // Build a lopdf::Document for per-page MediaBox lookup (only if we have a PDF).
    let doc_opt: Option<lopdf::Document> = match &pdf_bytes_opt {
        Some(pdf) => Some(lopdf::Document::load_mem(pdf)?),
        None => None,
    };

    // Helper: get page size (w, h) in PDF points for page `n` (0-based).
    // Falls back to first page, then to canvas size.
    let page_size = |n: usize| -> (f64, f64) {
        let Some(doc) = &doc_opt else {
            return bundle.canvas_size();
        };
        // lopdf pages are 1-based; n is 0-based.
        let pages = doc.get_pages();
        // Try the requested page (1-based).
        let page_num = (n + 1) as u32;
        let page_id = pages.get(&page_num).or_else(|| pages.values().next());
        let Some(&pid) = page_id else {
            return bundle.canvas_size();
        };
        let mb = doc
            .get_dictionary(pid)
            .ok()
            .and_then(|d| d.get(b"MediaBox").ok())
            .and_then(|o| o.as_array().ok());
        let Some(mb) = mb else {
            return bundle.canvas_size();
        };
        let num = |o: &lopdf::Object| {
            o.as_float()
                .map(|f| f as f64)
                .or_else(|_| o.as_i64().map(|i| i as f64))
                .ok()
        };
        match (mb.first(), mb.get(2), mb.get(1), mb.get(3)) {
            (Some(x0), Some(x1), Some(y0), Some(y1)) => {
                let w = num(x1).unwrap_or(0.0) - num(x0).unwrap_or(0.0);
                let h = num(y1).unwrap_or(0.0) - num(y0).unwrap_or(0.0);
                if w > 0.0 && h > 0.0 {
                    (w, h)
                } else {
                    bundle.canvas_size()
                }
            }
            _ => bundle.canvas_size(),
        }
    };

    // Collect all pages into a lookup keyed by page index.
    let all_pages = bundle.pages();
    let changed_set: std::collections::HashSet<usize> = changed.iter().copied().collect();

    let mut all_marks: Vec<Mark> = Vec::new();

    for page in &all_pages {
        if !changed_set.contains(&page.index) {
            continue;
        }

        let Some(scene) = page.scene()? else {
            continue;
        };

        // The page's backing source PDF page (None for inserted pages). The
        // coordinate transform + highlighter-ink text reconstruction key off the
        // SOURCE page, so they stay correct even when note-pages were inserted.
        let source_page = page.source_page();
        let transform = rmfiles::coords::Transform::new(page_size(source_page.unwrap_or(0)));

        let text_highlights: Vec<&TextHighlight> = scene.text_highlights();
        let strokes: Vec<&Stroke> = scene.strokes();

        let tl_ref = &textlayer;
        let marks = page_marks(
            page.index,
            source_page,
            &text_highlights,
            &strokes,
            &transform,
            &|pg, rect| match tl_ref {
                Some(tl) => tl.words_under(pg, rect),
                None => String::new(),
            },
        );

        all_marks.extend(marks);
    }

    Ok(all_marks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmfiles::{Pen, PenColor, Point};

    fn make_pen_stroke() -> Stroke {
        Stroke {
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
                    y: 50.0,
                    speed: None,
                    direction: None,
                    width: Some(3.0),
                    pressure: None,
                },
            ],
        }
    }

    fn make_highlighter_stroke() -> Stroke {
        Stroke {
            tool: Pen::Highlighter1,
            color: PenColor::Yellow,
            points: vec![
                Point {
                    x: 10.0,
                    y: 20.0,
                    speed: None,
                    direction: None,
                    width: Some(20.0),
                    pressure: None,
                },
                Point {
                    x: 100.0,
                    y: 20.0,
                    speed: None,
                    direction: None,
                    width: Some(20.0),
                    pressure: None,
                },
            ],
        }
    }

    fn dummy_transform() -> rmfiles::coords::Transform {
        // A 612×792 pt page (US Letter).
        rmfiles::coords::Transform::new((612.0, 792.0))
    }

    // A pen stroke on a real page → one Note carrying the strokes + source page.
    #[test]
    fn pen_stroke_gives_note() {
        let stroke = make_pen_stroke();
        let strokes = vec![&stroke];
        let transform = dummy_transform();
        let marks = page_marks(0, Some(7), &[], &strokes, &transform, &|_, _| String::new());
        let notes: Vec<_> = marks
            .iter()
            .filter(|m| matches!(m, Mark::Note { .. }))
            .collect();
        assert_eq!(notes.len(), 1, "expected exactly one Note");
        if let Mark::Note {
            source_page,
            strokes,
            ..
        } = &notes[0]
        {
            assert_eq!(*source_page, Some(7), "note keeps its source page");
            assert_eq!(strokes.len(), 1, "note carries the pen stroke");
        }
        let highlights = marks
            .iter()
            .filter(|m| matches!(m, Mark::Highlight { .. }))
            .count();
        assert_eq!(highlights, 0, "expected no highlights from pen stroke");
    }

    // An inserted page (source_page None) → one ink-only Note, no highlights.
    #[test]
    fn inserted_page_note_has_no_source() {
        let stroke = make_pen_stroke();
        let strokes = vec![&stroke];
        let transform = dummy_transform();
        let marks = page_marks(5, None, &[], &strokes, &transform, &|_, _| String::new());

        let notes: Vec<_> = marks
            .iter()
            .filter(|m| matches!(m, Mark::Note { .. }))
            .collect();
        assert_eq!(notes.len(), 1, "expected exactly one Note");
        if let Mark::Note { source_page, .. } = &notes[0] {
            assert_eq!(*source_page, None, "inserted page has no source");
        }
        let highlights = marks
            .iter()
            .filter(|m| matches!(m, Mark::Highlight { .. }))
            .count();
        assert_eq!(highlights, 0, "expected no highlights for inserted page");
    }

    // TextHighlight → verbatim Highlight (reconstruct never called).
    #[test]
    fn text_highlight_verbatim() {
        let highlight = TextHighlight {
            text: "hello world".into(),
            rectangles: vec![],
            color: PenColor::Highlight,
            color_rgba: None,
        };
        let highlights = vec![&highlight];
        let transform = dummy_transform();
        let marks = page_marks(0, Some(0), &highlights, &[], &transform, &|_, _| {
            panic!("reconstruct should not be called for TextHighlight")
        });

        assert_eq!(marks.len(), 1);
        if let Mark::Highlight { text, .. } = &marks[0] {
            assert_eq!(text, "hello world");
        } else {
            panic!("expected a Highlight mark");
        }
    }

    // Highlighter stroke with reconstruct returning "REBUILT" → Highlight with that text.
    #[test]
    fn highlighter_stroke_reconstruct_hit() {
        let stroke = make_highlighter_stroke();
        let strokes = vec![&stroke];
        let transform = dummy_transform();
        let marks = page_marks(0, Some(0), &[], &strokes, &transform, &|_, _| {
            "REBUILT".to_string()
        });

        let hl: Vec<_> = marks
            .iter()
            .filter(|m| matches!(m, Mark::Highlight { .. }))
            .collect();
        assert_eq!(hl.len(), 1, "expected one Highlight");
        if let Mark::Highlight { text, .. } = &hl[0] {
            assert_eq!(text, "REBUILT");
        }
    }

    // Highlighter stroke with reconstruct returning "" → no highlight emitted.
    #[test]
    fn highlighter_stroke_reconstruct_miss() {
        let stroke = make_highlighter_stroke();
        let strokes = vec![&stroke];
        let transform = dummy_transform();
        let marks = page_marks(0, Some(0), &[], &strokes, &transform, &|_, _| String::new());

        let hl: Vec<_> = marks
            .iter()
            .filter(|m| matches!(m, Mark::Highlight { .. }))
            .collect();
        assert_eq!(
            hl.len(),
            0,
            "expected no Highlight when reconstruct returns empty"
        );
    }

    // Integration test 1: rmtest-glyph.rmdoc — snap-to-text verbatim highlights.
    #[test]
    fn rmtest_glyph_verbatim_highlights() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let path = std::path::Path::new(manifest).join("tests/fixtures/rmtest-glyph.rmdoc");
        let bundle = rmfiles::Bundle::open(&path).unwrap();

        let pages = bundle.pages();
        let all_indices: Vec<usize> = pages.iter().map(|p| p.index).collect();

        let marks = extract(&bundle, &all_indices).unwrap();

        let texts: std::collections::HashSet<String> = marks
            .iter()
            .filter_map(|m| {
                if let Mark::Highlight { text, .. } = m {
                    Some(text.clone())
                } else {
                    None
                }
            })
            .collect();

        eprintln!("rmtest-glyph highlight texts: {texts:?}");
        eprintln!(
            "rmtest-glyph has_pdf={} page_count={}",
            bundle.source_pdf().is_some(),
            pages.len()
        );

        assert!(
            texts.contains("ARCHIVE"),
            "expected ARCHIVE in highlights; got {texts:?}"
        );
        assert!(
            texts.contains("Sphinx of black quartz, judge my vow."),
            "expected long quote in highlights; got {texts:?}"
        );
    }

    // Integration test 2: stamped-labels.rmdoc — highlighter ink reconstruction.
    #[test]
    fn stamped_labels_highlighter_reconstruction() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let path = std::path::Path::new(manifest).join("tests/fixtures/stamped-labels.rmdoc");
        let bundle = rmfiles::Bundle::open(&path).unwrap();

        let pages = bundle.pages();
        let all_indices: Vec<usize> = pages.iter().map(|p| p.index).collect();

        let marks = extract(&bundle, &all_indices).unwrap();

        let highlights: Vec<&Mark> = marks
            .iter()
            .filter(|m| matches!(m, Mark::Highlight { .. }))
            .collect();

        eprintln!("stamped-labels: {} highlight marks", highlights.len());
        for h in &highlights {
            if let Mark::Highlight { text, rgb, page } = h {
                eprintln!("  page={page} rgb={rgb:?} text={text:?}");
            }
        }

        // We must get at least one Highlight mark from the 4 highlighter strokes.
        assert!(
            !highlights.is_empty(),
            "expected at least 1 Highlight from stamped-labels highlighter ink"
        );

        // At least one must have non-empty text.
        let non_empty: Vec<_> = highlights
            .iter()
            .filter(|m| {
                if let Mark::Highlight { text, .. } = m {
                    !text.is_empty()
                } else {
                    false
                }
            })
            .collect();
        assert!(
            !non_empty.is_empty(),
            "expected at least one Highlight with non-empty reconstructed text"
        );

        // Attempt to assert a specific token. If geometry works, we should see
        // "ARCHIVE" or body words. We check for known tokens and tighten if found.
        let all_text: String = highlights
            .iter()
            .filter_map(|m| {
                if let Mark::Highlight { text, .. } = m {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        eprintln!("stamped-labels combined text: {all_text:?}");

        // The highlighter ink covers the "ARCHIVE" label and the body sentence.
        // Reconstruction via coords + the source PDF text layer is exact on this
        // real capture, so assert the actual highlighted content unconditionally.
        assert!(
            all_text.contains("ARCHIVE"),
            "expected ARCHIVE in reconstructed text; got {all_text:?}"
        );
        assert!(
            all_text.contains("quick brown fox"),
            "expected body sentence tokens in reconstructed text; got {all_text:?}"
        );
    }
}
