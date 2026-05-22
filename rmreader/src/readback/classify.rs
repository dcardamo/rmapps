//! Turn extracted on-device highlights into a plan of Readwise operations.
use std::collections::BTreeMap;

use crate::manifest::EmbeddedManifest;
use crate::readwise::{ActionKind, HighlightCreate};

/// One highlight located on a page, normalized from rmfiles output. `top_band` is
/// computed by the caller (device-space relative): true when the highlight sits in
/// the page's top action-label band.
#[derive(Debug, Clone)]
pub struct PageHighlight {
    pub page: usize,
    pub text: String,
    pub top_band: bool,
}

/// The set of Readwise operations implied by a document's annotations.
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub actions: Vec<(String, ActionKind)>, // (doc_id, kind)
    pub highlights: Vec<HighlightCreate>,
    pub warnings: Vec<String>,
}

/// Classify highlights against the embedded manifest.
/// - A highlight whose text is an action label AND is in the top band -> an action.
/// - Any other highlight -> a content highlight for the doc owning its page.
/// - 0 actions on a doc -> no location change; exactly 1 distinct action -> apply;
///   >= 2 distinct -> skip + warn (content highlights are still emitted).
/// - A highlight on a page not covered by the manifest -> warn + skip.
pub fn classify(m: &EmbeddedManifest, hs: &[PageHighlight]) -> Plan {
    let mut plan = Plan::default();
    let mut acted: BTreeMap<String, Vec<ActionKind>> = BTreeMap::new();

    for h in hs {
        let Some(doc) = m.doc_for_page(h.page) else {
            plan.warnings.push(format!(
                "highlight on page {} not in manifest; skipped",
                h.page
            ));
            continue;
        };
        let as_action = if h.top_band {
            ActionKind::parse_label(&h.text)
        } else {
            None
        };
        match as_action {
            Some(kind) => acted.entry(doc.id.clone()).or_default().push(kind),
            None => plan.highlights.push(HighlightCreate {
                text: h.text.clone(),
                title: doc.title.clone(),
                author: doc.author.clone(),
                source_url: doc.url.clone(),
                category: if doc.category.is_empty() {
                    "articles".into()
                } else {
                    doc.category.clone()
                },
            }),
        }
    }

    for (id, mut kinds) in acted {
        kinds.sort_by_key(|k| *k as u8);
        kinds.dedup();
        if kinds.len() == 1 {
            plan.actions.push((id, kinds[0]));
        } else {
            plan.warnings.push(format!(
                "doc {id}: {} action labels highlighted; skipped",
                kinds.len()
            ));
        }
    }
    plan
}
