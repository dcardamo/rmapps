//! The MVU loop runtime: the render walk (`render_document`) and the multi-cycle
//! driver (`App`, `DocSet`, `step` — added in a later task).

use crate::document::{DocKey, Document};
use crate::embed::embed_manifest;
use crate::error::Result;
use crate::manifest::{recover_regions, Manifest};
use crate::render::{compile_to_document, document_to_pdf};
use crate::widget::RenderCx;

/// Default document page geometry (points). 3:4-ish to suit e-ink; the device
/// fits to width. Single-page only this spec.
pub const DOC_PAGE_W: f64 = 420.0;
pub const DOC_PAGE_H: f64 = 560.0;

/// A rendered document: its PDF (manifest embedded), the recovered manifest, the
/// page height (for the device transform), and a content hash (for reconcile).
pub struct RenderedDoc {
    pub key: DocKey,
    pub pdf: Vec<u8>,
    pub manifest: Manifest,
    pub page_h: f64,
    pub hash: u64,
}

/// Assemble a document's Typst source: a page header plus each component's render
/// in flow order.
pub fn document_source<M>(doc: &Document<M>) -> String {
    let mut cx = RenderCx::new(0);
    let mut src = format!(
        "#set page(width: {DOC_PAGE_W}pt, height: {DOC_PAGE_H}pt, margin: 16pt)\n#set text(size: 12pt)\n"
    );
    for c in &doc.flow {
        src.push_str(&c.render(&mut cx));
    }
    src
}

/// Stable hash of a string (std DefaultHasher is deterministic within a build,
/// which is all reconcile needs — equal source -> equal hash).
fn hash_str(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// Render one document to a [`RenderedDoc`] at `version`.
pub fn render_document<M>(doc: &Document<M>, version: u64) -> Result<RenderedDoc> {
    let src = document_source(doc);
    let compiled = compile_to_document(&src)?;
    let page_h = compiled
        .pages
        .first()
        .map(|p| p.frame.height().to_pt())
        .unwrap_or(0.0);
    let manifest = recover_regions(&compiled)?.with_version(version);
    let pdf = embed_manifest(&document_to_pdf(&compiled)?, &manifest)?;
    Ok(RenderedDoc {
        key: doc.key.clone(),
        pdf,
        manifest,
        page_h,
        hash: hash_str(&src),
    })
}
