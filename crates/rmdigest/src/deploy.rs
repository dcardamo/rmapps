//! Deploy backend abstraction for rmdigest.
//!
//! A [`Backend`] lists candidate source documents, fetches a document's bundle
//! (PDF + any ink), and writes the generated digest PDF back. The library ships
//! [`LocalBackend`] (filesystem, for tests + `--local`); the cloud backend lives
//! in the `rmapps` binary and implements [`Backend`] over the native client.

use std::path::{Path, PathBuf};

use anyhow::Result;

/// A reMarkable document discovered by a [`Backend`].
#[derive(Debug, Clone, PartialEq)]
pub struct CloudDoc {
    /// Stable id (cloud: visible path; local: relative path).
    pub path: String,
    /// Visible document name (no extension).
    pub name: String,
    /// Parent folder (cloud path or local dir).
    pub folder: String,
    /// Opaque version/hash, if the backend exposes one.
    pub version: Option<String>,
}

/// Abstraction over "where the documents live".
pub trait Backend {
    /// List candidate documents under `root` (excluding generated digests).
    fn list(&self, root: &str, exclude_suffixes: &[String]) -> Result<Vec<CloudDoc>>;
    /// Download a document's bundle to a local temp dir; returns the bundle path.
    fn fetch(&self, doc: &CloudDoc) -> Result<Option<PathBuf>>;
    /// Write `pdf` back as a sibling named `name` in `folder`.
    fn put(&self, pdf: &Path, folder: &str, name: &str) -> Result<()>;
}

/// Filesystem backend: documents are `.pdf` files under a root dir.
#[derive(Debug, Default)]
pub struct LocalBackend;

impl LocalBackend {
    fn list_dir(root: &Path, exclude: &[String], out: &mut Vec<CloudDoc>) -> Result<()> {
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                Self::list_dir(&path, exclude, out)?;
            } else if path.extension().and_then(|e| e.to_str()) == Some("pdf") {
                let name = path.file_stem().unwrap().to_string_lossy().to_string();
                if exclude.iter().any(|s| name.ends_with(s)) {
                    continue;
                }
                let folder = path.parent().unwrap().to_string_lossy().to_string();
                out.push(CloudDoc {
                    path: path.to_string_lossy().to_string(),
                    name,
                    folder,
                    version: None,
                });
            }
        }
        Ok(())
    }
}

impl Backend for LocalBackend {
    fn list(&self, root: &str, exclude_suffixes: &[String]) -> Result<Vec<CloudDoc>> {
        let mut out = Vec::new();
        let root_path = Path::new(root);
        if root_path.is_dir() {
            Self::list_dir(root_path, exclude_suffixes, &mut out)?;
        }
        Ok(out)
    }
    fn fetch(&self, doc: &CloudDoc) -> Result<Option<PathBuf>> {
        let p = PathBuf::from(&doc.path);
        if p.exists() {
            Ok(Some(p))
        } else {
            Ok(None)
        }
    }
    fn put(&self, pdf: &Path, folder: &str, name: &str) -> Result<()> {
        let dest_dir = Path::new(folder);
        std::fs::create_dir_all(dest_dir)?;
        let dest = dest_dir.join(format!("{name}.pdf"));
        std::fs::copy(pdf, &dest)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_backend_lists_pdfs_excluding_suffixes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("a.pdf"), b"x").unwrap();
        std::fs::write(root.join("b.digest.pdf"), b"x").unwrap();
        let backend = LocalBackend;
        let docs = backend
            .list(&root.to_string_lossy(), &["digest".to_string()])
            .unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].name, "a");
    }

    #[test]
    fn local_backend_put_copies_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.pdf");
        std::fs::write(&src, b"pdf").unwrap();
        let backend = LocalBackend;
        backend
            .put(&src, &dir.path().to_string_lossy(), "out")
            .unwrap();
        assert!(dir.path().join("out.pdf").exists());
    }
}
