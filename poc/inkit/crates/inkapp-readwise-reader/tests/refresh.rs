//! Manual cassette refresh from real Readwise. Captures a few real articles into
//! the committed cassette so tests run on real-shaped data.
//!
//! Run:
//!   READWISE_TOKEN=xxxx nix develop -c cargo test -p inkapp-readwise-reader --test refresh -- --ignored refresh_cassette
//!
//! Reads the token from READWISE_TOKEN (the operator's rmreader credential),
//! fetches the reading list via `curl`, and rewrites fixtures/cassette/articles.json.
//! Shells out to `curl` to avoid a TLS dependency for a manual bar.

use std::process::Command;

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
