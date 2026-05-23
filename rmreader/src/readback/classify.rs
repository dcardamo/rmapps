//! Turn highlighter stroke geometry (in PDF points) into a plan of Readwise ops.
use std::collections::BTreeMap;

use crate::manifest::EmbeddedManifest;
use crate::readback::coords::PdfRect;
use crate::readwise::{ActionKind, HighlightCreate};

/// Map a Readwise Reader category to a valid v2 highlights category.
/// Only `books`, `articles`, `tweets`, and `podcasts` are accepted by the v2 API.
fn v2_category(reader_category: &str) -> &'static str {
    match reader_category.trim().to_ascii_lowercase().as_str() {
        "tweet" | "tweets" => "tweets",
        "podcast" | "podcasts" => "podcasts",
        "book" | "books" | "epub" => "books",
        _ => "articles",
    }
}

/// Decode common HTML entities in highlight text.
/// Processes `&amp;` first to avoid double-decoding (e.g. `&amp;lt;` → `&lt;`, not `<`).
fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

/// A highlighter stroke's bounding box on a page, in PDF points (bottom-left origin).
#[derive(Debug, Clone)]
pub struct StrokeHit {
    pub page: usize,
    pub bbox: PdfRect,
}

/// A snap-to-text highlight (`GlyphRange`) on a page: the device gave us the
/// exact highlighted string verbatim, so no geometry or text-layer lookup is
/// needed. This is the primary read-back path.
#[derive(Debug, Clone)]
pub struct TextHit {
    pub page: usize,
    pub text: String,
}

/// The set of Readwise operations implied by a document's annotations.
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub actions: Vec<(String, ActionKind)>, // (doc_id, kind)
    pub highlights: Vec<HighlightCreate>,
    pub warnings: Vec<String>,
}

/// Horizontal overlap between `a` and a rect defined by `[b_x0, b_x1]`.
fn x_overlap(a: &PdfRect, b_x0: f64, b_x1: f64) -> f64 {
    (a.x1.min(b_x1) - a.x0.max(b_x0)).max(0.0)
}

/// Classify text highlights and stroke hits against the embedded manifest.
///
/// Two read-back inputs feed the same plan and share one per-doc action map so
/// text- and ink-derived actions merge:
///
/// **Text highlights (`text_hits`) — primary, exact.** The device already gave
/// us the verbatim highlighted string (a snap-to-text `GlyphRange`), so this
/// path needs no coordinate transform and no `words_under` reconstruction:
/// - If the text parses as an action label → that doc's action.
/// - Otherwise → a content highlight carrying the exact text.
///
/// **Strokes (`hits`) — ink fallback (geometry).**
/// - A stroke whose **center y** falls inside an action-label column rect's y-band,
///   AND that overlaps the column in X, is classified as an action. When multiple
///   columns qualify, the one with the greatest horizontal (x) overlap wins.
///   This prevents inflated bboxes of first-body-line highlights from bleeding into
///   the label band and being misclassified (the inflation is symmetric, so the bbox
///   center equals the original stroke center).
/// - Any other stroke → a content highlight for the doc owning its page; its text is
///   reconstructed via `words_under(page, &bbox)`. Empty text → skip + warning.
///
/// Resolution (both paths): per doc, 0 actions → no location change; exactly 1
/// distinct action → apply; ≥ 2 distinct → skip + warn (content highlights are
/// still emitted). A hit on a page not covered by the manifest → warn + skip.
pub fn classify(
    m: &EmbeddedManifest,
    text_hits: &[TextHit],
    hits: &[StrokeHit],
    words_under: impl Fn(usize, &PdfRect) -> String,
) -> Plan {
    let mut plan = Plan::default();
    let mut acted: BTreeMap<String, Vec<ActionKind>> = BTreeMap::new();

    // Primary path: snap-to-text highlights with verbatim strings.
    for hit in text_hits {
        let Some(doc) = m.doc_for_page(hit.page) else {
            plan.warnings.push(format!(
                "text highlight on page {} not in manifest; skipped",
                hit.page
            ));
            continue;
        };

        match ActionKind::parse_label(&hit.text) {
            Some(kind) => acted.entry(doc.id.clone()).or_default().push(kind),
            None => {
                let text = decode_entities(hit.text.trim());
                if text.is_empty() {
                    plan.warnings.push(format!(
                        "text highlight on page {} was empty; skipped",
                        hit.page
                    ));
                } else {
                    plan.highlights.push(HighlightCreate {
                        text,
                        title: doc.title.clone(),
                        author: doc.author.clone(),
                        source_url: doc.url.clone(),
                        category: v2_category(&doc.category).to_string(),
                    });
                }
            }
        }
    }

    // Fallback path: highlighter ink geometry.
    for hit in hits {
        let Some(doc) = m.doc_for_page(hit.page) else {
            plan.warnings.push(format!(
                "stroke on page {} not in manifest; skipped",
                hit.page
            ));
            continue;
        };

        // An action requires the stroke's CENTER y to fall inside the label band.
        // Inflation is symmetric so bbox center == original stroke center — a first-
        // body-line highlight whose inflated top edge pokes into the band will have a
        // center below the band and is correctly ignored here.
        let cy = (hit.bbox.y0 + hit.bbox.y1) / 2.0;
        let best = m
            .label_rects
            .iter()
            .filter(|lr| cy >= lr.rect.y0 && cy <= lr.rect.y1) // center inside y-band
            .map(|lr| (lr, x_overlap(&hit.bbox, lr.rect.x0, lr.rect.x1)))
            .filter(|(_, ox)| *ox > 0.0)
            .max_by(|(_, a), (_, b)| a.total_cmp(b));

        match best.and_then(|(lr, _)| ActionKind::parse_label(&lr.kind)) {
            Some(kind) => acted.entry(doc.id.clone()).or_default().push(kind),
            None => {
                let text = decode_entities(words_under(hit.page, &hit.bbox).trim());
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
                        category: v2_category(&doc.category).to_string(),
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
