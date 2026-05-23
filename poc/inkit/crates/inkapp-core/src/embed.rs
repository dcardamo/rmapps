use lopdf::{Dictionary, Document, Object};

use crate::error::{Error, Result};
use crate::manifest::Manifest;

/// Info-dictionary key under which the manifest JSON is stored.
const MANIFEST_KEY: &[u8] = b"InkappManifest";

/// Embed the manifest as JSON in the PDF's Info dictionary.
pub fn embed_manifest(pdf: &[u8], manifest: &Manifest) -> Result<Vec<u8>> {
    let json = serde_json::to_string(manifest).map_err(|e| Error::Manifest(e.to_string()))?;
    let mut doc = Document::load_mem(pdf).map_err(|e| Error::Manifest(e.to_string()))?;

    // Ensure an Info dictionary exists and is referenced by the trailer.
    let info_id = match doc.trailer.get(b"Info") {
        Ok(obj) => obj
            .as_reference()
            .map_err(|e| Error::Manifest(e.to_string()))?,
        Err(_) => {
            let id = doc.add_object(Object::Dictionary(Dictionary::new()));
            doc.trailer.set("Info", Object::Reference(id));
            id
        }
    };
    if let Ok(Object::Dictionary(info)) = doc.get_object_mut(info_id) {
        info.set(MANIFEST_KEY, Object::string_literal(json));
    } else {
        return Err(Error::Manifest("Info object is not a dictionary".into()));
    }

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| Error::Manifest(e.to_string()))?;
    Ok(out)
}

/// Extract the manifest JSON from the PDF's Info dictionary.
pub fn extract_manifest(pdf: &[u8]) -> Result<Manifest> {
    let doc = Document::load_mem(pdf).map_err(|e| Error::Manifest(e.to_string()))?;
    let info_id = match doc.trailer.get(b"Info") {
        Ok(obj) => obj
            .as_reference()
            .map_err(|e| Error::Manifest(e.to_string()))?,
        Err(_) => return Err(Error::Manifest("no Info dictionary".into())),
    };
    let info = doc
        .get_object(info_id)
        .and_then(|o| o.as_dict())
        .map_err(|e| Error::Manifest(e.to_string()))?;
    let raw = info
        .get(MANIFEST_KEY)
        .and_then(|o| o.as_str())
        .map_err(|e| Error::Manifest(format!("manifest key missing: {e}")))?;
    serde_json::from_slice(raw).map_err(|e| Error::Manifest(e.to_string()))
}
