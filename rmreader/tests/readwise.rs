use rmreader::readwise::{fetch_documents, HttpResponse, HttpTransport};
use std::cell::RefCell;

/// Fake transport returning canned responses per URL substring.
struct Fake {
    calls: RefCell<Vec<String>>,
    script: Vec<(u16, Option<u64>, String)>,
    idx: RefCell<usize>,
}
impl HttpTransport for Fake {
    fn get(&self, url: &str, _token: &str) -> anyhow::Result<HttpResponse> {
        self.calls.borrow_mut().push(url.to_string());
        let mut i = self.idx.borrow_mut();
        let (status, retry, body) = self.script[*i].clone();
        *i += 1;
        Ok(HttpResponse {
            status,
            retry_after: retry,
            body,
        })
    }
}

fn page(results: &str, cursor: Option<&str>) -> String {
    let c = cursor.map(|x| format!("\"{x}\"")).unwrap_or("null".into());
    format!("{{\"count\":1,\"nextPageCursor\":{c},\"results\":[{results}]}}")
}
fn doc(id: &str, saved: &str) -> String {
    format!("{{\"id\":\"{id}\",\"title\":\"T{id}\",\"saved_at\":\"{saved}\",\"location\":\"new\",\"category\":\"article\",\"html_content\":\"<p>x</p>\"}}")
}

#[test]
fn parses_real_world_field_shapes() {
    // Real API: reading_time is a string ("3 mins"); some fields come back null.
    let body = r#"{"nextPageCursor":null,"results":[{"id":"x","title":"T","saved_at":"2026-01-01T00:00:00Z","reading_time":"3 mins","word_count":500,"author":null,"image_url":null,"summary":null,"html_content":"<p>hi</p>"}]}"#;
    let fake = Fake {
        calls: RefCell::new(vec![]),
        script: vec![(200, None, body.to_string())],
        idx: RefCell::new(0),
    };
    let docs = fetch_documents(&fake, "tok", &["new".into()], 10, |_| {}).unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].reading_time.as_deref(), Some("3 mins"));
    assert_eq!(docs[0].author, ""); // null -> default
    assert_eq!(docs[0].image_url, ""); // null -> default
    assert_eq!(docs[0].summary, ""); // null -> default
    assert_eq!(docs[0].word_count, Some(500));
}

#[test]
fn paginates_and_sorts_desc_and_caps() {
    // two pages on one location, returned newest-first after sort, capped to 2.
    let script = vec![
        (
            200,
            None,
            page(
                &format!(
                    "{},{}",
                    doc("a", "2026-01-01T00:00:00Z"),
                    doc("b", "2026-03-01T00:00:00Z")
                ),
                Some("CUR"),
            ),
        ),
        (200, None, page(&doc("c", "2026-02-01T00:00:00Z"), None)),
    ];
    let fake = Fake {
        calls: RefCell::new(vec![]),
        script,
        idx: RefCell::new(0),
    };
    let docs = fetch_documents(&fake, "tok", &["new".into()], 3, |_| {}).unwrap();
    assert_eq!(
        docs.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(),
        vec!["b", "c", "a"]
    );
    assert!(fake.calls.borrow()[1].contains("pageCursor=CUR"));
    assert!(fake.calls.borrow()[0].contains("withHtmlContent=true"));
    assert!(fake.calls.borrow()[0].contains("location=new"));
}

#[test]
fn stops_paginating_once_max_items_reached() {
    // The first page already satisfies max_items, so the second page must NOT be
    // fetched — `feed` has tens of thousands of items and draining it is fatal.
    let script = vec![
        (
            200,
            None,
            page(
                &format!(
                    "{},{}",
                    doc("a", "2026-01-01T00:00:00Z"),
                    doc("b", "2026-02-01T00:00:00Z")
                ),
                Some("CUR"),
            ),
        ),
        (200, None, page(&doc("c", "2026-03-01T00:00:00Z"), None)),
    ];
    let fake = Fake {
        calls: RefCell::new(vec![]),
        script,
        idx: RefCell::new(0),
    };
    let docs = fetch_documents(&fake, "tok", &["new".into()], 2, |_| {}).unwrap();
    assert_eq!(docs.len(), 2);
    assert_eq!(
        fake.calls.borrow().len(),
        1,
        "must stop after the first page once max_items is reached"
    );
}

#[test]
fn retries_after_429() {
    let mut slept = 0u64;
    let script = vec![
        (429, Some(7), String::new()),
        (200, None, page(&doc("a", "2026-01-01T00:00:00Z"), None)),
    ];
    let fake = Fake {
        calls: RefCell::new(vec![]),
        script,
        idx: RefCell::new(0),
    };
    let docs = fetch_documents(&fake, "tok", &["new".into()], 10, |s| slept += s).unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(slept, 7);
}

#[test]
fn dedupes_across_locations() {
    let script = vec![
        (200, None, page(&doc("a", "2026-01-01T00:00:00Z"), None)), // new
        (200, None, page(&doc("a", "2026-01-01T00:00:00Z"), None)), // later (same id)
    ];
    let fake = Fake {
        calls: RefCell::new(vec![]),
        script,
        idx: RefCell::new(0),
    };
    let docs = fetch_documents(&fake, "tok", &["new".into(), "later".into()], 10, |_| {}).unwrap();
    assert_eq!(docs.len(), 1);
}
