//! On-device transport for the reading queue, via the `rmapi` CLI. This is NOT
//! framework runtime — it lives in the app, shells out to `rmapi`, and is used
//! by the manual device bar. The framework owns only the loop *body*
//! (`App::step`); push/pull/delete are here.
//!
//! The `rmapi` invocations mirror the proven Spec #3 helpers
//! (`inkapp-harness/tests/common/mod.rs`): always `-ni` (non-interactive) with
//! stdin nulled (token-clobber guard, mechanics doc §10); `put --content-only`
//! (PDF-blob-only push, which preserves the device ink layer on a re-push, §3);
//! folder pulls via `mget` (plain `get` is single-file and errors on a folder).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use inkapp::{App as Framework, DocSet, Remarkable};
use inkapp_core::device::Device;
use inkapp_core::ink::Stroke;
use rm_files::Bundle;

use crate::{Connectors, Msg};

/// reMarkable folder for the app's documents.
const FOLDER: &str = "/ReadingQueue";

/// `rmapi -ni mkdir` (non-interactive; stdin nulled). Errors (e.g. "already
/// exists") are ignored — create each ancestor level separately (mechanics §10).
fn rmapi_mkdir(folder: &str) {
    let _ = Command::new("rmapi")
        .args(["-ni", "mkdir", folder])
        .stdin(Stdio::null())
        .status();
}

/// Push a rendered PDF as document `<key>` under FOLDER via
/// `rmapi -ni put --content-only`. On first push this creates the document; on a
/// re-push of the same name it swaps only the PDF blob, preserving the device's
/// ink layer (remarkable-pdf-mechanics.md §3) — which is how an updated article
/// keeps the user's annotations.
pub fn push_doc(key: &str, pdf: &[u8]) -> std::io::Result<()> {
    rmapi_mkdir(FOLDER);
    // The on-device visibleName is the file stem, so name the temp file <key>.pdf.
    let tmp = std::env::temp_dir().join(format!("{key}.pdf"));
    std::fs::write(&tmp, pdf)?;
    let ok = Command::new("rmapi")
        .args([
            "-ni",
            "put",
            "--content-only",
            tmp.to_str().unwrap(),
            FOLDER,
        ])
        .stdin(Stdio::null())
        .status()?
        .success();
    assert!(ok, "rmapi put failed for {key}");
    Ok(())
}

/// Delete document `<key>` from the device (`rmapi -ni rm`). Non-fatal on failure
/// (a missing doc is fine).
pub fn delete_doc(key: &str) {
    let _ = Command::new("rmapi")
        .args(["-ni", "rm", &format!("{FOLDER}/{key}")])
        .stdin(Stdio::null())
        .status();
}

/// Recursively collect `*.rmdoc` files under `dir` (rmapi `mget` nests downloads
/// under a subdir named after the remote folder, so we walk rather than assume a
/// flat layout).
fn find_rmdocs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            find_rmdocs(&p, out);
        } else if p.extension().is_some_and(|x| x == "rmdoc") {
            out.push(p);
        }
    }
}

/// Assemble per-page PDF-space strokes from an `.rmdoc` bundle, indexed by the
/// bundle's `.content` page order: slot `p` aligns with the manifest's
/// `region.page == p`. An un-inked page occupies its slot as an empty `Vec`, so it
/// never shifts later pages. All pages of a document share one `page_h` (Typst
/// `#set page` fixes every page to the same height). Empty if the bundle won't open.
pub fn strokes_by_page(device: &Remarkable, path: &Path, page_h: f64) -> Vec<Vec<Stroke>> {
    let Ok(bundle) = Bundle::open(path) else {
        return Vec::new();
    };
    bundle
        .pages()
        .iter()
        .map(|pg| match pg.scene_bytes() {
            Some(bytes) => device.read_ink(bytes, page_h).unwrap_or_default(),
            None => Vec::new(),
        })
        .collect()
}

/// Pull the whole FOLDER once (`rmapi -ni mget`) and return per-key ink. The doc
/// filename stem is the key (we push `<key>.pdf`), so a pulled `<key>.rmdoc` maps
/// back to its key and is decoded with that key's page height. Returns empty if
/// nothing has been pulled yet (e.g. before the first device sync).
pub fn pull_ink(
    device: &Remarkable,
    page_h_by_key: &HashMap<String, f64>,
) -> HashMap<String, Vec<Vec<Stroke>>> {
    let dir = std::env::temp_dir().join("reading-queue-pull");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mk pull dir");
    let ok = Command::new("rmapi")
        .args(["-ni", "mget", FOLDER])
        .current_dir(&dir)
        .stdin(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let mut out = HashMap::new();
    if !ok {
        return out;
    }
    let mut rmdocs = Vec::new();
    find_rmdocs(&dir, &mut rmdocs);
    for p in rmdocs {
        let Some(key) = p.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
            continue;
        };
        let page_h = page_h_by_key.get(&key).copied().unwrap_or(0.0);
        let pages = strokes_by_page(device, &p, page_h);
        // Insert only when the document carries some ink on some page.
        if pages.iter().any(|pg| !pg.is_empty()) {
            out.insert(key, pages);
        }
    }
    out
}

/// Render + push the current set (the "publish" half of a cycle).
pub async fn publish(app: &mut Framework<crate::App, Msg, Connectors>, set: &mut DocSet) {
    let rendered = app.render(set).await.expect("render");
    for rd in &rendered {
        push_doc(&rd.key.0, &rd.pdf).expect("push");
    }
    println!("published {} document(s) to {FOLDER}", rendered.len());
}

/// Pull ink for the whole folder, step once, and apply ops to the device (push
/// updated/created, delete removed).
pub async fn sync_once(
    app: &mut Framework<crate::App, Msg, Connectors>,
    device: &Remarkable,
    set: &mut DocSet,
) {
    let page_h: HashMap<String, f64> = set
        .keys()
        .into_iter()
        .filter_map(|k| set.page_h(&k).map(|h| (k.0, h)))
        .collect();
    let ink = pull_ink(device, &page_h);
    let cycle = app.step(set, &ink).await.expect("step");
    for op in &cycle.ops {
        if let inkapp_core::reconcile::DocOp::Delete(k) = op {
            delete_doc(&k.0);
        }
    }
    for rd in &cycle.rendered {
        push_doc(&rd.key.0, &rd.pdf).expect("push updated");
    }
    println!(
        "synced: {} message(s), {} op(s)",
        cycle.decoded.len(),
        cycle.ops.len()
    );
}
