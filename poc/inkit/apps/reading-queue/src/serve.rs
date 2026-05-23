//! On-device transport for the reading queue, via the `rmapi` CLI. This is NOT
//! framework runtime — it lives in the app, shells out to `rmapi`, and is used
//! by the manual device bar. The framework owns only the loop *body*
//! (`App::step`); push/pull/delete are here.

use std::collections::HashMap;
use std::process::Command;

use inkapp::{App as Framework, DocSet, Remarkable};
use inkapp_core::device::Device;
use inkapp_core::ink::Stroke;

use crate::{Connectors, Msg};

/// reMarkable folder for the app's documents.
const FOLDER: &str = "/ReadingQueue";

/// Push a rendered PDF as `<key>` under FOLDER (writing a temp file, then
/// `rmapi put`). Non-recursive mkdir per the mechanics doc.
pub fn push_doc(key: &str, pdf: &[u8]) -> std::io::Result<()> {
    let _ = Command::new("rmapi").args(["mkdir", FOLDER]).status(); // ignore "exists"
    let tmp = std::env::temp_dir().join(format!("{key}.pdf"));
    std::fs::write(&tmp, pdf)?;
    let status = Command::new("rmapi")
        .args(["put", tmp.to_str().unwrap(), FOLDER])
        .status()?;
    assert!(status.success(), "rmapi put failed for {key}");
    Ok(())
}

/// Pull ink for `key` from the device, returning PDF-space strokes (empty if the
/// document has no annotations yet). Uses `rmapi get` into a temp `.rmdoc`, reads
/// the first `.rm`, and parses it through the device transform.
pub fn pull_ink(device: &Remarkable, key: &str, page_h: f64) -> std::io::Result<Vec<Stroke>> {
    let out = std::env::temp_dir().join(format!("{key}.rmdoc"));
    let status = Command::new("rmapi")
        .args([
            "get",
            &format!("{FOLDER}/{key}"),
            "-o",
            out.to_str().unwrap(),
        ])
        .status()?;
    if !status.success() {
        return Ok(Vec::new());
    }
    let file = std::fs::File::open(&out)?;
    let mut zip = zip::ZipArchive::new(file).expect("rmdoc zip");
    let rm_name = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .find(|n| n.ends_with(".rm"));
    let Some(rm_name) = rm_name else {
        return Ok(Vec::new());
    };
    use std::io::Read;
    let mut bytes = Vec::new();
    zip.by_name(&rm_name).unwrap().read_to_end(&mut bytes)?;
    Ok(device.read_ink(&bytes, page_h).unwrap_or_default())
}

/// Delete a document from the device.
pub fn delete_doc(key: &str) {
    let _ = Command::new("rmapi")
        .args(["rm", &format!("{FOLDER}/{key}")])
        .status();
}

/// Render + push the current set (the "publish" half of a cycle).
pub fn publish(app: &mut Framework<crate::App, Msg, Connectors>, set: &mut DocSet) {
    let rendered = app.render(set).expect("render");
    for rd in &rendered {
        push_doc(&rd.key.0, &rd.pdf).expect("push");
    }
    println!("published {} document(s) to {FOLDER}", rendered.len());
}

/// Pull ink for every current key, step once, and apply ops to the device
/// (push updated/created, delete removed).
pub fn sync_once(
    app: &mut Framework<crate::App, Msg, Connectors>,
    device: &Remarkable,
    set: &mut DocSet,
) {
    let mut ink: HashMap<String, Vec<Stroke>> = HashMap::new();
    for key in set.keys() {
        let ph = set.page_h(&key).unwrap_or(0.0);
        if let Ok(strokes) = pull_ink(device, &key.0, ph) {
            if !strokes.is_empty() {
                ink.insert(key.0.clone(), strokes);
            }
        }
    }
    let cycle = app.step(set, &ink).expect("step");
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
