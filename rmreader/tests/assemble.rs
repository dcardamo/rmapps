use rmreader::assemble::assemble_document;
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
    // anchors present
    assert!(html.contains("id=\"item-a\""));
    assert!(html.contains("id=\"article-a\""));
    assert!(html.contains("href=\"#item-a\"")); // index -> card
    assert!(html.contains("href=\"#article-a\"")); // card -> article
                                                   // manifest
    assert_eq!(built.manifest.items.len(), 2);
    assert_eq!(built.manifest.items[0].id, "a");
    assert_eq!(built.manifest.items[0].card_anchor, "item-a");
    assert_eq!(built.manifest.items[0].article_anchor, "article-a");
}
