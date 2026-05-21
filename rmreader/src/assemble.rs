//! Build the 3-tier HTML document (index, cards, articles) + manifest.
use askama::Template;

use crate::manifest::{Manifest, ManifestItem};
use crate::readwise::Document;

pub struct Built {
    pub fragments: Vec<String>, // page fragments (wrapped by render::Base)
    pub assets: Vec<(String, Vec<u8>)>,
    pub manifest: Manifest,
}

#[derive(Template)]
#[template(path = "nav.html")]
struct Nav {
    prev: Option<String>,
    next: Option<String>,
    card: Option<String>,
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
#[template(path = "card.html")]
struct CardTpl<'a> {
    anchor: &'a str,
    article_anchor: &'a str,
    nav: &'a str,
    category: &'a str,
    title: &'a str,
    summary: &'a str,
    author: &'a str,
    site_name: &'a str,
    reading_time: &'a str,
}

#[derive(Template)]
#[template(path = "article.html")]
struct ArticleTpl<'a> {
    anchor: &'a str,
    nav: &'a str,
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

    // Index
    let rows: Vec<IndexRow> = docs
        .iter()
        .enumerate()
        .map(|(i, d)| IndexRow {
            num: format!("{:02}", i + 1),
            title: d.title.clone(),
            author: d.author.clone(),
            reading_time: rt(d),
            anchor: format!("item-{}", d.id),
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

    // Cards
    for (i, d) in docs.iter().enumerate() {
        let card_anchor = format!("item-{}", d.id);
        let article_anchor = format!("article-{}", d.id);
        let prev = if i > 0 {
            Some(format!("item-{}", docs[i - 1].id))
        } else {
            None
        };
        let next = if i + 1 < docs.len() {
            Some(format!("item-{}", docs[i + 1].id))
        } else {
            None
        };
        let nav = Nav {
            prev,
            next,
            card: None,
        }
        .render()
        .unwrap();
        fragments.push(
            CardTpl {
                anchor: &card_anchor,
                article_anchor: &article_anchor,
                nav: &nav,
                category: &d.category,
                title: &d.title,
                summary: &d.summary,
                author: &d.author,
                site_name: &d.site_name,
                reading_time: &rt(d),
            }
            .render()
            .unwrap(),
        );
        items.push(ManifestItem {
            id: d.id.clone(),
            title: d.title.clone(),
            url: d.url.clone(),
            card_anchor,
            article_anchor,
        });
    }

    // Articles
    for (i, d) in docs.iter().enumerate() {
        let article_anchor = format!("article-{}", d.id);
        let card_anchor = format!("item-{}", d.id);
        let prev = if i > 0 {
            Some(format!("article-{}", docs[i - 1].id))
        } else {
            None
        };
        let next = if i + 1 < docs.len() {
            Some(format!("article-{}", docs[i + 1].id))
        } else {
            None
        };
        let nav = Nav {
            prev,
            next,
            card: Some(card_anchor),
        }
        .render()
        .unwrap();
        let raw = d.html_content.clone().unwrap_or_default();
        let (processed, mut a) = content_fn(&raw, &d.id);
        assets.append(&mut a);
        fragments.push(
            ArticleTpl {
                anchor: &article_anchor,
                nav: &nav,
                category: &d.category,
                title: &d.title,
                author: &d.author,
                site_name: &d.site_name,
                reading_time: &rt(d),
                content: &processed,
            }
            .render()
            .unwrap(),
        );
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
