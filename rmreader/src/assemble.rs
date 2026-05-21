//! Build the reading document (index + full articles) + manifest.
//!
//! Two tiers: an index page that links straight to each full article, then the
//! articles themselves. There is no summary-card tier — tapping an index row goes
//! directly to the article; the article's nav goes Home (index) / Prev / Next.
use askama::Template;

use crate::manifest::{Manifest, ManifestItem};
use crate::readwise::Document;

pub struct Built {
    pub fragments: Vec<String>, // page fragments (wrapped by render::Base)
    pub assets: Vec<(String, Vec<u8>)>,
    pub manifest: Manifest,
}

struct IndexRow {
    num: String,
    title: String,
    author: String,
    reading_time: String,
    anchor: String,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTpl<'a> {
    collection: &'a str,
    count: usize,
    rows: &'a [IndexRow],
}

#[derive(Template)]
#[template(path = "article.html")]
struct ArticleTpl<'a> {
    anchor: &'a str,
    category: &'a str,
    title: &'a str,
    author: &'a str,
    site_name: &'a str,
    reading_time: &'a str,
    content: &'a str,
}

fn rt(d: &Document) -> String {
    match d.reading_time.as_deref() {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        _ => "—".into(),
    }
}

/// Heading fallback for the occasional doc that has no title.
fn title_or(d: &Document) -> String {
    if !d.title.trim().is_empty() {
        d.title.clone()
    } else if !d.site_name.trim().is_empty() {
        d.site_name.clone()
    } else {
        "Untitled".into()
    }
}

/// `content_fn(html_content, id) -> (processed_html, assets)` is injected so the
/// content pipeline (and its network) stays out of assembly (testable).
pub fn assemble_document(
    collection: &str,
    docs: &[Document],
    content_fn: impl Fn(&str, &str) -> (String, Vec<(String, Vec<u8>)>),
) -> Built {
    let mut assets: Vec<(String, Vec<u8>)> = Vec::new();
    let mut items: Vec<ManifestItem> = Vec::new();
    let mut fragments: Vec<String> = Vec::new();

    // Index — each row links straight to the full article.
    let rows: Vec<IndexRow> = docs
        .iter()
        .enumerate()
        .map(|(i, d)| IndexRow {
            num: format!("{:02}", i + 1),
            title: title_or(d),
            author: d.author.clone(),
            reading_time: rt(d),
            anchor: format!("article-{}", d.id),
        })
        .collect();
    fragments.push(
        IndexTpl {
            collection,
            count: docs.len(),
            rows: &rows,
        }
        .render()
        .unwrap(),
    );

    // Articles. Per-page Home/Prev/Next nav is drawn later by the PDF
    // post-processor (postprocess::add_per_page_nav), not rendered in-flow,
    // so it can repeat as a clickable bar on every page of a flowing article.
    for d in docs.iter() {
        let article_anchor = format!("article-{}", d.id);
        let raw = d.html_content.clone().unwrap_or_default();
        let (processed, mut a) = content_fn(&raw, &d.id);
        assets.append(&mut a);
        let title = title_or(d);
        fragments.push(
            ArticleTpl {
                anchor: &article_anchor,
                category: &d.category,
                title: &title,
                author: &d.author,
                site_name: &d.site_name,
                reading_time: &rt(d),
                content: &processed,
            }
            .render()
            .unwrap(),
        );
        items.push(ManifestItem {
            id: d.id.clone(),
            title,
            url: d.url.clone(),
            article_anchor,
        });
    }

    Built {
        fragments,
        assets,
        manifest: Manifest {
            collection: collection.to_string(),
            items,
        },
    }
}
