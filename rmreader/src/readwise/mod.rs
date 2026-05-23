//! Readwise Reader API client (https://readwise.io/reader_api).
pub mod http;

use serde::{Deserialize, Deserializer};

const LIST_URL: &str = "https://readwise.io/api/v3/list/";
const AUTH_URL: &str = "https://readwise.io/api/v2/auth/";
const UPDATE_URL: &str = "https://readwise.io/api/v3/update/";
const DELETE_URL: &str = "https://readwise.io/api/v3/delete/";
const HL_URL: &str = "https://readwise.io/api/v2/highlights/";

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub retry_after: Option<u64>,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Patch,
    Delete,
    Post,
}

/// Low-level seam so pagination/sort/rate-limit are testable without network.
pub trait HttpTransport {
    fn request(
        &self,
        method: HttpMethod,
        url: &str,
        token: &str,
        body: Option<&str>,
    ) -> anyhow::Result<HttpResponse>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Inbox,
    Later,
    Archive,
    Delete,
}

impl ActionKind {
    /// The Readwise `location` value for a move action; None for Delete (different endpoint).
    pub fn location(self) -> Option<&'static str> {
        match self {
            ActionKind::Inbox => Some("new"),
            ActionKind::Later => Some("later"),
            ActionKind::Archive => Some("archive"),
            ActionKind::Delete => None,
        }
    }

    /// Parse an action label word (case-insensitive) into a kind.
    pub fn parse_label(s: &str) -> Option<ActionKind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "inbox" => Some(ActionKind::Inbox),
            "later" => Some(ActionKind::Later),
            "archive" => Some(ActionKind::Archive),
            "delete" => Some(ActionKind::Delete),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct HighlightCreate {
    pub text: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub author: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub category: String,
}

/// Create highlights via the classic v2 endpoint. Readwise matches each highlight
/// to a document by `source_url` (+ title/author). Empty input is a no-op.
pub fn create_highlights(
    t: &dyn HttpTransport,
    token: &str,
    items: &[HighlightCreate],
) -> anyhow::Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    let body = serde_json::json!({ "highlights": items }).to_string();
    let r = t.request(HttpMethod::Post, HL_URL, token, Some(&body))?;
    anyhow::ensure!(
        (200..300).contains(&r.status),
        "create_highlights failed: HTTP {}",
        r.status
    );
    Ok(())
}

pub fn update_location(
    t: &dyn HttpTransport,
    token: &str,
    id: &str,
    loc: &str,
) -> anyhow::Result<()> {
    let url = format!("{UPDATE_URL}{id}/");
    let body = serde_json::json!({ "location": loc }).to_string();
    let r = t.request(HttpMethod::Patch, &url, token, Some(&body))?;
    anyhow::ensure!(
        (200..300).contains(&r.status),
        "update {id} -> {loc} failed: HTTP {}",
        r.status
    );
    Ok(())
}

pub fn delete_document(t: &dyn HttpTransport, token: &str, id: &str) -> anyhow::Result<()> {
    let url = format!("{DELETE_URL}{id}/");
    let r = t.request(HttpMethod::Delete, &url, token, None)?;
    // Readwise returns 204 on delete; accept any 2xx defensively.
    anyhow::ensure!(
        (200..300).contains(&r.status),
        "delete {id} failed: HTTP {}",
        r.status
    );
    Ok(())
}

