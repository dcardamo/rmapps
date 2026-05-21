//! Readwise Reader API client (https://readwise.io/reader_api).
pub mod http;

use serde::Deserialize;

const LIST_URL: &str = "https://readwise.io/api/v3/list/";
const AUTH_URL: &str = "https://readwise.io/api/v2/auth/";

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub retry_after: Option<u64>,
    pub body: String,
}

/// Low-level seam so pagination/sort/rate-limit are testable without network.
pub trait HttpTransport {
    fn get(&self, url: &str, token: &str) -> anyhow::Result<HttpResponse>;
}

#[derive(Debug, Clone, Deserialize)]
pub struct Document {
    pub id: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub source_url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub site_name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub image_url: String,
    #[serde(default)]
    pub word_count: Option<u32>,
    #[serde(default)]
    pub reading_time: Option<u32>,
    #[serde(default)]
    pub published_date: Option<String>,
    #[serde(default)]
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
    let mut u = format!("{LIST_URL}?withHtmlContent=true&limit=100&location={location}");
    if let Some(c) = cursor {
        u.push_str(&format!("&pageCursor={c}"));
    }
    u
}

/// Validate a token: GET /api/v2/auth/ returns 204 when valid.
pub fn validate_token(t: &dyn HttpTransport, token: &str) -> anyhow::Result<()> {
    let r = t.get(AUTH_URL, token)?;
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
        loop {
            let url = list_url(loc, cursor.as_deref());
            let resp = t.get(&url, token)?;
            if resp.status == 429 {
                sleep(resp.retry_after.unwrap_or(60));
                continue; // retry same cursor
            }
            if resp.status != 200 {
                anyhow::bail!("Readwise list failed (HTTP {}) for location {loc}", resp.status);
            }
            let parsed: ListResponse = serde_json::from_str(&resp.body)?;
            all.extend(parsed.results);
            match parsed.next_page_cursor {
                Some(c) if !c.is_empty() => cursor = Some(c),
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
