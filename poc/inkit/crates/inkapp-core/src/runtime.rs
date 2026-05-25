//! The MVU loop runtime: the render walk (`render_document`) and the multi-cycle
//! driver (`App`, `DocSet`, `step`).

use std::sync::Arc;

use crate::assets::{asset_key, resolve_assets, AssetMap, ImageFetcher, OfflineFetcher};
use crate::cache::Cache;
use crate::component::RenderCx;
use crate::connector::ConnectorSet;
use crate::crypto::Key;
use crate::document::{DocKey, Document};
use crate::embed::embed_manifest;
use crate::error::Result;
use crate::geometry::PageGeom;
use crate::manifest::{recover_regions, Manifest};
use crate::render::document_to_pdf;

/// The framework Typst prelude, baked into the binary. Always registered and
/// imported so any component (and `#region`) is in scope.
pub const REGION_PRELUDE: (&str, &str) =
    ("/inkapp/region.typ", include_str!("../typst/region.typ"));

/// A rendered document: its PDF (manifest embedded), the recovered manifest, the
/// page height (for the device transform), and a content hash (for reconcile).
pub struct RenderedDoc {
    pub key: DocKey,
    pub pdf: Vec<u8>,
    pub manifest: Manifest,
    pub page_h: f64,
    /// Number of pages this document paginated to under its render geometry.
    pub page_count: usize,
    pub hash: u64,
}

