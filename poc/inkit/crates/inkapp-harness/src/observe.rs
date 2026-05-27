//! Read-side views over a Session: the agent's "accessibility tree".
//!
//! Stubs for ink/layer fields land here; Tasks 8, 9 populate them. Task 6 wires
//! in PDF link annotations via `crate::pdf_links`.

use std::fs;
use std::path::Path;

use inkapp_core::ink::Stroke;
use inkapp_core::manifest::Manifest;
use inkapp_core::render::compile_to_document_with_sources;
use inkapp_core::REGION_PRELUDE;
use rm_cloud::{Client, Config};
use serde::Serialize;

use crate::pdf_links::{self, LinkTarget, RawLink};
use crate::session::{DocSummary, Session};

#[derive(Debug, Serialize)]
pub struct RegionDescribe {
    pub name: String,
    pub rect: [f64; 4],
    pub layer_hint: String,
    pub link: Option<LinkTarget>,
    pub app_state: serde_json::Value,
    pub ink: InkSummary,
}

#[derive(Debug, Serialize, Default)]
pub struct InkSummary {
    pub strokes: usize,
    pub by_layer: std::collections::BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
pub struct LinkAnnotation {
    pub rect: [f64; 4],
    pub target: LinkTarget,
    pub region: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PageDescribe {
    pub doc_id: String,
    pub page: usize,
    pub version: u32,
    pub regions: Vec<RegionDescribe>,
    pub links: Vec<LinkAnnotation>,
    pub layers_present: Vec<String>,
    pub image: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DocumentDescribe {
    pub doc_id: String,
    pub app_name: String,
    pub version: u32,
    pub pages: usize,
    pub regions_per_page: Vec<usize>,
    pub links_per_page: Vec<usize>,
}

pub fn page_describe(
    session: &Session,
    doc_id: &str,
    page: usize,
) -> std::io::Result<PageDescribe> {
    let (summary, manifest) = load_doc(session.state_dir(), doc_id)?;
    let pdf_bytes = fs::read(
        session
            .state_dir()
            .join("docs")
            .join(doc_id)
            .join("pdf.pdf"),
    )?;
    let all_links = pdf_links::extract(&pdf_bytes);
    let page_links: Vec<&RawLink> = all_links.iter().filter(|l| l.page == page).collect();

    let regions: Vec<RegionDescribe> = manifest
        .regions
        .iter()
        .filter(|r| r.page == page)
        .map(|r| {
            let region_rect = [r.rect.x0, r.rect.y0, r.rect.x1, r.rect.y1];
            let link = page_links
                .iter()
                .find(|l| rect_contains(&region_rect, &l.rect))
                .map(|l| l.target.clone());
            RegionDescribe {
                name: r.name.clone(),
                rect: region_rect,
                layer_hint: "pen".to_string(),
                link,
                app_state: manifest
                    .state
                    .components
                    .get(&r.name)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                ink: InkSummary::default(),
            }
        })
        .collect();

    let links: Vec<LinkAnnotation> = page_links
        .iter()
        .map(|l| {
            let region = manifest
                .regions
                .iter()
                .filter(|r| r.page == page)
                .find(|r| rect_contains(&[r.rect.x0, r.rect.y0, r.rect.x1, r.rect.y1], &l.rect))
                .map(|r| r.name.clone());
            LinkAnnotation {
                rect: l.rect,
                target: l.target.clone(),
                region,
            }
        })
        .collect();

    Ok(PageDescribe {
        doc_id: doc_id.to_string(),
        page,
        version: summary.version,
        regions,
        links,
        layers_present: Vec::new(),
        image: None,
    })
}

pub fn document_describe(session: &Session, doc_id: &str) -> std::io::Result<DocumentDescribe> {
    let (summary, manifest) = load_doc(session.state_dir(), doc_id)?;
    let pages = summary.pages.max(1);
    let mut regions_per_page = vec![0usize; pages];
    for r in &manifest.regions {
        if r.page < regions_per_page.len() {
            regions_per_page[r.page] += 1;
        }
    }
    let pdf_bytes = fs::read(
        session
            .state_dir()
            .join("docs")
            .join(doc_id)
            .join("pdf.pdf"),
    )?;
    let mut links_per_page = vec![0usize; pages];
    for l in pdf_links::extract(&pdf_bytes) {
        if l.page < links_per_page.len() {
            links_per_page[l.page] += 1;
        }
    }
    Ok(DocumentDescribe {
        doc_id: summary.id,
        app_name: summary.app_name,
        version: summary.version,
        pages,
        regions_per_page,
        links_per_page,
    })
}

/// True if `outer` "substantially contains" `inner`: either strict containment with a
/// 1pt tolerance, OR the rectangles overlap by more than half the inner rect's area.
/// The overlap fallback is needed because Typst's `#link` annotation rect bounds the
/// glyph's full ascender/descender extent, which can spill a few points outside a
/// `#region` frame whose height tracks only the laid-out text — both clearly refer
/// to the same visual target, so we attribute the link to the region anyway.
pub(crate) fn rect_contains(outer: &[f64; 4], inner: &[f64; 4]) -> bool {
    let strict = inner[0] >= outer[0] - 1.0
        && inner[1] >= outer[1] - 1.0
        && inner[2] <= outer[2] + 1.0
        && inner[3] <= outer[3] + 1.0;
    if strict {
        return true;
    }
    let inner_area = (inner[2] - inner[0]).max(0.0) * (inner[3] - inner[1]).max(0.0);
    let outer_area = (outer[2] - outer[0]).max(0.0) * (outer[3] - outer[1]).max(0.0);
    let min_area = inner_area.min(outer_area);
    if min_area <= 0.0 {
        return false;
    }
    let ox0 = inner[0].max(outer[0]);
    let oy0 = inner[1].max(outer[1]);
    let ox1 = inner[2].min(outer[2]);
    let oy1 = inner[3].min(outer[3]);
    let overlap = (ox1 - ox0).max(0.0) * (oy1 - oy0).max(0.0);
    // Overlap relative to the smaller rect: catches the Typst link-vs-region case where
    // the link rect is taller (ascender/descender) than the region's text-height frame.
    overlap / min_area >= 0.5
}

/// Re-compile the document's stored `source.typ` and rasterize page `page` to PNG bytes.
pub fn page_snapshot(session: &Session, doc_id: &str, page: usize) -> std::io::Result<Vec<u8>> {
    let doc_dir = session.state_dir().join("docs").join(doc_id);
    let src = fs::read_to_string(doc_dir.join("source.typ"))?;
    let sources = vec![(REGION_PRELUDE.0.to_string(), REGION_PRELUDE.1.to_string())];
    let typst_doc = compile_to_document_with_sources(&src, &sources)
        .map_err(|e| std::io::Error::other(format!("typst compile: {e}")))?;
    crate::inspector::render_page(&typst_doc, page)
        .map_err(|e| std::io::Error::other(format!("render: {e}")))
}

/// Re-compile the document's stored `source.typ`, fetch its PDF link annotations
/// for `page`, and rasterize an inspector PNG with overlays toggled per `opts`.
///
/// Ink stroke sets (synth / attributed) are passed through as empty for now —
/// Tasks 10 and 11 will plumb pending and attributed strokes through. This
/// task only exposes the rendering seam.
pub fn page_inspect(
    session: &Session,
    doc_id: &str,
    page: usize,
    opts: &crate::inspector::InspectOpts,
) -> std::io::Result<Vec<u8>> {
    let doc_dir = session.state_dir().join("docs").join(doc_id);
    let src = fs::read_to_string(doc_dir.join("source.typ"))?;
    let pdf_bytes = fs::read(doc_dir.join("pdf.pdf"))?;
    let (_, manifest) = load_doc(session.state_dir(), doc_id)?;

    let sources = vec![(REGION_PRELUDE.0.to_string(), REGION_PRELUDE.1.to_string())];
    let typst_doc = compile_to_document_with_sources(&src, &sources)
        .map_err(|e| std::io::Error::other(format!("typst compile: {e}")))?;

    let links: Vec<(f64, f64, f64, f64)> = pdf_links::extract(&pdf_bytes)
        .into_iter()
        .filter(|l| l.page == page)
        .map(|l| (l.rect[0], l.rect[1], l.rect[2], l.rect[3]))
        .collect();

    crate::inspector::inspect_with_opts(&typst_doc, &manifest, &links, &[], &[], page, opts)
        .map_err(|e| std::io::Error::other(format!("inspect: {e}")))
}

#[derive(Debug, Serialize)]
pub struct DeviceTree {
    pub root: DeviceTreeNode,
}

#[derive(Debug, Serialize)]
pub struct DeviceTreeNode {
    pub id: String,
    pub name: String,
    pub parent_id: String,
    pub is_folder: bool,
    pub children: Vec<DeviceTreeNode>,
}

/// Walk the session's fake cloud and return a tree of folders + docs starting at
/// the root. `_path` is reserved for future scoping (e.g. start under a subtree).
pub async fn device_tree(
    session: &Session,
    _device_id: &str,
    _path: &str,
) -> std::io::Result<DeviceTree> {
    let base = &session.cloud().base;
    let client = Client::from_user_token(Config::single_host(base), "user-token");
    let root = walk_tree(&client, "", "", "")
        .await
        .map_err(|e| std::io::Error::other(format!("walk: {e}")))?;
    Ok(DeviceTree { root })
}

fn walk_tree<'a>(
    client: &'a Client,
    parent_id: &'a str,
    self_id: &'a str,
    self_name: &'a str,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = rm_cloud::Result<DeviceTreeNode>> + Send + 'a>,
> {
    Box::pin(async move {
        let entries = client.ls(parent_id).await?;
        let mut children = Vec::new();
        for entry in entries {
            if entry.is_folder {
                let sub = walk_tree(client, &entry.id, &entry.id, &entry.name).await?;
                children.push(sub);
            } else {
                children.push(DeviceTreeNode {
                    id: entry.id,
                    name: entry.name,
                    parent_id: entry.parent,
                    is_folder: false,
                    children: Vec::new(),
                });
            }
        }
        Ok(DeviceTreeNode {
            id: self_id.to_string(),
            name: self_name.to_string(),
            parent_id: String::new(),
            is_folder: true,
            children,
        })
    })
}

#[derive(Debug, Clone, Copy)]
pub enum ObserveGroup {
    Flat,
    ByLayer,
    ByRegion,
}

#[derive(Debug, Serialize)]
pub struct InkList {
    pub strokes: Vec<Stroke>,
    pub by_layer: Option<std::collections::BTreeMap<String, Vec<Stroke>>>,
    pub by_region: Option<std::collections::BTreeMap<String, Vec<Stroke>>>,
}

/// List the pending strokes for `doc_id` page `page`, optionally grouped by
/// layer or by region. Reads from each device's `pending/<doc>/<page>.json`;
/// returns an empty result if nothing is persisted yet.
pub fn ink_list(
    session: &Session,
    doc_id: &str,
    page: usize,
    group: ObserveGroup,
) -> std::io::Result<InkList> {
    let devices_dir = session.state_dir().join("devices");
    let mut all: Vec<Stroke> = Vec::new();
    if devices_dir.exists() {
        for entry in fs::read_dir(&devices_dir)? {
            let entry = entry?;
            let pending = entry
                .path()
                .join("pending")
                .join(doc_id)
                .join(format!("{page}.json"));
            if pending.exists() {
                let bytes = fs::read(&pending)?;
                let mut v: Vec<Stroke> = serde_json::from_slice(&bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                all.append(&mut v);
            }
        }
    }

    let (by_layer, by_region) = match group {
        ObserveGroup::Flat => (None, None),
        ObserveGroup::ByLayer => {
            let mut m: std::collections::BTreeMap<String, Vec<Stroke>> = Default::default();
            for s in &all {
                let key = if s.highlighter { "highlights" } else { "pen" };
                m.entry(key.into()).or_default().push(s.clone());
            }
            (Some(m), None)
        }
        ObserveGroup::ByRegion => {
            let (_, manifest) = load_doc(session.state_dir(), doc_id)?;
            // Use the library's attribute() so the lens and the app loop see strokes
            // grouped identically. The library tests all points and allows multi-region
            // attribution; the former ad-hoc stroke_region helper used only the midpoint
            // and returned the first match, which diverged from the library's behaviour.
            let max_page = manifest
                .regions
                .iter()
                .map(|r| r.page)
                .max()
                .unwrap_or(page);
            let n_pages = max_page.max(page) + 1;
            let mut pages: Vec<Vec<Stroke>> = vec![Vec::new(); n_pages];
            pages[page] = all.clone();
            let region_inks = inkapp_core::readback::attribute(&pages, &manifest);
            let mut m: std::collections::BTreeMap<String, Vec<Stroke>> = Default::default();
            for ri in region_inks {
                if !ri.strokes.is_empty() {
                    m.insert(ri.region, ri.strokes);
                }
            }
            (None, Some(m))
        }
    };
    Ok(InkList {
        strokes: all,
        by_layer,
        by_region,
    })
}

pub(crate) fn load_doc(state_dir: &Path, doc_id: &str) -> std::io::Result<(DocSummary, Manifest)> {
    let dir = state_dir.join("docs").join(doc_id);
    let summary: DocSummary = serde_json::from_slice(&fs::read(dir.join("doc.json"))?)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let manifest: Manifest = serde_json::from_slice(&fs::read(dir.join("manifest.json"))?)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok((summary, manifest))
}
