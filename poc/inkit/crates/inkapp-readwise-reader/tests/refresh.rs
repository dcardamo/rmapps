//! Manual cassette refresh from real Readwise plus the pluggable-fetch /
//! cache-backed refresh tests.
//!
//! Manual cassette capture (writes the committed fixture):
//!   READWISE_TOKEN=xxxx nix develop -c cargo test -p inkapp-readwise-reader --test refresh -- --ignored refresh_cassette
//!
//! Reads the token from READWISE_TOKEN (the operator's rmreader credential),
//! fetches the reading list via `curl`, and rewrites fixtures/cassette/articles.json.
//! Shells out to `curl` to avoid a TLS dependency for a manual bar.

use std::process::Command;
use std::sync::Arc;

use inkapp_core::connector::{Connector, ConnectorError};
use inkapp_readwise_reader::{Article, ArticleId, FetchTransport, Location, Page, Readwise};

#[test]
#[ignore = "manual: requires READWISE_TOKEN + curl; writes the committed cassette"]
fn refresh_cassette() {
    let token = std::env::var("READWISE_TOKEN").expect("set READWISE_TOKEN");

    let out = Command::new("curl")
        .args([
            "-sS",
            "-H",
            &format!("Authorization: Token {token}"),
            "https://readwise.io/api/v2/books/?category=article&page_size=5",
        ])
        .output()
        .expect("run curl");
    assert!(
        out.status.success(),
        "curl failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let books: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("parse readwise json");

    let mut articles = Vec::new();
    if let Some(results) = books["results"].as_array() {
        for b in results.iter().take(3) {
            let id = match &b["id"] {
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::String(s) => s.clone(),
                other => panic!("unexpected Readwise id type: {other}"),
            };
            let title = b["title"].as_str().unwrap_or("Untitled").to_string();
            // The list endpoint returns metadata, not full text. For the cassette
            // a short representative body suffices (pagination is deferred this spec);
            // use the title as the body stand-in.
            let body = title.clone();
            articles.push(serde_json::json!({
                "id": id, "title": title, "body": body, "highlights": []
            }));
        }
    }
    assert!(!articles.is_empty(), "fetched at least one article");

    let out_json = serde_json::json!({ "articles": articles });
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/cassette/articles.json"
    );
    std::fs::write(path, serde_json::to_string_pretty(&out_json).unwrap()).expect("write cassette");
    eprintln!(
        "wrote {} article(s) to the committed cassette",
        articles.len()
    );
}

// ── pluggable fetch + cache-backed refresh ────────────────────────────────────

fn art(id: &str, saved: &str) -> Article {
    Article {
        id: ArticleId::new(id),
        title: id.into(),
        saved_at: saved.into(),
        ..Default::default()
    }
}

struct TwoPages;
#[async_trait::async_trait]
impl FetchTransport for TwoPages {
    async fn list(&self, location: &str, cursor: Option<&str>) -> Result<Page, ConnectorError> {
        if location != "new" {
            return Ok(Page {
                articles: vec![],
                next_cursor: None,
            });
        }
        match cursor {
            None => Ok(Page {
                articles: vec![art("a", "2024-01-02")],
                next_cursor: Some("c2".into()),
            }),
            Some("c2") => Ok(Page {
                articles: vec![art("b", "2024-01-03"), art("a", "2024-01-02")],
                next_cursor: None,
            }),
            _ => Ok(Page {
                articles: vec![],
                next_cursor: None,
            }),
        }
    }
}

#[tokio::test]
async fn pages_dedupes_and_sorts() {
    let rw = Readwise::fake()
        .with_fetch(Arc::new(TwoPages))
        .with_locations(vec!["new".into()]);
    rw.refresh().await.unwrap();
    let ids: Vec<String> = rw.queue().iter().map(|a| a.id.0.clone()).collect();
    assert_eq!(ids, vec!["b".to_string(), "a".to_string()]); // newest first, deduped
}

