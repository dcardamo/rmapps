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
    title: &'a str,
    byline: &'a str,
    content: &'a str,
}

fn rt(d: &Document) -> String {
    match d.reading_time.as_deref() {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        _ => "—".into(),
    }
}

/// Build the article byline, de-duplicating author vs site_name (newsletters
/// often set both to the same name, e.g. "James Clear · James Clear").
fn byline(d: &Document) -> String {
    let author = d.author.trim();
    let site = d.site_name.trim();
    let mut s = String::new();
    if !author.is_empty() {
        s.push_str(author);
    }
    if !site.is_empty() && !site.eq_ignore_ascii_case(author) {
        if !s.is_empty() {
            s.push_str(" · ");
        }
        s.push_str(site);
    }
    if !s.is_empty() {
        s.push_str(" · ");
    }
    s.push_str(&rt(d));
    s
}

/// Normalise text for comparison: collapse whitespace, lowercase.
fn norm(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Drop any `<h1>`/`<h2>` in the body whose text duplicates the article title —
/// emails/newsletters routinely repeat the title as a body heading, so it would
/// otherwise show twice (template headline + body heading).
fn remove_title_headings(html: &str, title: &str) -> String {
    let nt = norm(title);
    if nt.is_empty() {
        return html.to_string();
    }
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    loop {
        let next = ["<h1", "<h2"]
            .iter()
            .filter_map(|t| rest.find(t).map(|i| (i, &t[1..3])))
            .min_by_key(|&(i, _)| i);
        let Some((pos, tag)) = next else {
            out.push_str(rest);
            break;
        };
        let close = format!("</{tag}>");
        let Some(gt) = rest[pos..].find('>') else {
            out.push_str(rest);
            break;
        };
        let inner_start = pos + gt + 1;
        let Some(close_rel) = rest[inner_start..].find(&close) else {
            out.push_str(rest);
            break;
        };
        let inner = &rest[inner_start..inner_start + close_rel];
        let end = inner_start + close_rel + close.len();
        if norm(&crate::content::strip_tags(inner)) == nt {
            out.push_str(&rest[..pos]); // drop the duplicate-title heading
        } else {
            out.push_str(&rest[..end]); // keep it
        }
        rest = &rest[end..];
    }
    out
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
    mut content_fn: impl FnMut(&str, &str) -> (String, Vec<(String, Vec<u8>)>),
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
    // post-processor (postprocess::finalize_pdf), not rendered in-flow,
    // so it can repeat as a clickable bar on every page of a flowing article.
    for d in docs.iter() {
        let article_anchor = format!("article-{}", d.id);
        let raw = d.html_content.clone().unwrap_or_default();
        let (processed, mut a) = content_fn(&raw, &d.id);
        assets.append(&mut a);
        let title = title_or(d);
        let content = remove_title_headings(&processed, &title);
        let bl = byline(d);
        fragments.push(
            ArticleTpl {
                anchor: &article_anchor,
                title: &title,
                byline: &bl,
                content: &content,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::readwise::Document;

    fn doc(author: &str, site: &str) -> Document {
        Document {
            id: "d".into(),
            url: String::new(),
            source_url: String::new(),
            title: "T".into(),
            author: author.into(),
            site_name: site.into(),
            category: "article".into(),
            location: "new".into(),
            summary: String::new(),
            image_url: String::new(),
            word_count: None,
            reading_time: Some("3 mins".into()),
            published_date: None,
            saved_at: "2026-01-01T00:00:00Z".into(),
            html_content: None,
        }
    }

    #[test]
    fn byline_dedupes_author_and_site() {
        assert_eq!(
            byline(&doc("James Clear", "James Clear")),
            "James Clear · 3 mins"
        );
        assert_eq!(
            byline(&doc("Alice", "The Times")),
            "Alice · The Times · 3 mins"
        );
        assert_eq!(byline(&doc("", "")), "3 mins");
    }

    #[test]
    fn removes_body_heading_matching_title() {
        let out = remove_title_headings("<h1>My Title</h1><p>real body</p>", "My  Title");
        assert!(
            !out.contains("<h1>"),
            "title heading should be removed: {out}"
        );
        assert!(out.contains("real body"));
    }

    #[test]
    fn keeps_non_title_heading() {
        let out = remove_title_headings("<h2>Section Two</h2><p>body</p>", "My Title");
        assert!(out.contains("Section Two"));
    }
}
