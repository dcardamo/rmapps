//! reMarkable on-device transport: the `DeviceTransport` impl plus the `rmapi`
//! command seam it shells out through. The load-bearing logic — folder/key
//! mapping, recursive `.rmdoc` discovery, per-key page-height decode — is pure and
//! unit-tested without `rmapi` or a device, via a fake command seam.
//!
//! The real `rmapi` invocations preserve the proven invariants verbatim
//! (remarkable-pdf-mechanics.md §3, §10): always `-ni` with stdin nulled
//! (token-clobber guard); `put --content-only` (PDF-blob-only push, preserving the
//! device ink layer); folder pulls via `mget`; non-recursive `mkdir`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The `rmapi` surface the transport needs — the seam that makes the transport
/// testable without `rmapi` or a device.
pub trait RmCommand {
    /// Create `folder` (best-effort; non-recursive — create ancestors separately).
    fn mkdir(&self, folder: &str);
    /// Push `local_pdf` into `folder`, swapping only the PDF blob (content-only).
    fn put_content_only(&self, local_pdf: &Path, folder: &str) -> inkapp_core::error::Result<()>;
    /// Delete `remote_path` (best-effort; a missing document is fine).
    fn rm(&self, remote_path: &str);
    /// Pull `folder` recursively into `into_dir`. Returns false on failure.
    fn mget(&self, folder: &str, into_dir: &Path) -> bool;
}

/// A discovered on-device document pulled to disk: its key, the `.rmdoc` path, and
/// the page height to decode its ink at.
pub(crate) struct Discovered {
    pub key: String,
    pub path: PathBuf,
    pub page_h: f64,
}

/// Recursively collect `*.rmdoc` files under `dir` (mget nests downloads under a
/// subdir named after the remote folder, so we walk rather than assume flat).
pub(crate) fn find_rmdocs(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(dir, &mut out);
    out
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "rmdoc") {
            out.push(p);
        }
    }
}

/// Map each discovered `.rmdoc` to (key, path, page_h): the filename stem is the
/// key (we push `<key>.pdf`), decoded with that key's page height (0.0 if unknown).
pub(crate) fn discover(dir: &Path, page_h_by_key: &HashMap<String, f64>) -> Vec<Discovered> {
    let mut out = Vec::new();
    for path in find_rmdocs(dir) {
        let Some(key) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        let page_h = page_h_by_key.get(&key).copied().unwrap_or(0.0);
        out.push(Discovered { key, path, page_h });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_rmdocs_recurses_and_filters() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("ReadingQueue");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("a.rmdoc"), b"x").unwrap();
        std::fs::write(nested.join("b.rmdoc"), b"y").unwrap();
        std::fs::write(nested.join("notes.txt"), b"z").unwrap();
        let mut found: Vec<String> = find_rmdocs(dir.path())
            .iter()
            .filter_map(|p| p.file_name().and_then(|s| s.to_str()).map(str::to_string))
            .collect();
        found.sort();
        assert_eq!(found, vec!["a.rmdoc".to_string(), "b.rmdoc".to_string()]);
    }

    #[test]
    fn discover_maps_stem_to_key_and_page_height() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("Agenda");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("article-7.rmdoc"), b"x").unwrap();
        std::fs::write(nested.join("orphan.rmdoc"), b"y").unwrap();
        let mut page_h = HashMap::new();
        page_h.insert("article-7".to_string(), 560.0);
        let got = discover(dir.path(), &page_h);
        let by_key: HashMap<&str, f64> = got.iter().map(|d| (d.key.as_str(), d.page_h)).collect();
        assert_eq!(by_key.get("article-7"), Some(&560.0));
        assert_eq!(by_key.get("orphan"), Some(&0.0));
    }
}