/// Flatten an `AssetMap` into the `(path, bytes)` slice form the compile
/// functions take.
fn assets_as_slice(assets: &AssetMap) -> Vec<(String, Vec<u8>)> {
    assets.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// Collect the Typst sources to register for this document: the prelude plus each
/// component's declared sources, deduplicated by path (first occurrence wins).
pub fn collect_typst_sources<M>(doc: &Document<M>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> =
        vec![(REGION_PRELUDE.0.to_string(), REGION_PRELUDE.1.to_string())];
    for c in &doc.flow {
        for src in c.typst_sources() {
            if !out.iter().any(|(p, _)| p == &src.0) {
                out.push(src);
            }
        }
    }
    out
}

/// Assemble a document's Typst source at the default page geometry.
pub fn document_source<M>(doc: &Document<M>) -> String {
    document_source_in(doc, PageGeom::default())
}

/// Assemble a document's Typst source at an explicit page geometry: `#import`
/// lines for the prelude and authored sources, the `#set page` from `geom`, then
/// each component's render in flow order.
pub fn document_source_in<M>(doc: &Document<M>, geom: PageGeom) -> String {
    let mut cx = RenderCx::new(0);
    let mut src = String::new();
    for (path, _) in collect_typst_sources(doc) {
        src.push_str(&format!("#import \"{path}\": *\n"));
    }
    src.push_str(&format!(
        "#set page(width: {}pt, height: {}pt, margin: {}pt)\n#set text(size: 12pt)\n",
        geom.w, geom.h, geom.margin
    ));
    for c in &doc.flow {
        src.push_str(&c.render(&mut cx));
    }
    src
}

/// Compile a document at the default page geometry.
pub fn compile_document<M>(doc: &Document<M>) -> Result<typst::layout::PagedDocument> {
    compile_document_in(doc, PageGeom::default())
}

/// Compile a document at an explicit page geometry, with all its Typst sources
/// (prelude + authored components) registered.
pub fn compile_document_in<M>(
    doc: &Document<M>,
    geom: PageGeom,
) -> Result<typst::layout::PagedDocument> {
    compile_document_in_with_assets(doc, geom, &AssetMap::new())
}

/// Like `compile_document_in`, but also registers `assets` so the document may
/// embed `#image("/assets/{key}.png")`.
pub fn compile_document_in_with_assets<M>(
    doc: &Document<M>,
    geom: PageGeom,
    assets: &AssetMap,
) -> Result<typst::layout::PagedDocument> {
    let src = document_source_in(doc, geom);
    let sources = collect_typst_sources(doc);
    let asset_vec = assets_as_slice(assets);
    crate::render::compile_to_document_with_sources_and_assets(&src, &sources, &asset_vec)
}

/// Stable hash of a string (std DefaultHasher is deterministic within a build,
/// which is all reconcile needs — equal source -> equal hash).
fn hash_str(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// Render one document at the default page geometry.
pub fn render_document<M>(doc: &Document<M>, version: u64, key: &Key) -> Result<RenderedDoc> {
    render_document_in(doc, version, key, PageGeom::default())
}

/// Render one document at an explicit page geometry, sealing its manifest with `key`.
pub fn render_document_in<M>(
    doc: &Document<M>,
    version: u64,
    key: &Key,
    geom: PageGeom,
) -> Result<RenderedDoc> {
    render_document_in_with_assets(doc, version, key, geom, &AssetMap::new())
}

/// Like `render_document_in`, but registers `assets` so the document may embed
/// `#image("/assets/{key}.png")`.
pub fn render_document_in_with_assets<M>(
    doc: &Document<M>,
    version: u64,
    key: &Key,
    geom: PageGeom,
    assets: &AssetMap,
) -> Result<RenderedDoc> {
    let src = document_source_in(doc, geom);
    let sources = collect_typst_sources(doc);
    let asset_vec = assets_as_slice(assets);
    let compiled =
        crate::render::compile_to_document_with_sources_and_assets(&src, &sources, &asset_vec)?;
    // A single page_h suffices: `#set page` fixes every page of a document to the same
    // height, so the per-page device transform uses the same height on every page.
    let page_h = compiled
        .pages
        .first()
        .map(|p| p.frame.height().to_pt())
        .unwrap_or(geom.h);
    let page_count = compiled.pages.len();
    let mut manifest = recover_regions(&compiled)?.with_version(version);
    // Collect app-defined state into the manifest before sealing: the document's
    // own blob, then each stateful component's slice keyed by state_key().
    manifest.state.doc = doc.state.clone();
    for c in &doc.flow {
        if let (Some(k), Some(v)) = (c.state_key(), c.render_state()) {
            manifest.state.components.insert(k, v);
        }
    }
    let pdf = embed_manifest(&document_to_pdf(&compiled)?, &manifest, key)?;
    Ok(RenderedDoc {
        key: doc.key.clone(),
        pdf,
        manifest,
        page_h,
        page_count,
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
    page_count: usize,
    hash: u64,
    version: u64,
    /// Accumulated user ink (PDF space) per page — preserved across re-renders by key.
    ink: Vec<Vec<Stroke>>,
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

    /// The preserved per-page ink on `key` (empty slice if none / unknown).
    pub fn ink(&self, key: &DocKey) -> &[Vec<Stroke>] {
        self.entries
            .get(&key.0)
            .map(|e| e.ink.as_slice())
            .unwrap_or(&[])
    }

    /// The page count last rendered for `key`.
    pub fn page_count(&self, key: &DocKey) -> Option<usize> {
        self.entries.get(&key.0).map(|e| e.page_count)
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
    geom: PageGeom,
    fetcher: Arc<dyn ImageFetcher>,
    asset_cache: Option<Arc<Cache>>,
}

impl<M, Msg, Cx> App<M, Msg, Cx> {
    // The App genuinely has this many independent collaborators; the builder
    // (`app(..).connector(..).update(..).view(..).key(..)`) is the ergonomic
    // construction path, so the wide `new` is acceptable.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        model: M,
        connectors: Cx,
        update: UpdateFn<M, Msg, Cx>,
        view: ViewFn<M, Msg, Cx>,
        key: Key,
        geom: PageGeom,
        fetcher: Arc<dyn ImageFetcher>,
        asset_cache: Option<Arc<Cache>>,
    ) -> Self {
        Self {
            model,
            connectors,
            update,
            view,
            version: 1,
            key,
            geom,
            fetcher,
            asset_cache,
        }
    }

    /// Flush the asset cache (if any) so resolved images survive a restart.
    /// Live binaries call this on shutdown. Does not require connectors.
    pub async fn close(&self) -> Result<()> {
        if let Some(c) = &self.asset_cache {
            c.close().await?;
        }
        Ok(())
    }
}

impl<M, Msg, Cx: ConnectorSet> App<M, Msg, Cx> {
    /// Refresh every registered connector concurrently (warm caches before the
    /// sync `view`/`update` read them). Per-connector refresh errors are
    /// swallowed: a connector that can't refresh serves its stale cache.
    async fn refresh_all(&self) {
        let cs = self.connectors.connectors();
        futures::future::join_all(cs.iter().map(|c| c.refresh())).await;
    }

    /// Flush every registered connector's write queue concurrently.
    async fn flush_all(&self) {
        let cs = self.connectors.connectors();
        futures::future::join_all(cs.iter().map(|c| c.flush())).await;
    }

    /// Collect every component's declared image URLs across the doc set, map each
    /// to its `(asset_key, url)` pair, and resolve them through the pipeline into
    /// an `AssetMap` (fetch + normalize + cache, placeholder on failure).
    async fn resolve_doc_assets(&self, docs: &Documents<Msg>) -> AssetMap {
        let mut pairs: Vec<(String, String)> = Vec::new();
        for doc in &docs.0 {
            for c in &doc.flow {
                for url in c.image_urls() {
                    pairs.push((asset_key(&url), url));
                }
            }
        }
        resolve_assets(&pairs, self.asset_cache.as_deref(), &*self.fetcher).await
    }

    /// Render the full document set from current state, (re)populating `set`.
    /// Refreshes connectors first so `view` reads warm caches.
    pub async fn render(&mut self, set: &mut DocSet) -> Result<Vec<RenderedDoc>> {
        self.refresh_all().await;
        let docs = (self.view)(&self.model, &self.connectors);
        let assets = self.resolve_doc_assets(&docs).await;
        let mut out = Vec::new();
        let mut entries = HashMap::new();
        for doc in &docs.0 {
            let rd =
                render_document_in_with_assets(doc, self.version, &self.key, self.geom, &assets)?;
            entries.insert(
                rd.key.0.clone(),
                DocEntry {
                    manifest: rd.manifest.clone(),
                    page_h: rd.page_h,
                    page_count: rd.page_count,
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

    /// One loop cycle: refresh, decode `ink_by_key` (pre-fold view + stored
    /// manifest), fold the messages, re-render, reconcile, update `set`, then
    /// flush queued writes.
    pub async fn step(
        &mut self,
        set: &mut DocSet,
        ink_by_key: &HashMap<String, Vec<Vec<Stroke>>>,
    ) -> Result<Cycle<Msg>>
    where
        Msg: Clone,
    {
        self.refresh_all().await;

        // 1. Decode against the pre-fold trees + the stored manifests.
        let pre = (self.view)(&self.model, &self.connectors);
        let mut decoded: Vec<Msg> = Vec::new();
        for doc in &pre.0 {
            let Some(pages) = ink_by_key.get(&doc.key.0) else {
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
            let region_ink = attribute(pages, &entry.manifest);
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
        let assets = self.resolve_doc_assets(&next).await;
        let mut next_rendered: Vec<RenderedDoc> = Vec::new();
        for doc in &next.0 {
            next_rendered.push(render_document_in_with_assets(
                doc,
                self.version,
                &self.key,
                self.geom,
                &assets,
            )?);
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

        // 5. Apply: rebuild entries, preserving ink on survivors and appending
        //    this cycle's input ink. Collect created/updated for push.
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
            // Preserve prior per-page ink, then append this cycle's input per page. This only
            // GROWS the outer vec: if a re-render paginates to fewer pages than previously
            // inked, the extra per-page entries are retained rather than dropped — keeping the
            // user's real annotations is safer than silently discarding them on a transient
            // shrink. Consequence: after such a shrink, `ink.len()` may exceed `page_count`.
            // This is harmless for decode (`attribute` range-checks pages against manifest
            // regions); proper ink reflow across re-pagination is a separate, deferred concern.
            let mut ink: Vec<Vec<Stroke>> = set
                .entries
                .get(&rd.key.0)
                .map(|e| e.ink.clone())
                .unwrap_or_default();
            if let Some(new_pages) = ink_by_key.get(&rd.key.0) {
                if ink.len() < new_pages.len() {
                    ink.resize(new_pages.len(), Vec::new());
                }
                for (p, strokes) in new_pages.iter().enumerate() {
                    ink[p].extend(strokes.iter().cloned());
                }
            }
            let is_changed = changed.contains_key(rd.key.0.as_str());
            new_entries.insert(
                rd.key.0.clone(),
                DocEntry {
                    manifest: rd.manifest.clone(),
                    page_h: rd.page_h,
                    page_count: rd.page_count,
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

        // 6. Push this cycle's enqueued writes out (recorded-and-retried).
        self.flush_all().await;

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
            geom: PageGeom::default(),
            fetcher: Arc::new(OfflineFetcher),
            asset_cache: None,
        }
    }
}

pub struct BuilderReady<M, Msg, Cx> {
    model: M,
    connectors: Cx,
    update: UpdateFn<M, Msg, Cx>,
    view: ViewFn<M, Msg, Cx>,
    key: Key,
    geom: PageGeom,
    fetcher: Arc<dyn ImageFetcher>,
    asset_cache: Option<Arc<Cache>>,
}

impl<M, Msg, Cx> BuilderReady<M, Msg, Cx> {
    /// Override the page geometry for this app (default: 420×560pt with 16pt margin).
    #[must_use]
    pub fn page(mut self, geom: PageGeom) -> Self {
        self.geom = geom;
        self
    }

    /// Inject the image fetcher (default: `OfflineFetcher`, i.e. no network).
    #[must_use]
    pub fn fetcher(mut self, fetcher: Arc<dyn ImageFetcher>) -> Self {
        self.fetcher = fetcher;
        self
    }

    /// Inject the durable asset cache used for warm-restart / offline image serving.
    /// Asset bytes occupy the `assets/*` key namespace; a cache shared with a
    /// connector must keep its own keys clear of that prefix.
    #[must_use]
    pub fn asset_cache(mut self, cache: Arc<Cache>) -> Self {
        self.asset_cache = Some(cache);
        self
    }

    pub fn build(self) -> App<M, Msg, Cx> {
        App::new(
            self.model,
            self.connectors,
            self.update,
            self.view,
            self.key,
            self.geom,
            self.fetcher,
            self.asset_cache,
        )
    }
}
