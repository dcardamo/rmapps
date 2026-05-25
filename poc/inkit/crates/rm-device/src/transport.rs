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
use std::process::{Command, Stdio};

use inkapp_core::device::Device;
use inkapp_core::error::{Error, Result};
use inkapp_core::ink::Stroke;
use inkapp_core::sync::DeviceTransport;
use rm_files::Bundle;

use crate::Remarkable;

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

/// The real seam: shells out to the `rmapi` CLI.
pub struct Rmapi;

impl RmCommand for Rmapi {
    fn mkdir(&self, folder: &str) {
        let _ = Command::new("rmapi")
            .args(["-ni", "mkdir", folder])
            .stdin(Stdio::null())
            .status();
    }

    fn put_content_only(&self, local_pdf: &Path, folder: &str) -> Result<()> {
        let path = local_pdf
            .to_str()
            .ok_or_else(|| Error::Transport("non-utf8 pdf path".into()))?;
        let ok = Command::new("rmapi")
            .args(["-ni", "put", "--content-only", path, folder])
            .stdin(Stdio::null())
            .status()
            .map_err(|e| Error::Transport(format!("rmapi put: {e}")))?
            .success();
        if ok {
            Ok(())
        } else {
            Err(Error::Transport(format!("rmapi put failed for {path}")))
        }
    }

    fn rm(&self, remote_path: &str) {
        let _ = Command::new("rmapi")
            .args(["-ni", "rm", remote_path])
            .stdin(Stdio::null())
            .status();
    }

