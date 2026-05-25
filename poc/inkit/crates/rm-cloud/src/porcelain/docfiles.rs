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
