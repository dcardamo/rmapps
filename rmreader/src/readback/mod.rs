//! Read on-device annotations and turn them into Readwise operations.
pub mod classify;
pub mod coords;
pub mod textlayer;

pub use classify::{classify, Plan, StrokeHit};
pub use coords::{PdfRect, Transform};
pub use textlayer::{TextLayer, Word};

use crate::deploy::Deployer;
use crate::readwise::{self, ActionKind, HttpTransport};

/// Read on-device annotations for one collection and apply them to Readwise.
/// Best-effort: returns the executed Plan; per-op failures are logged, not fatal.
/// First run / no doc / no embedded manifest -> Ok(Plan::default()), a clean no-op.
pub fn sync_collection(
    deployer: &dyn Deployer,
    transport: &dyn HttpTransport,
    token: &str,
    folder: &str,
    name: &str,
) -> anyhow::Result<Plan> {
    let Some(bundle_path) = deployer.fetch(folder, name)? else {
        return Ok(Plan::default());
    };
    let plan = detect(&bundle_path)?;
    eprintln!(
        "[rmreader] {name}: read-back found {} action(s), {} content highlight(s), {} warning(s)",
        plan.actions.len(),
        plan.highlights.len(),
        plan.warnings.len()
    );
    for (id, kind) in &plan.actions {
        eprintln!("[rmreader] {name}: action {kind:?} on doc {id}");
    }
    execute(transport, token, &plan);
    Ok(plan)
}

/// Detect (without applying) the read-back plan for an already-downloaded bundle.
/// No network side effects — used by `sync_collection` and the `readback_inspect`
/// example for non-destructive dry runs.
pub fn detect(bundle_path: &std::path::Path) -> anyhow::Result<Plan> {
    let bundle = rmfiles::Bundle::open(bundle_path)?;
    let Some(pdf) = bundle.source_pdf().map(|b| b.to_vec()) else {
        return Ok(Plan::default());
    };
    let doc = lopdf::Document::load_mem(&pdf)?;
    let Some(manifest) = crate::embed::read(&doc)? else {
        eprintln!("[rmreader] no embedded manifest in fetched PDF; skipping read-back");
        return Ok(Plan::default());
    };
    // Page size from the source PDF MediaBox; fall back to the device canvas only if
    // it is somehow missing (the transform renders at native dpi, so it needs the
    // PDF page size, not the canvas size).
    let page = first_page_size(&doc).unwrap_or_else(|| bundle.canvas_size());
    let transform = Transform::new(page);
    let textlayer = TextLayer::extract(&pdf)?;

    let mut hits = Vec::new();
    for pg in bundle.pages() {
        if let Some(scene) = pg.scene()? {
            for s in scene.strokes() {
                if !s.is_highlighter() {
                    continue;
                }
                // Expand the centre-line bbox vertically so a horizontal highlight
                // covers the text line it overlays (the raw points lie on the ink
                // band's centre axis). Cap the expansion at ~half a text line so a
                // single-line highlight doesn't also grab the lines above/below.
                const MAX_HALF_H_PT: f64 = 6.0;
                let ink_half_h_pt = s
                    .points
                    .iter()
                    .filter_map(|p| p.width)
                    .fold(0.0f32, f32::max) as f64
                    / (2.0 * transform.scale());
                let half_h = ink_half_h_pt.min(MAX_HALF_H_PT);
                if let Some(mut bbox) =
                    transform.pdf_bbox(s.points.iter().map(|p| (p.x as f64, p.y as f64)))
                {
                    bbox.y0 -= half_h;
                    bbox.y1 += half_h;
                    hits.push(StrokeHit {
                        page: pg.index,
                        bbox,
                    });
                }
            }
        }
    }
    Ok(classify(&manifest, &hits, |page, rect| {
        textlayer.words_under(page, rect)
    }))
}

fn execute(t: &dyn HttpTransport, token: &str, plan: &Plan) {
    for (id, kind) in &plan.actions {
        // Exhaustive match: a new ActionKind variant will cause a compile error
        // rather than a runtime panic.
        let r = match kind {
            ActionKind::Delete => readwise::delete_document(t, token, id),
            ActionKind::Inbox | ActionKind::Later | ActionKind::Archive => {
                readwise::update_location(t, token, id, kind.location().unwrap())
            }
        };
        if let Err(e) = r {
            eprintln!("[rmreader] action {id:?} {kind:?} failed: {e:#}");
        }
    }
    if let Err(e) = readwise::create_highlights(t, token, &plan.highlights) {
        eprintln!("[rmreader] create_highlights failed: {e:#}");
    }
    for w in &plan.warnings {
        eprintln!("[rmreader] readback: {w}");
    }
}

/// PDF points (w,h) of the first page's MediaBox.
fn first_page_size(doc: &lopdf::Document) -> Option<(f64, f64)> {
    let page_id = *doc.get_pages().values().next()?;
    let mb = doc
        .get_dictionary(page_id)
        .ok()?
        .get(b"MediaBox")
        .ok()?
        .as_array()
        .ok()?;
    let num = |o: &lopdf::Object| {
        o.as_float()
            .map(|f| f as f64)
            .or_else(|_| o.as_i64().map(|i| i as f64))
            .ok()
    };
    // Use .get()/.first() so a malformed/short MediaBox returns None rather than panicking.
    Some((
        num(mb.get(2)?)? - num(mb.first()?)?,
        num(mb.get(3)?)? - num(mb.get(1)?)?,
    ))
}
