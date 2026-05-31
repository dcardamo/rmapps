use rmreader::assemble::assemble_document;
use rmreader::manifest::{Manifest, ManifestItem, PageRange};
use rmreader::readwise::Document;

fn doc(id: &str) -> Document {
    Document {
        id: id.into(),
        url: format!("https://ex/{id}"),
        source_url: String::new(),
        title: format!("Title {id}"),
        author: "Auth".into(),
        site_name: "Site".into(),
        category: "article".into(),
        location: "new".into(),
        summary: "Sum".into(),
        image_url: String::new(),
        word_count: Some(500),
        reading_time: Some("3 min".into()),
        published_date: None,
        saved_at: "2026-01-01T00:00:00Z".into(),
        html_content: Some("<p>Body</p>".into()),
    }
}

#[test]
fn builds_linked_document_and_manifest() {
    let docs = vec![doc("a"), doc("b")];
    // content_fn: identity-ish (returns processed html + no assets)
    let built = assemble_document("Library", &docs, |html, _id| {
        (html.to_string(), vec![] as Vec<(String, Vec<u8>)>)
    });
    let html = built.fragments.join("\n");
    // No summary-card tier: index links straight to the article.
    assert!(html.contains("id=\"article-a\""));
    assert!(html.contains("href=\"#article-a\"")); // index -> article
    assert!(!html.contains("item-a")); // no card section/anchors
                                       // manifest
    assert_eq!(built.manifest.items.len(), 2);
    assert_eq!(built.manifest.items[0].id, "a");
    assert_eq!(built.manifest.items[0].article_anchor, "article-a");
}

#[test]
fn manifest_items_carry_readwise_metadata() {
    let mut d = doc("a");
    d.author = "Jane Doe".into();
    d.source_url = "https://original.example/article".into();
    d.category = "tweet".into();

    let built = assemble_document("Library", &[d], |html, _id| {
        (html.to_string(), vec![] as Vec<(String, Vec<u8>)>)
    });

    let item = &built.manifest.items[0];
    assert_eq!(item.author, "Jane Doe");
    assert_eq!(item.source_url, "https://original.example/article");
    assert_eq!(item.category, "tweet");
}

#[test]
fn to_embedded_prefers_source_url_and_defaults_page_range() {
    // Case 1: source_url present — embedded url should use it.
    let manifest_with_source = Manifest {
        collection: "Feed".into(),
        items: vec![ManifestItem {
            id: "x1".into(),
            title: "Test Article".into(),
            url: "https://readwise.io/reader/x1".into(),
            article_anchor: "article-x1".into(),
            author: "Alice".into(),
            source_url: "https://original.example/x1".into(),
            category: "article".into(),
            page_range: None,
        }],
    };

    let embedded = manifest_with_source.to_embedded();
    assert_eq!(embedded.schema_version, 1);
    assert_eq!(embedded.collection, "Feed");
    assert_eq!(embedded.docs.len(), 1);
    let d = &embedded.docs[0];
    assert_eq!(d.url, "https://original.example/x1"); // prefers source_url
    assert_eq!(d.page_range, PageRange { first: 0, last: 0 });

    // Case 2: source_url empty — embedded url falls back to item url.
    let manifest_no_source = Manifest {
        collection: "Library".into(),
        items: vec![ManifestItem {
            id: "x2".into(),
            title: "Another Article".into(),
            url: "https://readwise.io/reader/x2".into(),
            article_anchor: "article-x2".into(),
            author: "Bob".into(),
            source_url: String::new(),
            category: "article".into(),
            page_range: None,
        }],
    };

    let embedded2 = manifest_no_source.to_embedded();
    let d2 = &embedded2.docs[0];
    assert_eq!(d2.url, "https://readwise.io/reader/x2"); // falls back to url
    assert_eq!(d2.page_range, PageRange { first: 0, last: 0 });
}