#[tokio::test]
async fn fetch_error_preserves_prior_warm_cache() {
    struct AlwaysError;
    #[async_trait::async_trait]
    impl FetchTransport for AlwaysError {
        async fn list(&self, _location: &str, _c: Option<&str>) -> Result<Page, ConnectorError> {
            Err(ConnectorError::Transport("boom".into()))
        }
    }

    // First refresh populates the warm cache from TwoPages.
    let rw = Readwise::fake()
        .with_fetch(Arc::new(TwoPages))
        .with_locations(vec!["new".into()]);
    rw.refresh().await.unwrap();
    let before: Vec<String> = rw.queue().iter().map(|a| a.id.0.clone()).collect();
    assert_eq!(before, vec!["b".to_string(), "a".to_string()]);

    // A failing fetch must return Err and leave the warm cache untouched.
    let rw = rw.with_fetch(Arc::new(AlwaysError));
    assert!(rw.refresh().await.is_err(), "fetch error surfaces as Err");
    let after: Vec<String> = rw.queue().iter().map(|a| a.id.0.clone()).collect();
    assert_eq!(after, before, "prior warm cache preserved on fetch error");
}

#[tokio::test]
async fn warm_restart_serves_from_durable_cache() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Arc::new(
        inkapp_core::cache::Cache::open(dir.path(), 1 << 20, 8 << 20)
            .await
            .unwrap(),
    );
    {
        let rw = Readwise::fake()
            .with_fetch(Arc::new(TwoPages))
            .with_locations(vec!["new".into()])
            .with_cache(cache.clone());
        rw.refresh().await.unwrap();
        cache.close().await.unwrap();
    }
    let cache2 = Arc::new(
        inkapp_core::cache::Cache::open(dir.path(), 1 << 20, 8 << 20)
            .await
            .unwrap(),
    );
    let rw2 = Readwise::fake().with_cache_hydrated(cache2).await;
    let ids: Vec<String> = rw2.queue().iter().map(|a| a.id.0.clone()).collect();
    assert_eq!(ids, vec!["b".to_string(), "a".to_string()]);
}

#[tokio::test]
async fn refresh_prunes_applied_overlay_entry() {
    struct OnlyB;
    #[async_trait::async_trait]
    impl FetchTransport for OnlyB {
        async fn list(&self, location: &str, _c: Option<&str>) -> Result<Page, ConnectorError> {
            if location == "new" {
                Ok(Page {
                    articles: vec![art("b", "2024-01-03")],
                    next_cursor: None,
                })
            } else {
                Ok(Page {
                    articles: vec![],
                    next_cursor: None,
                })
            }
        }
    }
    let rw = Readwise::fake()
        .with_fetch(Arc::new(TwoPages))
        .with_locations(vec!["new".into()]);
    rw.refresh().await.unwrap();
    let a = ArticleId::new("a");
    rw.archive(&a);
    assert!(rw.queue().iter().all(|x| x.id != a), "a hidden by overlay");
    let rw = rw.with_fetch(Arc::new(OnlyB)); // server now omits a (archive applied)
    rw.refresh().await.unwrap();
    assert!(
        rw.archived().iter().all(|id| id != &a),
        "applied overlay entry pruned"
    );
}

#[tokio::test]
async fn library_and_feed_filter_by_location() {
    struct Mixed;
    #[async_trait::async_trait]
    impl FetchTransport for Mixed {
        async fn list(&self, location: &str, _c: Option<&str>) -> Result<Page, ConnectorError> {
            let a = |id: &str, loc: Location| Article {
                id: ArticleId::new(id),
                title: id.into(),
                location: loc,
                saved_at: "2024".into(),
                ..Default::default()
            };
            let arts = match location {
                "new" => vec![a("n", Location::New)],
                "later" => vec![a("l", Location::Later)],
                "feed" => vec![a("f", Location::Feed)],
                _ => vec![],
            };
            Ok(Page {
                articles: arts,
                next_cursor: None,
            })
        }
    }
    let rw = Readwise::fake()
        .with_fetch(Arc::new(Mixed))
        .with_locations(vec!["new".into(), "later".into(), "feed".into()]);
    rw.refresh().await.unwrap();
    let lib: Vec<String> = rw.library().iter().map(|a| a.id.0.clone()).collect();
    assert!(
        lib.contains(&"n".to_string()) && lib.contains(&"l".to_string()),
        "lib has new+later"
    );
    assert!(!lib.contains(&"f".to_string()), "feed item not in library");
    assert_eq!(
        rw.feed().iter().map(|a| a.id.0.clone()).collect::<Vec<_>>(),
        vec!["f".to_string()]
    );
}
