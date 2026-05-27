//! Extract `/Link` annotations from a PDF.
//!
//! Reads each page's `/Annots` array, filters by `/Subtype = /Link`, and decodes the
//! `/A` action into a `LinkTarget` (URI or page index). Page indices are 0-based to
//! match the rest of the harness; PDF rects are bundled as `[x0, y0, x1, y1]` in PDF
//! default user space (same coordinate system as the manifest's region rects).

use lopdf::{Document, Object, ObjectId};
use serde::Serialize;

#[derive(Debug, Clone)]
pub enum LinkTarget {
    Page(usize),
    Uri(String),
}

impl LinkTarget {
    pub fn as_string(&self) -> String {
        match self {
            Self::Page(p) => format!("page:{p}"),
            Self::Uri(s) => format!("uri:{s}"),
        }
    }
}

impl Serialize for LinkTarget {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.as_string())
    }
}

#[derive(Debug, Clone)]
pub struct RawLink {
    pub page: usize,
    pub rect: [f64; 4],
    pub target: LinkTarget,
}

/// Extract every `/Link` annotation from `pdf_bytes`. Errors during parsing return an
/// empty vec rather than propagating — link extraction is a best-effort observation
/// and shouldn't break describe() if a PDF is unusual.
pub fn extract(pdf_bytes: &[u8]) -> Vec<RawLink> {
    let Ok(doc) = Document::load_mem(pdf_bytes) else {
        return Vec::new();
    };
    let pages = doc.get_pages(); // BTreeMap<u32, ObjectId>, 1-based.
                                 // Map page ObjectId -> 0-based index, for resolving GoTo /D references.
    let page_index: std::collections::HashMap<ObjectId, usize> = pages
        .iter()
        .map(|(num, id)| (*id, (*num as usize).saturating_sub(1)))
        .collect();

    let mut out = Vec::new();
    for (num, page_id) in &pages {
        let page_idx = (*num as usize).saturating_sub(1);
        let Ok(page_dict) = doc.get_object(*page_id).and_then(|o| o.as_dict()) else {
            continue;
        };
        let annots = match page_dict.get(b"Annots") {
            Ok(a) => a,
            Err(_) => continue,
        };
        // /Annots may be an inline array or an indirect reference to one.
        let annots_arr = match annots {
            Object::Array(arr) => arr.clone(),
            Object::Reference(id) => match doc.get_object(*id).and_then(|o| o.as_array()) {
                Ok(a) => a.clone(),
                Err(_) => continue,
            },
            _ => continue,
        };
        for annot in annots_arr {
            let annot_dict = match annot {
                Object::Dictionary(d) => d,
                Object::Reference(id) => match doc.get_object(id).and_then(|o| o.as_dict()) {
                    Ok(d) => d.clone(),
                    Err(_) => continue,
                },
                _ => continue,
            };
            // Subtype == Name("Link")
            match annot_dict.get(b"Subtype") {
                Ok(Object::Name(n)) if n == b"Link" => {}
                _ => continue,
            }
            let Some(rect) = read_rect(annot_dict.get(b"Rect").ok()) else {
                continue;
            };
            // /A action dictionary (possibly indirect).
            let Some(action) = annot_dict
                .get(b"A")
                .ok()
                .and_then(|o| resolve_dict(&doc, o))
            else {
                continue;
            };
            let s = match action.get(b"S") {
                Ok(Object::Name(n)) => n.clone(),
                _ => continue,
            };
            let target = if s == b"URI" {
                let uri = match action.get(b"URI") {
                    Ok(Object::String(bytes, _)) => String::from_utf8_lossy(bytes).into_owned(),
                    _ => continue,
                };
                LinkTarget::Uri(uri)
            } else if s == b"GoTo" {
                let d = match action.get(b"D") {
                    Ok(o) => o,
                    Err(_) => continue,
                };
                // /D is usually an array [pageref, /XYZ ...]; sometimes a named dest (skip).
                let arr = match d {
                    Object::Array(a) => a,
                    _ => continue,
                };
                let Some(Object::Reference(target_id)) = arr.first() else {
                    continue;
                };
                let Some(idx) = page_index.get(target_id) else {
                    continue;
                };
                LinkTarget::Page(*idx)
            } else {
                continue;
            };
            out.push(RawLink {
                page: page_idx,
                rect,
                target,
            });
        }
    }
    out
}

fn read_rect(obj: Option<&Object>) -> Option<[f64; 4]> {
    let arr = match obj? {
        Object::Array(a) => a,
        _ => return None,
    };
    if arr.len() != 4 {
        return None;
    }
    let mut out = [0.0; 4];
    for (i, v) in arr.iter().enumerate() {
        out[i] = as_f64(v)?;
    }
    Some(out)
}

fn as_f64(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(*r as f64),
        _ => None,
    }
}

fn resolve_dict(doc: &Document, obj: &Object) -> Option<lopdf::Dictionary> {
    match obj {
        Object::Dictionary(d) => Some(d.clone()),
        Object::Reference(id) => doc.get_object(*id).ok()?.as_dict().ok().cloned(),
        _ => None,
    }
}
