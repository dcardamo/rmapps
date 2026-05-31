//! Document IO: download a doc to `DocFiles`/`Bundle`, upload a new doc, and the
//! ink-preserving content-only PDF swap.

use crate::client::Client;
use crate::error::{Error, Result};
use crate::plumbing::commit::{DocUpsert, Mutation};
use crate::plumbing::index::parse_doc_index;
use crate::plumbing::snapshot::Snapshot;
use crate::porcelain::docfiles::DocFiles;

/// Current time in unix millis as a string (reMarkable's `lastModified` format).
pub(crate) fn now_millis() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

impl Client {
    /// Download a document's full file-set (fetches a fresh snapshot first).
    pub async fn get(&self, id: &str) -> Result<DocFiles> {
        let snap = self.snapshot().await?;
        self.get_from(&snap, id).await
    }

    /// Download a document's file-set against an already-fetched `snap`. Avoids a
    /// redundant root fetch when iterating many docs (used by `ls`/`sync`).
    pub(crate) async fn get_from(&self, snap: &Snapshot, id: &str) -> Result<DocFiles> {
        let doc = snap.doc(id).ok_or(Error::NotFound)?;
        // doc-index blob is keyed by the doc hash, named "<id>.docSchema".
        let index = self.get_blob(&doc.hash, &format!("{id}.docSchema")).await?;
        let entries = parse_doc_index(&index)?;
        let mut files = Vec::with_capacity(entries.len());
        for e in &entries {
            let bytes = self.get_blob(&e.hash, &e.id).await?;
            files.push((e.id.clone(), bytes));
        }
        Ok(DocFiles {
            id: id.to_string(),
            files,
        })
    }

    /// Read just a document's `.metadata` (its doc-index + the one metadata blob),
    /// resolving the doc hash from `snap`. Far cheaper than [`get_from`](Self::get_from)
    /// for `ls`/`sync`, which only need metadata — not the PDF/ink blobs.
    pub(crate) async fn metadata_from(
        &self,
        snap: &Snapshot,
        id: &str,
    ) -> Result<crate::porcelain::docfiles::Metadata> {
        let doc = snap.doc(id).ok_or(Error::NotFound)?;
        self.metadata_by(&doc.hash, id).await
    }

    /// Read a document's `.metadata` given its doc hash directly (no snapshot lookup),
    /// fetching only the doc-index blob and the `.metadata` blob.
    pub(crate) async fn metadata_by(
        &self,
        doc_hash: &str,
        id: &str,
    ) -> Result<crate::porcelain::docfiles::Metadata> {
        let index = self.get_blob(doc_hash, &format!("{id}.docSchema")).await?;
        let entries = parse_doc_index(&index)?;
        let meta = entries
            .iter()
            .find(|e| e.id.ends_with(".metadata"))
            .ok_or_else(|| Error::Parse(format!("doc {id} has no .metadata file")))?;
        let bytes = self.get_blob(&meta.hash, &meta.id).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Download a document and open it as a `rm_files::Bundle` (via a temp `.rmdoc`).
    pub async fn get_bundle(&self, id: &str) -> Result<rm_files::Bundle> {
        let docfiles = self.get(id).await?;
        let dir = tempfile::tempdir()?;
        let path = dir.path().join(format!("{id}.rmdoc"));
        docfiles.write_rmdoc(&path)?;
        rm_files::Bundle::open(&path).map_err(|e| Error::Parse(e.to_string()))
    }

    /// Upload `docfiles` as a document (its `id` is used as-is).
    pub async fn put(&self, docfiles: DocFiles) -> Result<()> {
        let up = DocUpsert {
            id: docfiles.id.clone(),
            files: docfiles.files,
        };
        self.commit(Mutation {
            upserts: vec![up],
            removals: vec![],
        })
        .await?;
        Ok(())
    }

    /// Remove a document.
    pub async fn rm(&self, id: &str) -> Result<()> {
        self.commit(Mutation {
            upserts: vec![],
            removals: vec![id.to_string()],
        })
        .await?;
        Ok(())
    }

    /// Move/rename: edit only the `.metadata` blob (parent and/or visibleName), preserving
    /// every other blob (content/pdf/ink) byte-for-byte.
    pub async fn mv(
        &self,
        id: &str,
        new_parent: Option<&str>,
        new_name: Option<&str>,
    ) -> Result<()> {
        let mut docfiles = self.get(id).await?;
        let mut meta = docfiles.metadata()?;
        if let Some(p) = new_parent {
            meta.parent = p.to_string();
        }
        if let Some(n) = new_name {
            meta.visible_name = n.to_string();
        }
        meta.last_modified = now_millis();
        docfiles.set_metadata(&meta)?;
        self.put(docfiles).await
    }

    /// Ink-preserving PDF swap (mechanics §3): replace only the `<id>.pdf` blob. The
    /// `.content` and every `.rm` blob are left untouched, so on-device ink and page
    /// order survive.
    pub async fn put_content_only(&self, id: &str, new_pdf: Vec<u8>) -> Result<()> {
        let mut docfiles = self.get(id).await?;
        let pdf_name = format!("{id}.pdf");
        let slot = docfiles
            .files
            .iter_mut()
            .find(|(n, _)| *n == pdf_name)
            .ok_or_else(|| Error::Parse("document has no .pdf to replace".into()))?;
        slot.1 = new_pdf;
        self.put(docfiles).await
    }
}
