//! Turn a bundle's changed pages into an ordered list of digest marks.
use rmfiles::{Bundle, PenColor, Stroke, TextHighlight};

use crate::ink::{render_strokes, Background, InkOpts};
use crate::textlayer::TextLayer;

/// One item destined for the digest.
pub enum Mark {
    /// A highlight (snap-to-text verbatim, or reconstructed from highlighter ink).
    Highlight {
        page: usize,
        text: String,
        color: PenColor,
    },
    /// A handwritten note (pen ink) on an existing page, rendered to PNG.
    Note { page: usize, png: Vec<u8> },
    /// A reMarkable-inserted page (no backing PDF page), rendered full to PNG.
    InsertedPage { after_page: usize, png: Vec<u8> },
}

/// Classify one page's marks.
///
/// `reconstruct` maps a highlighter stroke's `PdfRect` to the underlying text
/// (injected so tests can stub it instead of requiring a real PDF).
pub(crate) fn page_marks(
    page_index: usize,
    source_pages: usize,
    text_highlights: &[&TextHighlight],
    strokes: &[&Stroke],
    transform: &rmfiles::coords::Transform,
    reconstruct: &dyn Fn(usize, &rmfiles::coords::PdfRect) -> String,
) -> anyhow::Result<Vec<Mark>> {
    let mut marks = Vec::new();

    // An inserted page has no backing PDF page — render ALL strokes as a full image.
    if page_index >= source_pages {
        let after_page = source_pages.saturating_sub(1);
        if !strokes.is_empty() {
            let opts = InkOpts {
                background: Background::White,
                ..Default::default()
            };
            let png = render_strokes(strokes, &opts)?;
            marks.push(Mark::InsertedPage { after_page, png });
        }
        return Ok(marks);
    }

    // Snap-to-text highlights: verbatim text from the device.
    for h in text_highlights {
        marks.push(Mark::Highlight {
            page: page_index,
            text: h.text.clone(),
            color: h.color,
        });
    }

    // Highlighter ink strokes: reconstruct text from geometry.
    // Pen strokes: collect separately and emit as a single Note.
    let mut pen_strokes: Vec<&Stroke> = Vec::new();

    for &stroke in strokes {
        if stroke.is_highlighter() {
            // Expand the bbox vertically so the highlight band covers the text
            // line it overlays. The raw points lie on the ink centre axis, so
            // we expand by half the ink width (capped at MAX_HALF_H_PT).
            // Matches the logic in rmreader readback/mod.rs `detect()`.
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
                let text = reconstruct(page_index, &bbox);
                if !text.is_empty() {
                    marks.push(Mark::Highlight {
                        page: page_index,
                        text,
                        color: stroke.color,
                    });
                }
            }
        } else {
            pen_strokes.push(stroke);
        }
    }

    // All pen strokes on this page become a single Note.
    if !pen_strokes.is_empty() {
        let opts = InkOpts {
            background: Background::White,
            ..Default::default()
        };
        let png = render_strokes(&pen_strokes, &opts)?;
        marks.push(Mark::Note {
            page: page_index,
            png,
        });
    }

    Ok(marks)
}

/// How many pages of the source PDF to treat as "original" (vs inserted).
///
/// Pages at index `>= source_pages` are reMarkable-inserted note-pages. The PDF
/// page count comes from lopdf, but lopdf can LOAD some real-world PDFs while
/// failing to enumerate their page tree (returning 0). A literal 0 would
/// misclassify EVERY annotated page as inserted and silently drop real
/// highlights, so we fall back to the bundle's page count (treating all pages
/// as original) whenever the PDF is absent, unparseable, or reports 0 pages.
pub(crate) fn effective_source_pages(pdf: Option<&[u8]>, bundle_pages: usize) -> usize {
    match pdf {
        Some(bytes) => {
            let n = lopdf::Document::load_mem(bytes)
                .map(|d| d.get_pages().len())
                .unwrap_or(0);
            if n > 0 {
                n
            } else {
                bundle_pages
            }
        }
        None => bundle_pages,
    }
}

