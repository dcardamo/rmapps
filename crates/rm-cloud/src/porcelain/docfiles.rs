//! `DocFiles` — a document as the cloud stores it: logical name -> bytes, plus the
//! metadata/content JSON. Converts to/from a `.rmdoc` zip for `rm_files::Bundle`.

use std::io::{Cursor, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// reMarkable document metadata (`<id>.metadata`). Only the fields we read/write.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Metadata {
    /// User-visible document name.
    #[serde(rename = "visibleName")]
    pub visible_name: String,
    /// "DocumentType" | "CollectionType".
    #[serde(rename = "type")]
    pub doc_type: String,
    /// Parent folder id ("" = root, "trash" = trash).
    pub parent: String,
    /// Last-modified unix-millis string.
    #[serde(rename = "lastModified")]
    pub last_modified: String,
    /// Soft-delete flag.
    #[serde(default)]
    pub deleted: bool,
    /// Any other metadata fields (round-tripped verbatim).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A document's full file-set (logical name -> bytes).
#[derive(Debug, Clone, Default)]
pub struct DocFiles {
    /// Document UUID.
    pub id: String,
    /// All files keyed by logical name, e.g. `<id>.metadata`, `<id>.pdf`, `<id>/<page>.rm`.
    pub files: Vec<(String, Vec<u8>)>,
}

impl DocFiles {
    /// Build a brand-new PDF document file-set: a fresh UUID, a `DocumentType`
    /// `.metadata` (named `visible_name`, under `parent`), a full PDF `.content`,
    /// and the `.pdf` blob. No `.rm` ink yet — the device adds those when the user
    /// writes. Later background swaps go through
    /// [`Client::put_content_only`](crate::Client::put_content_only), which
    /// preserves that ink (mechanics §1, §3).
    ///
    /// The `.content` carries an explicit page list: `pageCount`/`originalPageCount`
    /// (from the PDF's actual page count, via `lopdf`), a `pages` array of fresh
    /// per-page UUIDs, and a matching `redirectionPageMap`. This is the firmware's
    /// legacy flat schema — writing it ourselves means the device shows the pages
    /// immediately, without having to parse the whole PDF (which fails for large
    /// image-heavy PDFs). If the PDF can't be parsed (page count 0) we fall back to
    /// the minimal `{"fileType":"pdf","formatVersion":1}` content.
    ///
    /// Generating the page UUIDs here is correct: the reader deploys via a
    /// destructive `replace` (a fresh doc each run), and for bujo's content-only
    /// path the `.content` is written once at create and preserved on refresh.
    pub fn new_pdf(visible_name: &str, parent: &str, pdf: Vec<u8>) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let meta = Metadata {
            visible_name: visible_name.to_string(),
            doc_type: "DocumentType".to_string(),
            parent: parent.to_string(),
            last_modified: super::document::now_millis(),
            deleted: false,
            extra: Default::default(),
        };
        // Count pages straight from the PDF bytes. 0 => unparseable; fall back.
        let n = lopdf::Document::load_mem(&pdf)
            .map(|d| d.get_pages().len())
            .unwrap_or(0);
        let content = if n > 0 {
            let pages: Vec<String> = (0..n).map(|_| uuid::Uuid::new_v4().to_string()).collect();
            let redirection_page_map: Vec<i64> = (0..n as i64).collect();
            serde_json::to_vec(&serde_json::json!({
                "fileType": "pdf",
                "formatVersion": 1,
                "pageCount": n,
                "originalPageCount": n,
                "pages": pages,
                "redirectionPageMap": redirection_page_map,
                "sizeInBytes": pdf.len().to_string(),
                "coverPageNumber": 0,
                "orientation": "portrait",
                "textScale": 1,
                "textAlignment": "justify",
                "zoomMode": "bestFit",
                "lineHeight": -1,
                "fontName": "",
                "customZoomCenterX": 0,
                "customZoomCenterY": 936,
                "customZoomOrientation": "portrait",
                "customZoomPageHeight": 1872,
                "customZoomPageWidth": 1404,
                "customZoomScale": 1,
                "tags": [],
                "pageTags": [],
                "documentMetadata": {},
                "extraMetadata": {},
            }))
            .expect("serialize content")
        } else {
            br#"{"fileType":"pdf","formatVersion":1}"#.to_vec()
        };
        let files = vec![
            (
                format!("{id}.metadata"),
                serde_json::to_vec(&meta).expect("serialize metadata"),
            ),
            (format!("{id}.content"), content),
            (format!("{id}.pdf"), pdf),
        ];
        Self { id, files }
    }

    /// Get a file's bytes by logical name.
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.files
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, b)| b.as_slice())
    }

    /// Parse the `<id>.metadata` blob.
    pub fn metadata(&self) -> Result<Metadata> {
        let raw = self
            .get(&format!("{}.metadata", self.id))
            .ok_or_else(|| Error::Parse("missing .metadata".into()))?;
        Ok(serde_json::from_slice(raw)?)
    }

    /// Replace the `<id>.metadata` blob with `meta`.
    pub fn set_metadata(&mut self, meta: &Metadata) -> Result<()> {
        let name = format!("{}.metadata", self.id);
        let bytes = serde_json::to_vec(meta)?;
        if let Some(slot) = self.files.iter_mut().find(|(n, _)| *n == name) {
            slot.1 = bytes;
        } else {
            self.files.push((name, bytes));
        }
        Ok(())
    }

    /// Write a `.rmdoc` zip to `path` (openable by `rm_files::Bundle::open`).
    pub fn write_rmdoc(&self, path: &Path) -> Result<()> {
        let file = std::fs::File::create(path)?;
        let mut zip = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in &self.files {
            zip.start_file(name, opts)
                .map_err(|e| Error::Parse(e.to_string()))?;
            zip.write_all(bytes)?;
        }
        zip.finish().map_err(|e| Error::Parse(e.to_string()))?;
        Ok(())
    }

    /// Build a `DocFiles` from a `.rmdoc` zip on disk.
    pub fn from_rmdoc(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        Self::from_zip_bytes(&bytes)
    }

    /// Build a `DocFiles` from in-memory zip bytes.
    pub fn from_zip_bytes(bytes: &[u8]) -> Result<Self> {
        use std::io::Read;
        let mut zip =
            zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| Error::Parse(e.to_string()))?;
        let mut files = Vec::new();
        let mut id = String::new();
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(|e| Error::Parse(e.to_string()))?;
            let name = f.name().to_string();
            if let Some(stem) = name.strip_suffix(".content") {
                id = stem.to_string();
            }
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            files.push((name, buf));
        }
        if id.is_empty() {
            return Err(Error::Parse(
                "rmdoc has no .content entry (cannot determine id)".into(),
            ));
        }
        Ok(Self { id, files })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid PDF with `pages` empty pages, via lopdf.
    fn build_pdf(pages: usize) -> Vec<u8> {
        use lopdf::dictionary;
        use lopdf::{Document, Object, ObjectId};

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let kids: Vec<Object> = (0..pages)
            .map(|_| {
                let page_id = doc.add_object(dictionary! {
                    "Type" => "Page",
                    "Parent" => pages_id,
                    "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                });
                Object::Reference(page_id)
            })
            .collect();
        let kid_ids: Vec<ObjectId> = kids
            .iter()
            .filter_map(|o| match o {
                Object::Reference(id) => Some(*id),
                _ => None,
            })
            .collect();
        let pages_dict = dictionary! {
            "Type" => "Pages",
            "Count" => Object::Integer(pages as i64),
            "Kids" => kids,
        };
        doc.objects.insert(pages_id, Object::Dictionary(pages_dict));
        // Touch kid_ids so unused-var lint stays quiet across lopdf versions.
        let _ = kid_ids;
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);

        let mut buf = Vec::new();
        doc.save_to(&mut buf).expect("save pdf");
        buf
    }

    #[test]
    fn new_pdf_writes_full_page_list() {
        let pdf = build_pdf(2);
        let docs = DocFiles::new_pdf("Test", "", pdf.clone());
        let content_raw = docs
            .get(&format!("{}.content", docs.id))
            .expect("has .content");
        let content: serde_json::Value =
            serde_json::from_slice(content_raw).expect("valid content json");

        assert_eq!(content["fileType"], "pdf");
        let page_count = content["pageCount"].as_u64().expect("pageCount");
        assert_eq!(page_count, 2);
        let pages_len = content["pages"].as_array().expect("pages array").len() as u64;
        let map_len = content["redirectionPageMap"]
            .as_array()
            .expect("redirectionPageMap array")
            .len() as u64;
        assert_eq!(page_count, pages_len);
        assert_eq!(page_count, map_len);
        // sizeInBytes is a string of the byte length.
        assert_eq!(content["sizeInBytes"], pdf.len().to_string());
    }

    #[test]
    fn new_pdf_one_page() {
        let pdf = build_pdf(1);
        let docs = DocFiles::new_pdf("One", "", pdf);
        let content_raw = docs.get(&format!("{}.content", docs.id)).unwrap();
        let content: serde_json::Value = serde_json::from_slice(content_raw).unwrap();
        assert_eq!(content["pageCount"].as_u64(), Some(1));
    }

    #[test]
    fn new_pdf_unparseable_falls_back() {
        let docs = DocFiles::new_pdf("Bad", "", b"not a pdf".to_vec());
        let content_raw = docs.get(&format!("{}.content", docs.id)).unwrap();
        let content: serde_json::Value = serde_json::from_slice(content_raw).unwrap();
        assert_eq!(content["fileType"], "pdf");
        assert_eq!(content["formatVersion"].as_u64(), Some(1));
        // Fallback content has no page list.
        assert!(content.get("pageCount").is_none());
    }
}