/// Deserialize a possibly-`null` value into `T`'s default. The real Readwise API
/// returns explicit `null` for string fields like `author`/`image_url` on some
/// documents, which a plain `String` field would reject; this coerces null →
/// default (and `#[serde(default)]` handles the absent case).
fn null_to_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// `reading_time` comes back as a human string ("3 mins") in the real API, but
/// older docs / other shapes may send a number of minutes. Normalize either into
/// a display-ready string; null/empty → None.
fn reading_time_display<'de, D>(d: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde_json::Value;
    Ok(match Option::<Value>::deserialize(d)? {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        Some(Value::Number(n)) => Some(format!("{n} min")),
        Some(v) => Some(v.to_string()),
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct Document {
    pub id: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub url: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub source_url: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub title: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub author: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub site_name: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub category: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub location: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub summary: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub image_url: String,
    #[serde(default)]
    pub word_count: Option<u32>,
    #[serde(default, deserialize_with = "reading_time_display")]
    pub reading_time: Option<String>,
    #[serde(default)]
    pub published_date: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub saved_at: String,
    #[serde(default)]
    pub html_content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListResponse {
    #[serde(rename = "nextPageCursor", default)]
    next_page_cursor: Option<String>,
    results: Vec<Document>,
}

fn list_url(location: &str, cursor: Option<&str>) -> String {
    // limit=50 (not the max 100): a withHtmlContent page of 100 full articles is a
    // multi-MB response that can exceed the read timeout; smaller pages are far more
    // reliable and still well within the 20-requests/minute list rate limit.
    let mut u = format!("{LIST_URL}?withHtmlContent=true&limit=50&location={location}");
    if let Some(c) = cursor {
        u.push_str(&format!("&pageCursor={c}"));
    }
    u
}

/// Validate a token: GET /api/v2/auth/ returns 204 when valid.
pub fn validate_token(t: &dyn HttpTransport, token: &str) -> anyhow::Result<()> {
    let r = t.request(HttpMethod::Get, AUTH_URL, token, None)?;
    if r.status == 204 || r.status == 200 {
        Ok(())
    } else {
        anyhow::bail!("Readwise token rejected (HTTP {})", r.status)
    }
}

/// Fetch + merge + dedupe + sort(saved_at desc) + cap. `sleep` is injected so
/// tests can assert Retry-After handling without real delays.
pub fn fetch_documents(
    t: &dyn HttpTransport,
    token: &str,
    locations: &[String],
    max_items: u32,
    mut sleep: impl FnMut(u64),
) -> anyhow::Result<Vec<Document>> {
    let mut all: Vec<Document> = Vec::new();
    for loc in locations {
        let mut cursor: Option<String> = None;
        let mut got = 0usize;
        loop {
            let url = list_url(loc, cursor.as_deref());
            // Retry transient failures (429 rate-limit, 5xx server errors like the
            // occasional 502) a few times before giving up; other non-200s are fatal.
            let mut attempt = 0u32;
            let resp = loop {
                let r = t.request(HttpMethod::Get, &url, token, None)?;
                let transient = r.status == 429 || (500..=599).contains(&r.status);
                if transient && attempt < 5 {
                    attempt += 1;
                    let wait = r
                        .retry_after
                        .unwrap_or(if r.status == 429 { 60 } else { 3 });
                    sleep(wait);
                    continue;
                }
                break r;
            };
            if resp.status != 200 {
                anyhow::bail!(
                    "Readwise list failed (HTTP {}) for location {loc}",
                    resp.status
                );
            }
            let parsed: ListResponse = serde_json::from_str(&resp.body)?;
            got += parsed.results.len();
            all.extend(parsed.results);
            // The API returns each location newest-first, and we sort + cap below, so
            // the newest `max_items` are covered by the first `max_items` of each
            // location. Stop once we have enough instead of draining the location —
            // `feed` alone can hold tens of thousands of items, which is fatal to fetch
            // in full with html_content.
            match parsed.next_page_cursor {
                Some(c) if !c.is_empty() && got < max_items as usize => cursor = Some(c),
                _ => break,
            }
        }
    }
    // dedupe by id (keep first seen)
    let mut seen = std::collections::HashSet::new();
    all.retain(|d| seen.insert(d.id.clone()));
    // sort newest first by saved_at (ISO 8601 sorts lexicographically)
    all.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));
    all.truncate(max_items as usize);
    Ok(all)
}
