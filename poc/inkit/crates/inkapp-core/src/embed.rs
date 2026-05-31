use lopdf::{Dictionary, Document, Object, StringFormat};

use crate::crypto::{open, seal, Key};
use crate::error::{Error, Result};
use crate::manifest::Manifest;

/// Info-dictionary key under which the *sealed* manifest is stored.
const MANIFEST_KEY: &[u8] = b"InkappManifest";

/// Seal the manifest and embed it in the PDF's Info dictionary as a hex string.
/// Nothing readable (region names, version) reaches the PDF.
pub fn embed_manifest(pdf: &[u8], manifest: &Manifest, key: &Key) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(manifest).map_err(|e| Error::Manifest(e.to_string()))?;
    let sealed = seal(key, &json)?;
    let mut doc = Document::load_mem(pdf).map_err(|e| Error::Manifest(e.to_string()))?;

    let info_id = match doc.trailer.get(b"Info") {
        Ok(obj) => obj.as_reference().map_err(|_| {
            Error::Manifest("Info trailer entry is not an indirect reference".into())
        })?,
        Err(_) => {
            let id = doc.add_object(Object::Dictionary(Dictionary::new()));
            doc.trailer.set("Info", Object::Reference(id));
            id
        }
    };
    if let Ok(Object::Dictionary(info)) = doc.get_object_mut(info_id) {
        // Hexadecimal string keeps arbitrary ciphertext bytes binary-safe in PDF.
        info.set(
            MANIFEST_KEY,
            Object::String(sealed, StringFormat::Hexadecimal),
        );
    } else {
        return Err(Error::Manifest("Info object is not a dictionary".into()));
    }

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| Error::Manifest(e.to_string()))?;
    Ok(out)
}

/// Extract and open the sealed manifest from the PDF's Info dictionary.
pub fn extract_manifest(pdf: &[u8], key: &Key) -> Result<Manifest> {
    let doc = Document::load_mem(pdf).map_err(|e| Error::Manifest(e.to_string()))?;
    let info_id = match doc.trailer.get(b"Info") {
        Ok(obj) => obj.as_reference().map_err(|_| {
            Error::Manifest("Info trailer entry is not an indirect reference".into())
        })?,
        Err(_) => return Err(Error::Manifest("no Info dictionary".into())),
    };
    let info = doc
        .get_object(info_id)
        .and_then(|o| o.as_dict())
        .map_err(|e| Error::Manifest(e.to_string()))?;
    let sealed = match info.get(MANIFEST_KEY) {
        Ok(Object::String(bytes, _)) => bytes.clone(),
        Ok(_) => return Err(Error::Manifest("manifest key has unexpected type".into())),
        Err(e) => return Err(Error::Manifest(format!("manifest key missing: {e}"))),
    };
    let json = open(key, &sealed)?;
    serde_json::from_slice(&json).map_err(|e| Error::Manifest(e.to_string()))
}
