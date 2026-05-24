//! The MVU loop runtime: the render walk (`render_document`) and the multi-cycle
//! driver (`App`, `DocSet`, `step`).

use crate::crypto::Key;
use crate::document::{DocKey, Document};
use crate::embed::embed_manifest;
use crate::error::Result;
use crate::manifest::{recover_regions, Manifest};
use crate::render::{compile_to_document, document_to_pdf};
use crate::widget::RenderCx;

/// Default document page geometry (points). 3:4-ish to suit e-ink; the device
/// fits to width. Single-page only this spec.
pub const DOC_PAGE_W: f64 = 420.0;
pub const DOC_PAGE_H: f64 = 560.0;

/// A rendered document: its PDF (manifest embedded), the recovered manifest, the
/// page height (for the device transform), and a content hash (for reconcile).
pub struct RenderedDoc {
    pub key: DocKey,
    pub pdf: Vec<u8>,
    pub manifest: Manifest,
    pub page_h: f64,
    pub hash: u64,
}

/// Assemble a document's Typst source: a page header plus each component's render
/// in flow order.
pub fn document_source<M>(doc: &Document<M>) -> String {
    let mut cx = RenderCx::new(0);
    let mut src = format!(
        "#set page(width: {DOC_PAGE_W}pt, height: {DOC_PAGE_H}pt, margin: 16pt)\n#set text(size: 12pt)\n"
    );
    for c in &doc.flow {
        src.push_str(&c.render(&mut cx));
    }
    src
}