    fn mget(&self, folder: &str, into_dir: &Path) -> bool {
        Command::new("rmapi")
            .args(["-ni", "mget", folder])
            .current_dir(into_dir)
            .stdin(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// Assemble per-page PDF-space strokes from an `.rmdoc` bundle, indexed by the
/// bundle's `.content` page order: slot `p` aligns with the manifest's
/// `region.page == p`. An un-inked page occupies its slot as an empty `Vec`, so it
/// never shifts later pages. All pages of a document share one `page_h` (Typst
/// `#set page` fixes every page to the same height). Empty if the bundle won't open.
pub fn strokes_by_page(device: &Remarkable, path: &Path, page_h: f64) -> Vec<Vec<Stroke>> {
    let Ok(bundle) = Bundle::open(path) else {
        return Vec::new();
    };
    bundle
        .pages()
        .into_iter()
        .map(|pg| match pg.scene_bytes() {
            Some(bytes) => device.read_ink(bytes, page_h).unwrap_or_default(),
            None => Vec::new(),
        })
        .collect()
}

/// reMarkable transport: maps the framework's key/PDF/ink model onto `rmapi` and
/// the `.rmdoc` zip format. Generic over the command seam so tests inject a fake.
pub struct RmTransport<C: RmCommand = Rmapi> {
    folder: String,
    device: Remarkable,
    cmd: C,
}

impl RmTransport<Rmapi> {
    /// A transport that shells out to the real `rmapi`, deploying under `folder`.
    pub fn new(folder: impl Into<String>) -> Self {
        Self::with_command(Rmapi, folder)
    }
}

impl<C: RmCommand> RmTransport<C> {
    /// A transport over an explicit command seam (tests pass a fake).
    pub fn with_command(cmd: C, folder: impl Into<String>) -> Self {
        Self {
            folder: folder.into(),
            device: Remarkable::new(),
            cmd,
        }
    }
}

impl<C: RmCommand> DeviceTransport for RmTransport<C> {
    fn push(&self, key: &str, pdf: &[u8]) -> Result<()> {
        self.cmd.mkdir(&self.folder);
        // The on-device visibleName is the file stem, so name the temp file <key>.pdf.
        let tmp = std::env::temp_dir().join(format!("{key}.pdf"));
        std::fs::write(&tmp, pdf).map_err(|e| Error::Transport(format!("write {key}.pdf: {e}")))?;
        self.cmd.put_content_only(&tmp, &self.folder)
    }

    fn delete(&self, key: &str) {
        self.cmd.rm(&format!("{}/{}", self.folder, key));
    }

    fn pull(&self, page_h_by_key: &HashMap<String, f64>) -> HashMap<String, Vec<Vec<Stroke>>> {
        let mut out = HashMap::new();
        let Ok(dir) = tempfile::tempdir() else {
            return out;
        };
        if !self.cmd.mget(&self.folder, dir.path()) {
            return out;
        }
        for d in discover(dir.path(), page_h_by_key) {
            let pages = strokes_by_page(&self.device, &d.path, d.page_h);
            // Insert only when the document carries ink on some page.
            if pages.iter().any(|pg| !pg.is_empty()) {
                out.insert(d.key, pages);
            }
        }
        out
    }
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

    use super::RmTransport;
    use inkapp_core::geometry::PdfPoint;
    use inkapp_core::ink::Stroke;
    use std::path::Path;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeRm {
        puts: Mutex<Vec<(String, String)>>, // (file_name, folder)
        rms: Mutex<Vec<String>>,
        mget_doc: Option<(String, Vec<u8>)>, // (key, .rm bytes) materialized on mget
    }

    impl RmCommand for FakeRm {
        fn mkdir(&self, _folder: &str) {}
        fn put_content_only(
            &self,
            local_pdf: &Path,
            folder: &str,
        ) -> inkapp_core::error::Result<()> {
            let name = local_pdf.file_name().unwrap().to_str().unwrap().to_string();
            self.puts.lock().unwrap().push((name, folder.to_string()));
            Ok(())
        }
        fn rm(&self, remote_path: &str) {
            self.rms.lock().unwrap().push(remote_path.to_string());
        }
        fn mget(&self, _folder: &str, into_dir: &Path) -> bool {
            if let Some((key, rm_bytes)) = &self.mget_doc {
                // Mimic mget's nested layout AND the `.rmdoc` bundle format
                // `Bundle::open` expects: a `<uuid>.content` page list plus a
                // `<uuid>/<page>.rm` for each inked page.
                use std::io::Write;
                let nested = into_dir.join("RemoteFolder");
                std::fs::create_dir_all(&nested).unwrap();
                let f = std::fs::File::create(nested.join(format!("{key}.rmdoc"))).unwrap();
                let mut zw = zip::ZipWriter::new(f);
                let opts = zip::write::SimpleFileOptions::default();
                zw.start_file("doc.content", opts).unwrap();
                zw.write_all(br#"{"cPages":{"pages":[{"id":"p0"}]}}"#)
                    .unwrap();
                zw.start_file("doc/p0.rm", opts).unwrap();
                zw.write_all(rm_bytes).unwrap();
                zw.finish().unwrap();
            }
            true
        }
    }

    #[test]
    fn push_maps_key_to_pdf_named_under_folder() {
        let t = RmTransport::with_command(FakeRm::default(), "/ReadingQueue");
        t.push("article-7", b"%PDF fake bytes").unwrap();
        let puts = t.cmd.puts.lock().unwrap();
        assert_eq!(puts.len(), 1);
        assert_eq!(puts[0].0, "article-7.pdf");
        assert_eq!(puts[0].1, "/ReadingQueue");
    }

    #[test]
    fn delete_targets_folder_slash_key() {
        let t = RmTransport::with_command(FakeRm::default(), "/Agenda");
        t.delete("event-3");
        assert_eq!(
            *t.cmd.rms.lock().unwrap(),
            vec!["/Agenda/event-3".to_string()]
        );
    }

    #[test]
    fn pull_decodes_ink_under_the_right_key() {
        // Build real .rm bytes for one stroke via the transform's inverse. We do
        // not assert coordinates (the harness already proves the transform); we
        // assert the ink maps back to its key at the requested page height.
        let device = Remarkable::new();
        let page_h = 560.0;
        let strokes = vec![Stroke {
            points: vec![
                PdfPoint { x: 100.0, y: 200.0 },
                PdfPoint { x: 150.0, y: 220.0 },
            ],
            highlighter: false,
        }];
        let rm_bytes = device.write_ink(&strokes, page_h).unwrap();

        let fake = FakeRm {
            mget_doc: Some(("article-7".to_string(), rm_bytes)),
            ..FakeRm::default()
        };
        let t = RmTransport::with_command(fake, "/ReadingQueue");

        let mut page_h_by_key = HashMap::new();
        page_h_by_key.insert("article-7".to_string(), page_h);
        let ink = t.pull(&page_h_by_key);

        assert!(ink.contains_key("article-7"), "ink mapped back to its key");
        assert_eq!(ink["article-7"].len(), 1, "single page");
        assert_eq!(ink["article-7"][0].len(), 1, "one stroke round-tripped");
    }
}
