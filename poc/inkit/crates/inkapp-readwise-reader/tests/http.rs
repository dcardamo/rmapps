use inkapp_readwise_reader::http::{build_list_url, highlight_body, parse_list, ListResponse};
use inkapp_readwise_reader::Location;

#[test]
fn list_url_has_expected_query() {
    let u = build_list_url("later", Some("CUR"));
    assert!(u.contains("location=later"), "{u}");
    assert!(u.contains("withHtmlContent=true"), "{u}");
    assert!(u.contains("pageCursor=CUR"), "{u}");
    assert!(u.contains("limit="), "{u}");
    let u2 = build_list_url("new", None);
    assert!(!u2.contains("pageCursor"), "no cursor param when None");
}

#[test]
fn parses_reader_list_json() {
    let raw = r#"{
      "nextPageCursor": "NEXT",
      "results": [{
        "id": "01", "url": "https://readwise.io/read/01",
        "source_url": "https://example.com/x", "title": "T", "author": "A",
        "site_name": "Site", "category": "article", "location": "later",
        "summary": "S", "image_url": "https://img/x.png", "word_count": 1200,
        "reading_time": "5 min", "published_date": "2024-01-01",
        "saved_at": "2024-02-02T00:00:00Z",
        "html_content": "<p>hi</p>"
      }]
    }"#;
    let ListResponse {
        articles,
        next_cursor,
    } = parse_list(raw).unwrap();
    assert_eq!(next_cursor.as_deref(), Some("NEXT"));
    let a = &articles[0];
    assert_eq!(a.id.0, "01");
    assert_eq!(a.location, Location::Later);
    assert_eq!(a.html_content.as_deref(), Some("<p>hi</p>"));
    assert_eq!(a.source_url, "https://example.com/x");
    assert_eq!(a.title, "T");
    assert_eq!(a.author, "A");
    assert_eq!(a.site_name, "Site");
    assert_eq!(a.summary, "S");
    assert_eq!(a.word_count, Some(1200));
    assert_eq!(a.category, "article");
}

#[test]
fn highlight_body_matches_v2_shape() {
    let body = highlight_body(
        "the text",
        "Title",
        "Author",
        "https://example.com/x",
        "articles",
    );
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let h = &v["highlights"][0];
    assert_eq!(h["text"], "the text");
    assert_eq!(h["source_url"], "https://example.com/x");
    assert_eq!(h["category"], "articles");
    assert_eq!(h["title"], "Title");
    assert_eq!(h["author"], "Author");
}

#[test]
fn status_error_maps_correctly() {
    use inkapp_core::connector::ConnectorError;
    use inkapp_readwise_reader::http::status_error;

    assert!(status_error(200).is_none());
    assert!(status_error(204).is_none());
    assert!(matches!(status_error(401), Some(ConnectorError::Auth(_))));
    assert!(matches!(status_error(403), Some(ConnectorError::Auth(_))));
    assert!(matches!(
        status_error(429),
        Some(ConnectorError::RateLimited)
    ));
    assert!(matches!(
        status_error(500),
        Some(ConnectorError::Transport(_))
    ));
}
