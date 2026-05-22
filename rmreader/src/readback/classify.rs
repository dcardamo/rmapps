//! Turn highlighter stroke geometry (in PDF points) into a plan of Readwise ops.
use std::collections::BTreeMap;

use crate::manifest::EmbeddedManifest;
use crate::readback::coords::PdfRect;
use crate::readwise::{ActionKind, HighlightCreate};

/// A highlighter stroke's bounding box on a page, in PDF points (bottom-left origin).
#[derive(Debug, Clone)]
pub struct StrokeHit {
    pub page: usize,
    pub bbox: PdfRect,
}

/// The set of Readwise operations implied by a document's annotations.
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub actions: Vec<(String, ActionKind)>, // (doc_id, kind)
    pub highlights: Vec<HighlightCreate>,
    pub warnings: Vec<String>,
}

/// Classify stroke hits against the embedded manifest.
///
/// - A stroke overlapping an action-label column rect → an action. When a stroke
///   overlaps multiple columns, the one with the **most** overlap area wins, so a
///   stroke that barely nicks a neighbour still resolves to the dominant column.
/// - Any other stroke → a content highlight for the doc owning its page; its text is
///   reconstructed via `words_under(page, &bbox)`. Empty text → skip + warning.
/// - Per doc: 0 actions → no location change; exactly 1 distinct action → apply;
///   ≥ 2 distinct → skip + warn (content highlights are still emitted).
/// - A stroke on a page not covered by the manifest → warn + skip.
pub fn classify(
    m: &EmbeddedManifest,
    hits: &[StrokeHit],
    words_under: impl Fn(usize, &PdfRect) -> String,
) -> Plan {
    let mut plan = Plan::default();
    let mut acted: BTreeMap<String, Vec<ActionKind>> = BTreeMap::new();

    for hit in hits {
        let Some(doc) = m.doc_for_page(hit.page) else {
            plan.warnings.push(format!(
                "stroke on page {} not in manifest; skipped",
                hit.page
            ));
            continue;
        };

        // Find the label rect with the greatest positive overlap area.
        let best = m
            .label_rects
            .iter()
            .map(|lr| (lr, hit.bbox.overlap_area(&lr.rect.to_pdf_rect())))
            .filter(|(_, area)| *area > 0.0)
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap());

        match best.and_then(|(lr, _)| ActionKind::parse_label(&lr.kind)) {
            Some(kind) => acted.entry(doc.id.clone()).or_default().push(kind),
            None => {
                let text = words_under(hit.page, &hit.bbox).trim().to_string();
                if text.is_empty() {
                    plan.warnings.push(format!(
                        "content highlight on page {} recovered no text; skipped",
                        hit.page
                    ));
                } else {
                    plan.highlights.push(HighlightCreate {
                        text,
                        title: doc.title.clone(),
                        author: doc.author.clone(),
                        source_url: doc.url.clone(),
                        category: if doc.category.is_empty() {
                            "articles".into()
                        } else {
                            doc.category.clone()
                        },
                    });
                }
            }
        }
    }

    for (id, mut kinds) in acted {
        kinds.sort_by_key(|k| *k as u8);
        kinds.dedup();
        if kinds.len() == 1 {
            plan.actions.push((id, kinds[0]));
        } else {
            plan.warnings.push(format!(
                "doc {id}: {} distinct action labels highlighted; skipped",
                kinds.len()
            ));
        }
    }
    plan
}
