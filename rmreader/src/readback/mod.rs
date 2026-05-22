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
    let bundle = rmfiles::Bundle::open(&bundle_path)?;
    let Some(pdf) = bundle.source_pdf().map(|b| b.to_vec()) else {
        return Ok(Plan::default());
    };
    let doc = lopdf::Document::load_mem(&pdf)?;
    let Some(manifest) = crate::embed::read(&doc)? else {
        eprintln!("[rmreader] {name}: no embedded manifest in fetched PDF; skipping read-back");
        return Ok(Plan::default());
    };
    let canvas = bundle.canvas_size();
    let page = first_page_size(&doc).unwrap_or(canvas); // (w,h) PDF points
    let transform = Transform::new(canvas, page);
    let textlayer = TextLayer::extract(&pdf)?;

    let mut hits = Vec::new();
    for pg in bundle.pages() {
        if let Some(scene) = pg.scene()? {
            for s in scene.strokes() {
                if !s.is_highlighter() {
                    continue;
                }
                // Expand the centre-line bbox by the stroke's physical half-height
                // in PDF points so that horizontal highlights cover the text they
                // visually overlay (the raw points lie on the centre axis of the
                // rendered ink band).
                let half_h = s
                    .points
                    .iter()
                    .filter_map(|p| p.width)
                    .fold(0.0f32, f32::max) as f64
                    / (2.0 * transform.scale());
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
    let plan = classify(&manifest, &hits, |page, rect| {
        textlayer.words_under(page, rect)
    });
    execute(transport, token, &plan);
    Ok(plan)
}

fn execute(t: &dyn HttpTransport, token: &str, plan: &Plan) {
    for (id, kind) in &plan.actions {
        let r = match kind {
            ActionKind::Delete => readwise::delete_document(t, token, id),
            k => readwise::update_location(
                t,
                token,
                id,
                k.location().expect("non-delete has a location"),
            ),
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
    Some((num(&mb[2])? - num(&mb[0])?, num(&mb[3])? - num(&mb[1])?))
}