/// Extract marks for the given changed page indices (in `bundle.pages()` order).
pub fn extract(bundle: &Bundle, changed: &[usize]) -> anyhow::Result<Vec<Mark>> {
    // Determine how many pages the source PDF has.
    // When there is NO source PDF, treat every page as "original" so glyph
    // highlights still emit `Highlight` rather than `InsertedPage`.
    let pdf_bytes_opt: Option<Vec<u8>> = bundle.source_pdf().map(|b| b.to_vec());

    let source_pages = effective_source_pages(pdf_bytes_opt.as_deref(), bundle.pages().len());

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

        let transform = rmfiles::coords::Transform::new(page_size(page.index));

        let text_highlights: Vec<&TextHighlight> = scene.text_highlights();
        let strokes: Vec<&Stroke> = scene.strokes();

        let tl_ref = &textlayer;
        let marks = page_marks(
            page.index,
            source_pages,
            &text_highlights,
            &strokes,
            &transform,
            &|pg, rect| match tl_ref {
                Some(tl) => tl.words_under(pg, rect),
                None => String::new(),
            },
        )?;

        all_marks.extend(marks);
    }

    Ok(all_marks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmfiles::{Pen, PenColor, Point};

    /// A valid PDF with `n` real pages (lopdf enumerates it).
    fn pdf_with_pages(n: usize) -> Vec<u8> {
        use lopdf::{dictionary, Document, Object};
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let kids: Vec<Object> = (0..n)
            .map(|_| {
                doc.add_object(dictionary! {
                    "Type" => "Page", "Parent" => pages_id,
                    "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                })
                .into()
            })
            .collect();
        let count = kids.len() as i64;
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => kids, "Count" => count,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut buf = Vec::new();
        doc.save_to(&mut buf).unwrap();
        buf
    }

    #[test]
    fn source_pages_uses_real_count_for_normal_pdf() {
        let pdf = pdf_with_pages(3);
        assert_eq!(effective_source_pages(Some(&pdf), 99), 3);
    }

    #[test]
    fn source_pages_falls_back_when_no_pdf() {
        assert_eq!(effective_source_pages(None, 42), 42);
    }

    #[test]
    fn source_pages_falls_back_on_unparseable_pdf() {
        assert_eq!(effective_source_pages(Some(b"not a pdf at all"), 42), 42);
    }

    #[test]
    fn source_pages_falls_back_when_lopdf_reports_zero_pages() {
        // A catalog whose Pages tree has no Kids: lopdf LOADS it but get_pages()
        // returns 0. This is the real-world case (a 447-page manual lopdf can load
        // but not enumerate) that previously dropped every highlight.
        let pdf = pdf_with_pages(0);
        // Sanity: lopdf does load it, and reports 0 pages.
        let doc = lopdf::Document::load_mem(&pdf).expect("lopdf loads 0-page pdf");
        assert_eq!(doc.get_pages().len(), 0);
        // The helper must NOT return 0 — it falls back to the bundle page count.
        assert_eq!(effective_source_pages(Some(&pdf), 447), 447);
    }

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

    // A pen stroke on a normal page → exactly one Note with non-empty PNG.
    #[test]
    fn pen_stroke_gives_note() {
        let stroke = make_pen_stroke();
        let strokes = vec![&stroke];
        let transform = dummy_transform();
        let marks = page_marks(0, 1, &[], &strokes, &transform, &|_, _| String::new()).unwrap();
        let notes: Vec<_> = marks
            .iter()
            .filter(|m| matches!(m, Mark::Note { .. }))
            .collect();
        assert_eq!(notes.len(), 1, "expected exactly one Note");
        if let Mark::Note { png, .. } = &notes[0] {
            assert!(!png.is_empty(), "PNG should be non-empty");
        }
        let highlights: Vec<_> = marks
            .iter()
            .filter(|m| matches!(m, Mark::Highlight { .. }))
            .collect();
        assert_eq!(
            highlights.len(),
            0,
            "expected no highlights from pen stroke"
        );
    }

    // page_index >= source_pages → InsertedPage, no highlights.
    #[test]
    fn inserted_page_when_index_exceeds_source() {
        let stroke = make_pen_stroke();
        let strokes = vec![&stroke];
        let transform = dummy_transform();
        let marks = page_marks(5, 2, &[], &strokes, &transform, &|_, _| String::new()).unwrap();

        let inserted: Vec<_> = marks
            .iter()
            .filter(|m| matches!(m, Mark::InsertedPage { .. }))
            .collect();
        assert_eq!(inserted.len(), 1, "expected exactly one InsertedPage");
        if let Mark::InsertedPage { after_page, png } = &inserted[0] {
            assert_eq!(*after_page, 1, "after_page should be source_pages - 1 = 1");
            assert!(!png.is_empty(), "PNG should be non-empty");
        }

        let highlights: Vec<_> = marks
            .iter()
            .filter(|m| matches!(m, Mark::Highlight { .. }))
            .collect();
        assert_eq!(
            highlights.len(),
            0,
            "expected no highlights for inserted page"
        );
    }

    // TextHighlight → verbatim Highlight (reconstruct never called).
    #[test]
    fn text_highlight_verbatim() {
        let highlight = TextHighlight {
            text: "hello world".into(),
            rectangles: vec![],
            color: PenColor::Highlight,
        };
        let highlights = vec![&highlight];
        let transform = dummy_transform();
        let marks = page_marks(0, 1, &highlights, &[], &transform, &|_, _| {
            panic!("reconstruct should not be called for TextHighlight")
        })
        .unwrap();

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
        let marks = page_marks(0, 1, &[], &strokes, &transform, &|_, _| {
            "REBUILT".to_string()
        })
        .unwrap();

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
        let marks = page_marks(0, 1, &[], &strokes, &transform, &|_, _| String::new()).unwrap();

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
            if let Mark::Highlight { text, color, page } = h {
                eprintln!("  page={page} color={color:?} text={text:?}");
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
