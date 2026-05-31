//! Embed a self-describing manifest inside the generated PDF so the downloaded
//! bundle is the single source of truth (no local manifest state). Stored as a
//! Flate-compressed stream referenced by a custom Catalog key.
use crate::manifest::EmbeddedManifest;
use lopdf::{Dictionary, Document, Object, Stream};

const CATALOG_KEY: &[u8] = b"RMReaderManifest";

pub fn write(doc: &mut Document, manifest: &EmbeddedManifest) -> anyhow::Result<()> {
    let json = serde_json::to_vec(manifest)?;
    let mut stream = Stream::new(Dictionary::new(), json);
    stream.compress()?; // Flate
    let sid = doc.add_object(Object::Stream(stream));
    let catalog_id = doc.trailer.get(b"Root")?.as_reference()?;
    let catalog = doc.get_dictionary_mut(catalog_id)?;
    catalog.set(CATALOG_KEY, Object::Reference(sid));
    Ok(())
}

pub fn read(doc: &Document) -> anyhow::Result<Option<EmbeddedManifest>> {
    let catalog_id = doc.trailer.get(b"Root")?.as_reference()?;
    let catalog = doc.get_dictionary(catalog_id)?;
    let sid = match catalog.get(CATALOG_KEY) {
        Ok(Object::Reference(id)) => *id,
        _ => return Ok(None),
    };
    let stream = doc.get_object(sid)?.as_stream()?;
    let bytes = stream.decompressed_content()?;
    Ok(Some(serde_json::from_slice(&bytes)?))
}
