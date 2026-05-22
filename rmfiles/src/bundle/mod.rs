//! Bundle abstraction for `.rmdoc` zip archives and unpacked directories.
//!
//! A [`Bundle`] can be opened from either a `.rmdoc` zip file or an unpacked
//! directory (identical content, different container). It provides the document
//! metadata, the original source PDF, the per-page scene files, and the device
//! canvas dimensions required to convert `.rm` coordinates to PDF space.

pub mod content;
pub mod metadata;

pub use metadata::Metadata;

use std::collections::HashMap;
use std::io::Read as IoRead;
use std::path::Path;

use crate::{Result, Scene};
use content::Content;

// ---------------------------------------------------------------------------
// Bundle
// ---------------------------------------------------------------------------

/// An opened reMarkable document bundle.
///
/// All file data is read into memory on [`open`][Bundle::open]; subsequent
/// operations are pure in-memory lookups.
pub struct Bundle {
    /// All files keyed by forward-slash relative path, e.g.
    /// `"<uuid>/<page>.rm"` or `"<uuid>.metadata"`.
    files: HashMap<String, Vec<u8>>,

    /// The document UUID (basename of the `*.content` entry).
    uuid: String,

    /// Parsed `.metadata` sidecar.
    meta: Metadata,

    /// Page IDs in reading order (from `.content`).
    page_ids: Vec<String>,

    /// Device canvas dimensions `(width, height)` in pixels.
    canvas: (f64, f64),
}

impl Bundle {
    /// Open a bundle from a `.rmdoc` zip file or an unpacked directory.
    pub fn open(path: &Path) -> Result<Bundle> {
        let files = if path.is_dir() {
            load_dir(path)?
        } else {
            load_zip(path)?
        };

        // Find the UUID from the *.content entry name.
        let uuid = files
            .keys()
            .find(|k| !k.contains('/') && k.ends_with(".content"))
            .map(|k| k.trim_end_matches(".content").to_string())
            .ok_or_else(|| crate::Error::BundleMissing("*.content".into()))?;

        // Parse .content
        let content: Content = match files.get(&format!("{uuid}.content")) {
            Some(bytes) => serde_json::from_slice(bytes)?,
            None => Content::default(),
        };

        // Parse .metadata
        let meta: Metadata = match files.get(&format!("{uuid}.metadata")) {
            Some(bytes) => serde_json::from_slice(bytes)?,
            None => Metadata::default(),
        };

        let page_ids = content.page_ids();

        let canvas = (
            content.page_width.unwrap_or(1404.0),
            content.page_height.unwrap_or(1872.0),
        );

        Ok(Bundle {
            files,
            uuid,
            meta,
            page_ids,
            canvas,
        })
    }

    /// Document metadata (visible name, last-modified timestamp, type).
    pub fn metadata(&self) -> &Metadata {
        &self.meta
    }

    /// Raw bytes of the source PDF, if one is bundled.
    pub fn source_pdf(&self) -> Option<&[u8]> {
        self.files
            .get(&format!("{}.pdf", self.uuid))
            .map(|v| v.as_slice())
    }

    /// Pages in reading order.
    pub fn pages(&self) -> Vec<Page<'_>> {
        self.page_ids
            .iter()
            .enumerate()
            .map(|(index, id)| Page {
                index,
                id: id.clone(),
                bundle: self,
            })
            .collect()
    }

    /// Device canvas size `(width, height)` in pixels.
    ///
    /// Read from `customZoomPageWidth`/`customZoomPageHeight` in `.content`;
    /// defaults to `(1404.0, 1872.0)` when absent.
    pub fn canvas_size(&self) -> (f64, f64) {
        self.canvas
    }
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

/// A single page within a [`Bundle`].
pub struct Page<'a> {
    /// Zero-based page index in reading order.
    pub index: usize,

    /// The page UUID as found in `.content`.
    pub id: String,

    bundle: &'a Bundle,
}

impl Page<'_> {
    /// Parse the `.rm` scene for this page, if one exists.
    ///
    /// Returns `Ok(None)` when no `.rm` file is present for this page (common
    /// for pages that have never been annotated).
    pub fn scene(&self) -> Result<Option<Scene>> {
        let key = format!("{}/{}.rm", self.bundle.uuid, self.id);
        match self.bundle.files.get(&key) {
            Some(bytes) => Scene::parse(bytes).map(Some),
            None => Ok(None),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers: load from zip or directory
// ---------------------------------------------------------------------------

/// Read all entries from a zip archive into a `HashMap<path, bytes>`.
fn load_zip(path: &Path) -> Result<HashMap<String, Vec<u8>>> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut map = HashMap::new();

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        // Skip directory entries.
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        map.insert(name, bytes);
    }

    Ok(map)
}

/// Walk a directory recursively and collect all files into a `HashMap`.
///
/// Keys are forward-slash paths relative to `root`.
fn load_dir(root: &Path) -> Result<HashMap<String, Vec<u8>>> {
    let mut map = HashMap::new();
    collect_dir(root, root, &mut map)?;
    Ok(map)
}

fn collect_dir(root: &Path, dir: &Path, map: &mut HashMap<String, Vec<u8>>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_dir(root, &path, map)?;
        } else {
            // Build a forward-slash relative path.  Use map_err instead of
            // expect so a symlink-escaped path returns an error rather than panicking.
            let rel = path
                .strip_prefix(root)
                .map_err(|_| crate::Error::Parse("path escapes bundle root".into()))?
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            let bytes = std::fs::read(&path)?;
            map.insert(rel, bytes);
        }
    }
    Ok(())
}