/// Stable hash of a string (std DefaultHasher is deterministic within a build,
/// which is all reconcile needs — equal source -> equal hash).
fn hash_str(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// Render one document to a [`RenderedDoc`] at `version`, sealing its manifest
/// with `key`.
pub fn render_document<M>(doc: &Document<M>, version: u64, key: &Key) -> Result<RenderedDoc> {
    let src = document_source(doc);
    let compiled = compile_to_document(&src)?;
    let page_h = compiled
        .pages
        .first()
        .map(|p| p.frame.height().to_pt())
        .unwrap_or(0.0);
    let manifest = recover_regions(&compiled)?.with_version(version);
    let pdf = embed_manifest(&document_to_pdf(&compiled)?, &manifest, key)?;
    Ok(RenderedDoc {
        key: doc.key.clone(),
        pdf,
        manifest,
        page_h,
        hash: hash_str(&src),
    })
}

// ---- Multi-cycle loop driver ------------------------------------------------

use std::collections::HashMap;

use crate::document::Documents;
use crate::ink::Stroke;
use crate::readback::{attribute, guard_version};
use crate::reconcile::{reconcile, DocOp};

/// Per-key state the framework carries between cycles.
struct DocEntry {
    manifest: Manifest,
    page_h: f64,
    hash: u64,
    version: u64,
    /// Accumulated user ink (PDF space) on this document — preserved across
    /// re-renders by key.
    ink: Vec<Stroke>,
}

/// The framework's view of the device's document set, keyed by `DocKey`.
#[derive(Default)]
pub struct DocSet {
    entries: HashMap<String, DocEntry>,
}

impl DocSet {
    /// The manifest of the document last rendered for `key`.
    pub fn manifest(&self, key: &DocKey) -> Option<&Manifest> {
        self.entries.get(&key.0).map(|e| &e.manifest)
    }

    /// The page height (points) last used for `key`.
    pub fn page_h(&self, key: &DocKey) -> Option<f64> {
        self.entries.get(&key.0).map(|e| e.page_h)
    }

    /// The preserved ink on `key` (empty if none / unknown).
    pub fn ink(&self, key: &DocKey) -> &[Stroke] {
        self.entries
            .get(&key.0)
            .map(|e| e.ink.as_slice())
            .unwrap_or(&[])
    }

    /// All keys currently in the set.
    pub fn keys(&self) -> Vec<DocKey> {
        self.entries.keys().cloned().map(DocKey).collect()
    }
}

type UpdateFn<M, Msg, Cx> = fn(Msg, &mut M, &Cx);
type ViewFn<M, Msg, Cx> = fn(&M, &Cx) -> Documents<Msg>;

/// The result of one `step`.
pub struct Cycle<Msg> {
    /// Messages decoded from this cycle's ink (before folding).
    pub decoded: Vec<Msg>,
    /// The reconciliation ops applied to the document set.
    pub ops: Vec<DocOp>,
    /// The documents that were created or updated (to push to the device).
    pub rendered: Vec<RenderedDoc>,
}

/// An assembled MVU app: owned model + connectors, plus the `update`/`view`
/// functions. `M` = model, `Msg` = message, `Cx` = the app's connectors struct.
pub struct App<M, Msg, Cx> {
    pub model: M,
    pub connectors: Cx,
    update: UpdateFn<M, Msg, Cx>,
    view: ViewFn<M, Msg, Cx>,
    version: u64,
    key: Key,
}

impl<M, Msg, Cx> App<M, Msg, Cx> {
    pub fn new(
        model: M,
        connectors: Cx,
        update: UpdateFn<M, Msg, Cx>,
        view: ViewFn<M, Msg, Cx>,
        key: Key,
    ) -> Self {
        Self {
            model,
            connectors,
            update,
            view,
            version: 1,
            key,
        }
    }

    /// Render the full document set from current state, (re)populating `set`.
    pub fn render(&mut self, set: &mut DocSet) -> Result<Vec<RenderedDoc>> {
        let docs = (self.view)(&self.model, &self.connectors);
        let mut out = Vec::new();
        let mut entries = HashMap::new();
        for doc in &docs.0 {
            let rd = render_document(doc, self.version, &self.key)?;
            entries.insert(
                rd.key.0.clone(),
                DocEntry {
                    manifest: rd.manifest.clone(),
                    page_h: rd.page_h,
                    hash: rd.hash,
                    version: self.version,
                    ink: Vec::new(),
                },
            );
            out.push(rd);
        }
        set.entries = entries;
        Ok(out)
    }

    /// One loop cycle: decode `ink_by_key` (pre-fold view + stored manifest),
    /// fold the messages, re-render, reconcile, and update `set` (preserving ink
    /// on surviving keys and appending this cycle's input ink).
    pub fn step(
        &mut self,
        set: &mut DocSet,
        ink_by_key: &HashMap<String, Vec<Stroke>>,
    ) -> Result<Cycle<Msg>>
    where
        Msg: Clone,
    {
        // 1. Decode against the pre-fold trees + the stored manifests.
        let pre = (self.view)(&self.model, &self.connectors);
        let mut decoded: Vec<Msg> = Vec::new();
        for doc in &pre.0 {
            let Some(strokes) = ink_by_key.get(&doc.key.0) else {
                continue;
            };
            let Some(entry) = set.entries.get(&doc.key.0) else {
                continue;
            };
            // Entries are version-stamped at render time, so this check is
            // structural today (entry.version == entry.manifest.version by
            // construction). It reserves the call site for the future path where
            // ink carries its own base version (multi-device / vector clock).
            guard_version(entry.version, &entry.manifest)?;
            let region_ink = attribute(strokes, &entry.manifest);
            for c in &doc.flow {
                decoded.extend(c.decode(&region_ink, &entry.manifest));
            }
        }

        // 2. Bump version, then fold each message through update. The version
        //    stamps the post-fold render in phase 3.
        self.version += 1;
        // Clone each message to fold; `decoded` itself is moved into the
        // returned `Cycle` for the caller to inspect.
        for m in decoded.iter().cloned() {
            (self.update)(m, &mut self.model, &self.connectors);
        }

        // 3. Re-render the post-fold view.
        let next = (self.view)(&self.model, &self.connectors);
        let mut next_rendered: Vec<RenderedDoc> = Vec::new();
        for doc in &next.0 {
            next_rendered.push(render_document(doc, self.version, &self.key)?);
        }

        // 4. Reconcile by key against the prior set.
        let prev: Vec<(DocKey, u64)> = set
            .entries
            .iter()
            .map(|(k, e)| (DocKey(k.clone()), e.hash))
            .collect();
        let next_pairs: Vec<(DocKey, u64)> = next_rendered
            .iter()
            .map(|rd| (rd.key.clone(), rd.hash))
            .collect();
        let ops = reconcile(&prev, &next_pairs);

        // 5. Apply: build the new entry map, preserving ink on survivors and
        //    appending this cycle's input ink. Collect created/updated for push.
        let changed: HashMap<&str, ()> = ops
            .iter()
            .filter_map(|o| match o {
                DocOp::Create(k) | DocOp::Update(k) => Some((k.0.as_str(), ())),
                DocOp::Delete(_) => None,
            })
            .collect();
        let mut new_entries: HashMap<String, DocEntry> = HashMap::new();
        let mut rendered_out: Vec<RenderedDoc> = Vec::new();

        for rd in next_rendered {
            // Preserve prior ink for this key, then append this cycle's input.
            let mut ink = set
                .entries
                .get(&rd.key.0)
                .map(|e| e.ink.clone())
                .unwrap_or_default();
            if let Some(new_ink) = ink_by_key.get(&rd.key.0) {
                ink.extend(new_ink.iter().cloned());
            }
            let is_changed = changed.contains_key(rd.key.0.as_str());
            new_entries.insert(
                rd.key.0.clone(),
                DocEntry {
                    manifest: rd.manifest.clone(),
                    page_h: rd.page_h,
                    hash: rd.hash,
                    version: self.version,
                    ink,
                },
            );
            if is_changed {
                rendered_out.push(rd);
            }
        }
        set.entries = new_entries;

        Ok(Cycle {
            decoded,
            ops,
            rendered: rendered_out,
        })
    }
}

/// Builder entry point: `app(model).connector(cx).update(f).view(g).build()`.
pub fn app<M>(model: M) -> Builder<M> {
    Builder { model }
}

pub struct Builder<M> {
    model: M,
}

impl<M> Builder<M> {
    pub fn connector<Cx>(self, connectors: Cx) -> BuilderCx<M, Cx> {
        BuilderCx {
            model: self.model,
            connectors,
        }
    }
}

pub struct BuilderCx<M, Cx> {
    model: M,
    connectors: Cx,
}

impl<M, Cx> BuilderCx<M, Cx> {
    pub fn update<Msg>(self, update: UpdateFn<M, Msg, Cx>) -> BuilderUpd<M, Msg, Cx> {
        BuilderUpd {
            model: self.model,
            connectors: self.connectors,
            update,
        }
    }
}

pub struct BuilderUpd<M, Msg, Cx> {
    model: M,
    connectors: Cx,
    update: UpdateFn<M, Msg, Cx>,
}

impl<M, Msg, Cx> BuilderUpd<M, Msg, Cx> {
    pub fn view(self, view: ViewFn<M, Msg, Cx>) -> BuilderFull<M, Msg, Cx> {
        BuilderFull {
            model: self.model,
            connectors: self.connectors,
            update: self.update,
            view,
        }
    }
}

pub struct BuilderFull<M, Msg, Cx> {
    model: M,
    connectors: Cx,
    update: UpdateFn<M, Msg, Cx>,
    view: ViewFn<M, Msg, Cx>,
}

impl<M, Msg, Cx> BuilderFull<M, Msg, Cx> {
    /// Supply the per-user key the framework seals manifests with. Tests pass a
    /// fixed `Key::from_bytes(..)`; apps pass `SecretStore::user_key()`.
    pub fn key(self, key: Key) -> BuilderReady<M, Msg, Cx> {
        BuilderReady {
            model: self.model,
            connectors: self.connectors,
            update: self.update,
            view: self.view,
            key,
        }
    }
}

pub struct BuilderReady<M, Msg, Cx> {
    model: M,
    connectors: Cx,
    update: UpdateFn<M, Msg, Cx>,
    view: ViewFn<M, Msg, Cx>,
    key: Key,
}

impl<M, Msg, Cx> BuilderReady<M, Msg, Cx> {
    pub fn build(self) -> App<M, Msg, Cx> {
        App::new(
            self.model,
            self.connectors,
            self.update,
            self.view,
            self.key,
        )
    }
}
