//! Live Reader API: pure URL/body builders + response parsers (unit-tested), and
//! the thin reqwest-middleware transports that call them. Network round-trips are
//! covered by the `#[ignore]` live bar, not unit tests.

use serde::Deserialize;

use inkapp_core::connector::ConnectorError;

use crate::{Article, ArticleId, FetchTransport, Location, Page, Write, WriteTransport};

const LIST: &str = "https://readwise.io/api/v3/list/";
const UPDATE: &str = "https://readwise.io/api/v3/update/";
const DELETE: &str = "https://readwise.io/api/v3/delete/";
const HIGHLIGHTS: &str = "https://readwise.io/api/v2/highlights/";
const LIMIT: u32 = 50;

// --- Pure builders / parsers (unit-tested, no network) ---

/// Build the list URL for a location + optional cursor.
pub fn build_list_url(location: &str, cursor: Option<&str>) -> String {
    let mut u = format!("{LIST}?withHtmlContent=true&limit={LIMIT}&location={location}");
    if let Some(c) = cursor {
        // `location` is one of the five known ASCII enum strings; Readwise page
        // cursors are opaque base64url (URL-safe) tokens. Both are safe to concatenate
        // directly. If either could ever contain reserved chars, use proper query
        // encoding (e.g. url::Url::query_pairs_mut).
        u.push_str("&pageCursor=");
        u.push_str(c);
    }
    u
}

/// The serialized v2 highlight-create body.
pub fn highlight_body(
    text: &str,
    title: &str,
    author: &str,
    source_url: &str,
    category: &str,
) -> String {
    serde_json::json!({
        "highlights": [{
            "text": text,
            "title": title,
            "author": author,
            "source_url": source_url,
            "category": category,
        }]
    })
    .to_string()
}

/// A parsed list page.
pub struct ListResponse {
    pub articles: Vec<Article>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize)]
struct RawList {
    #[serde(rename = "nextPageCursor")]
    next: Option<String>,
    results: Vec<RawDoc>,
}

#[derive(Deserialize)]
struct RawDoc {
    id: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    source_url: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    site_name: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    location: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    image_url: Option<String>,
    #[serde(default)]
    word_count: Option<u32>,
    #[serde(default)]
    reading_time: Option<String>,
    #[serde(default)]
    published_date: Option<String>,
    #[serde(default)]
    saved_at: String,
    #[serde(default)]
    html_content: Option<String>,
}

fn loc_from(s: &str) -> Location {
    match s {
        "later" => Location::Later,
        "shortlist" => Location::Shortlist,
        "archive" => Location::Archive,
        "feed" => Location::Feed,
        _ => Location::New,
    }
}

/// Parse a Reader v3 list body into articles + cursor.
pub fn parse_list(raw: &str) -> Result<ListResponse, ConnectorError> {
    let parsed: RawList = serde_json::from_str(raw)
        .map_err(|e| ConnectorError::Transport(format!("list parse: {e}")))?;
    let articles = parsed
        .results
        .into_iter()
        .map(|d| Article {
            id: ArticleId::new(d.id),
            title: d.title,
            body: String::new(),
            highlights: Vec::new(),
            url: d.url,
            source_url: d.source_url,
            author: d.author,
            site_name: d.site_name,
            category: d.category,
            location: loc_from(&d.location),
            summary: d.summary,
            image_url: d.image_url,
            word_count: d.word_count,
            reading_time: d.reading_time,
            published_date: d.published_date,
            saved_at: d.saved_at,
            html_content: d.html_content,
        })
        .collect();
    Ok(ListResponse {
        articles,
        next_cursor: parsed.next,
    })
}

/// Map an HTTP status code to a connector error (returns `None` for 2xx).
pub fn status_error(status: u16) -> Option<ConnectorError> {
    match status {
        200..=299 => None,
        401 | 403 => Some(ConnectorError::Auth(format!("status {status}"))),
        429 => Some(ConnectorError::RateLimited),
        s => Some(ConnectorError::Transport(format!("status {s}"))),
    }
}

// --- Thin transports ---

use reqwest_middleware::ClientWithMiddleware;

/// Lookup closure type: maps an `ArticleId` to its cached `Article` (for highlight metadata).
pub type ArticleLookup = std::sync::Arc<dyn Fn(&ArticleId) -> Option<Article> + Send + Sync>;

/// HTTP transport for fetching Reader article lists.
pub struct HttpFetch {
    client: ClientWithMiddleware,
    token: String,
}

impl HttpFetch {
    pub fn new(client: ClientWithMiddleware, token: String) -> Self {
        Self { client, token }
    }
}

/// HTTP transport for pushing writes (move / delete / highlight) to the Reader API.
pub struct HttpWrite {
    client: ClientWithMiddleware,
    token: String,
    /// Article lookup: provides title/author/source_url/category for highlight POSTs.
    lookup: ArticleLookup,
}

impl HttpWrite {
    pub fn new(client: ClientWithMiddleware, token: String, lookup: ArticleLookup) -> Self {
        Self {
            client,
            token,
            lookup,
        }
    }
}

#[async_trait::async_trait]
impl FetchTransport for HttpFetch {
    async fn list(&self, location: &str, cursor: Option<&str>) -> Result<Page, ConnectorError> {
        let url = build_list_url(location, cursor);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Token {}", self.token))
            .send()
            .await
            .map_err(|e| ConnectorError::Transport(e.to_string()))?;
        if let Some(err) = status_error(resp.status().as_u16()) {
            return Err(err);
        }
        let raw = resp
            .text()
            .await
            .map_err(|e| ConnectorError::Transport(e.to_string()))?;
        let ListResponse {
            articles,
            next_cursor,
        } = parse_list(&raw)?;
        Ok(Page {
            articles,
            next_cursor,
        })
    }
}

#[async_trait::async_trait]
impl WriteTransport for HttpWrite {
    async fn push(&self, write: &Write) -> Result<(), ConnectorError> {
        let auth = format!("Token {}", self.token);
        let resp = match write {
            Write::Move(id, loc) => {
                self.client
                    .patch(format!("{UPDATE}{}/", id.0))
                    .header("Authorization", auth)
                    .json(&serde_json::json!({ "location": loc.as_str() }))
                    .send()
                    .await
            }
            Write::Delete(id) => {
                self.client
                    .delete(format!("{DELETE}{}/", id.0))
                    .header("Authorization", auth)
                    .send()
                    .await
            }
            Write::Highlight(id, text) => {
                let a = (self.lookup)(id).unwrap_or_else(|| Article {
                    id: id.clone(),
                    ..Default::default()
                });
                let cat = if a.category.is_empty() {
                    "articles".to_string()
                } else {
                    a.category.clone()
                };
                let body = highlight_body(text, &a.title, &a.author, &a.source_url, &cat);
                self.client
                    .post(HIGHLIGHTS)
                    .header("Authorization", auth)
                    .header("Content-Type", "application/json")
                    .body(body)
                    .send()
                    .await
            }
        }
        .map_err(|e| ConnectorError::Transport(e.to_string()))?;

        match status_error(resp.status().as_u16()) {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
}
