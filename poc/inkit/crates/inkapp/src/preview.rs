//! Local preview: render the app's document set to PDFs and optionally serve
//! them over HTTP for browser viewing (Tailscale-friendly: binds 0.0.0.0).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use inkapp_core::connector::ConnectorSet;
use inkapp_core::error::{Error, Result};
use inkapp_core::runtime::{App, DocSet};

pub use crate::cli::PreviewArgs;

#[derive(Debug, Clone)]
pub struct RenderedEntry {
    pub key: String,
    pub path: PathBuf,
    pub size_bytes: usize,
    pub page_count: usize,
}

/// Render the app's document set and write each to `<out>/<key>.pdf`.
pub async fn render_to_dir<M, Msg, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    out: &Path,
) -> Result<Vec<RenderedEntry>> {
    std::fs::create_dir_all(out).map_err(|e| Error::Config(format!("preview mkdir: {e}")))?;
    let mut set = DocSet::default();
    let rendered = app.render(&mut set).await?;
    let mut entries = Vec::with_capacity(rendered.len());
    for rd in &rendered {
        let path = out.join(format!("{}.pdf", sanitize_key(&rd.key.0)));
        std::fs::write(&path, &rd.pdf)
            .map_err(|e| Error::Config(format!("preview write: {e}")))?;
        entries.push(RenderedEntry {
            key: rd.key.0.clone(),
            path,
            size_bytes: rd.pdf.len(),
            page_count: rd.page_count,
        });
    }
    Ok(entries)
}

/// Replace path separators in a doc key so it can be a filename.
fn sanitize_key(k: &str) -> String {
    k.replace(['/', '\\'], "_")
}

/// Render to dir and also return an in-memory PDF map keyed by doc key (used by `run`).
#[allow(dead_code)]
pub(crate) async fn render_to_dir_and_map<M, Msg, Cx: ConnectorSet>(
    app: &mut App<M, Msg, Cx>,
    out: &Path,
) -> Result<(Vec<RenderedEntry>, HashMap<String, Vec<u8>>)> {
    std::fs::create_dir_all(out).map_err(|e| Error::Config(format!("preview mkdir: {e}")))?;
    let mut set = DocSet::default();
    let rendered = app.render(&mut set).await?;
    let mut entries = Vec::with_capacity(rendered.len());
    let mut pdfs: HashMap<String, Vec<u8>> = HashMap::new();
    for rd in rendered {
        let path = out.join(format!("{}.pdf", sanitize_key(&rd.key.0)));
        std::fs::write(&path, &rd.pdf)
            .map_err(|e| Error::Config(format!("preview write: {e}")))?;
        entries.push(RenderedEntry {
            key: rd.key.0.clone(),
            path,
            size_bytes: rd.pdf.len(),
            page_count: rd.page_count,
        });
        pdfs.insert(rd.key.0, rd.pdf);
    }
    Ok((entries, pdfs))
}

#[cfg(test)]
mod tests {
    mod fixture {
        use inkapp_core::crypto::Key;
        use inkapp_core::runtime::{app, App};
        use inkapp_readwise_reader::Readwise;
        use reading_queue::{update, view, App as RqApp, Connectors};
        use std::sync::Arc;

        pub fn cassette_app() -> App<RqApp, reading_queue::Msg, Connectors> {
            let connectors = Connectors::from_arc(Arc::new(Readwise::from_cassette()));
            app(RqApp)
                .connector(connectors)
                .update(update)
                .view(view)
                .key(Key::from_bytes([7u8; 32]))
                .build()
        }
    }

    use super::*;

    #[tokio::test]
    async fn render_to_dir_writes_nonempty_pdfs_starting_with_magic() {
        let tmp = tempfile::tempdir().unwrap();
        let mut application = fixture::cassette_app();
        let entries = render_to_dir(&mut application, tmp.path()).await.unwrap();
        assert!(!entries.is_empty(), "cassette must yield at least one doc");
        for e in &entries {
            assert!(e.path.exists(), "{} must exist on disk", e.path.display());
            let bytes = std::fs::read(&e.path).unwrap();
            assert!(!bytes.is_empty(), "{} must be non-empty", e.key);
            assert!(bytes.starts_with(b"%PDF"), "{} must start with %PDF", e.key);
            assert_eq!(bytes.len(), e.size_bytes, "size_bytes matches file");
            assert!(e.page_count >= 1, "{} must have at least one page", e.key);
        }
    }
}
