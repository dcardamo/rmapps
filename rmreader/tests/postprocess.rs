//! End-to-end check: assemble a 2-article doc where one article spans 2+ pages,
//! render to PDF, run the per-page nav post-processor, and assert that real
//! clickable nav links land on every article page — including the *second* page
//! of a flowing article, which fulgur leaves link-free.
use lopdf::{Document, Object, ObjectId};

use rmreader::assemble::assemble_document;
use rmreader::device::get_device;
use rmreader::embed;
use rmreader::manifest::{EmbeddedDoc, EmbeddedManifest, PageRange};
use rmreader::postprocess::finalize_pdf;
use rmreader::readwise::Document as RwDoc;
use rmreader::render::render_pdf;
use rmreader::theme::load_theme;

/// Build a minimal `EmbeddedManifest` with `n` placeholder docs (page_range all
/// zeros). `finalize_pdf` overwrites `page_range` once starts are resolved.
fn stub_embedded(n: usize) -> EmbeddedManifest {
    EmbeddedManifest {
        schema_version: 1,
        collection: "Library".into(),
        docs: (0..n)
            .map(|i| EmbeddedDoc {
                id: format!("doc{i}"),
                title: format!("Title {i}"),
                url: format!("https://ex/doc{i}"),
                author: "Auth".into(),
                category: "article".into(),
                page_range: PageRange { first: 0, last: 0 },
            })
            .collect(),
        label_rects: Vec::new(),
    }
}

fn doc(id: &str, html: &str) -> RwDoc {
    RwDoc {
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
        html_content: Some(html.into()),
    }
}

/// Ordered page object ids.
fn pages(d: &Document) -> Vec<ObjectId> {
    d.get_pages().into_values().collect()
}

/// Count /Link annotations across the whole doc.
fn count_links(d: &Document) -> usize {
    let mut n = 0;
    for pid in pages(d) {
        n += page_link_annots(d, pid).len();
    }
    n
}

/// The annotation dictionaries on a page that are /Subtype /Link.
fn page_link_annots(d: &Document, pid: ObjectId) -> Vec<lopdf::Dictionary> {
    let mut out = Vec::new();
    let Ok(page) = d.get_dictionary(pid) else {
        return out;
    };
    let Ok(annots) = page.get(b"Annots") else {
        return out;
    };
    let arr = match annots {
        Object::Array(a) => a.clone(),
        Object::Reference(id) => d
            .get_object(*id)
            .ok()
            .and_then(|o| o.as_array().ok())
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    for item in arr {
        let dict = match item {
            Object::Reference(id) => d.get_dictionary(id).ok().cloned(),
            Object::Dictionary(dd) => Some(dd),
            _ => None,
        };
        if let Some(dd) = dict {
            if dd.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) == Some(b"Link") {
                out.push(dd);
            }
        }
    }
    out
}

/// Resolve a /Link annotation's destination page id. fulgur stores `/Dest` as a
/// reference to a `[pageRef /XYZ ...]` array, so follow the ref then read elem 0.
/// Also handles an inline `/Dest [pageRef ...]` array.
fn link_dest_page(d: &Document, annot: &lopdf::Dictionary) -> Option<ObjectId> {
    let dest = annot.get(b"Dest").ok()?;
    let arr = match dest {
        Object::Array(a) => a.clone(),
        Object::Reference(id) => d.get_object(*id).ok()?.as_array().ok()?.clone(),
        _ => return None,
    };
    match arr.first()? {
        Object::Reference(id) => Some(*id),
        _ => None,
    }
}

/// Article start page indices: first N /Link dests in page order (the index rows).
fn article_starts(d: &Document, n: usize) -> Vec<usize> {
    let ordered = pages(d);
    let index: std::collections::HashMap<ObjectId, usize> =
        ordered.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    let mut starts = Vec::new();
    'outer: for &pid in &ordered {
        for annot in page_link_annots(d, pid) {
            if let Some(target) = link_dest_page(d, &annot) {
                if let Some(&i) = index.get(&target) {
                    starts.push(i);
                    if starts.len() == n {
                        break 'outer;
                    }
                }
            }
        }
    }
    starts
}

#[test]
fn nav_bar_adds_links_to_every_article_page_including_flow_pages() {
    let device = get_device("paper-pro-move").unwrap();
    let theme = load_theme("reader").unwrap();

    // Article "a" is long enough to flow across multiple pages; "b" is short.
    let long = "<p>Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do \
        eiusmod tempor incididunt ut labore et dolore magna aliqua.</p>"
        .repeat(60);
    let docs = vec![doc("a", &long), doc("b", "<p>Short article.</p>")];

    let built = assemble_document("Library", &docs, |html, _id| {
        (html.to_string(), vec![] as Vec<(String, Vec<u8>)>)
    });

    let out = std::env::temp_dir().join("rmreader_postprocess_test.pdf");
    render_pdf(&device, &theme, &built.fragments, &built.assets, &out).unwrap();

    // Before post-processing: locate article starts and capture the baseline.
    let before = Document::load(&out).unwrap();
    let starts = article_starts(&before, docs.len());
    assert_eq!(
        starts.len(),
        2,
        "expected 2 article-start dests, got {starts:?}"
    );
    let first_start = starts[0];
    let links_before = count_links(&before);

    // The first article must actually flow onto a 2nd page for this test to mean
    // anything; that 2nd page must have NO links pre-postprocess.
    let flow_page = first_start + 1;
    assert!(
        flow_page < starts[1],
        "article 'a' did not span 2+ pages (starts={starts:?}); make content longer"
    );
    assert_eq!(
        page_link_annots(&before, pages(&before)[flow_page]).len(),
        0,
        "expected the 2nd page of article 'a' to have no links before post-processing"
    );

    // Run the post-processor.
    let mut embedded = stub_embedded(docs.len());
    finalize_pdf(
        &out,
        docs.len(),
        device.width_pt(),
        device.height_pt(),
        "#F3F1EA",
        "#2A2F6B",
        "#F4F1E8",
        &mut embedded,
    )
    .unwrap();

    // After: total link count grew.
    let after = Document::load(&out).unwrap();
    let links_after = count_links(&after);
    assert!(
        links_after > links_before,
        "expected more links after post-processing: before={links_before} after={links_after}"
    );

    // (b) The flow page (2nd page of article 'a') now has at least one link.
    let after_pages = pages(&after);
    let flow_links = page_link_annots(&after, after_pages[flow_page]);
    assert!(
        !flow_links.is_empty(),
        "expected the flow page (index {flow_page}) to have nav links after post-processing"
    );

    // (c) A Home link on that flow page points to page index 0. Our nav writes
    // /Dest as an inline [pageRef /XYZ ...] array (not a ref), so link_dest_page
    // handles both forms.
    let page0 = after_pages[0];
    let found_home = flow_links
        .iter()
        .filter_map(|a| link_dest_page(&after, a))
        .any(|t| t == page0);
    assert!(
        found_home,
        "expected a Home link on the flow page whose /Dest resolves to page index 0"
    );

    // Sanity: the flow page (2nd page of the first article) should carry a Next
    // link pointing at article 'b' (the next article's start), proving Next
    // wiring on a non-first page of a flowing article.
    let next_start = after_pages[starts[1]];
    let found_next_to_b = flow_links
        .iter()
        .filter_map(|a| link_dest_page(&after, a))
        .any(|t| t == next_start);
    assert!(
        found_next_to_b,
        "expected a Next link on the flow page whose /Dest resolves to the next article's start"
    );
}

/// After `finalize_pdf`, the embedded manifest must contain 4 label rects
/// tiling the full page width in the top band, and each doc's `page_range`
/// must reflect the article page spans detected from the link annotations.
#[test]
fn embeds_manifest_with_page_ranges_and_label_rects() {
    let device = get_device("paper-pro-move").unwrap();
    let theme = load_theme("reader").unwrap();

    let long = "<p>Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do \
        eiusmod tempor incididunt ut labore et dolore magna aliqua.</p>"
        .repeat(60);
    let docs = vec![doc("a", &long), doc("b", "<p>Short article.</p>")];

    let built = assemble_document("Library", &docs, |html, _id| {
        (html.to_string(), vec![] as Vec<(String, Vec<u8>)>)
    });

    let out = std::env::temp_dir().join("rmreader_postprocess_manifest_test.pdf");
    render_pdf(&device, &theme, &built.fragments, &built.assets, &out).unwrap();

    let mut embedded = stub_embedded(docs.len());
    finalize_pdf(
        &out,
        docs.len(),
        device.width_pt(),
        device.height_pt(),
        "#F3F1EA",
        "#2A2F6B",
        "#F4F1E8",
        &mut embedded,
    )
    .unwrap();

    // Reload and read back the embedded manifest.
    let reloaded = Document::load(&out).unwrap();
    let got = embed::read(&reloaded)
        .expect("embed::read should not error")
        .expect("embedded manifest should be present");

    let page_w = f64::from(device.width_pt());
    let page_h = f64::from(device.height_pt());

    // --- label rects ---
    assert_eq!(got.label_rects.len(), 4, "expected 4 label rects");
    let kinds: Vec<&str> = got.label_rects.iter().map(|l| l.kind.as_str()).collect();
    assert_eq!(kinds, ["inbox", "archive", "later", "delete"]);

    // Columns must tile the full page width.
    let eps = 0.5_f64; // tolerance for floating-point representation
    assert!(
        (got.label_rects[0].rect.x0 - 0.0).abs() < eps,
        "first column x0 should be ~0, got {}",
        got.label_rects[0].rect.x0
    );
    assert!(
        (got.label_rects[3].rect.x1 - page_w).abs() < eps,
        "last column x1 should be ~{page_w}, got {}",
        got.label_rects[3].rect.x1
    );
    // Adjacent columns must meet.
    for i in 0..3 {
        let gap = (got.label_rects[i + 1].rect.x0 - got.label_rects[i].rect.x1).abs();
        assert!(
            gap < eps,
            "columns {i} and {} do not meet: x1={} x0={}",
            i + 1,
            got.label_rects[i].rect.x1,
            got.label_rects[i + 1].rect.x0
        );
    }
    // Rects must lie in the action band: y ∈ [page_h-112, page_h-84].
    for lr in &got.label_rects {
        assert!(
            lr.rect.y1 <= page_h - 84.0 + 0.5,
            "label rect y1={} should be at or below page_h-84 ({:.1})",
            lr.rect.y1,
            page_h - 84.0
        );
        assert!(
            lr.rect.y0 >= page_h - 112.0 - 0.5,
            "label rect y0={} should be at or above page_h-112 ({:.1})",
            lr.rect.y0,
            page_h - 112.0
        );
    }

    // --- page ranges ---
    // Both docs should have non-trivial page ranges (last >= first).
    for (i, d) in got.docs.iter().enumerate() {
        assert!(
            d.page_range.last >= d.page_range.first,
            "doc {i} page_range is invalid: {:?}",
            d.page_range
        );
    }
    // Doc 'a' (long) must span at least 2 pages; doc 'b' (short) exactly 1.
    assert!(
        got.docs[0].page_range.last > got.docs[0].page_range.first,
        "doc 'a' should span multiple pages, got {:?}",
        got.docs[0].page_range
    );
    // Pages are consecutive: doc 'b' starts right after doc 'a' ends.
    assert_eq!(
        got.docs[1].page_range.first,
        got.docs[0].page_range.last + 1,
        "doc 'b' should start immediately after doc 'a' ends"
    );

    // Print label rects so the caller can sanity-check the column/band geometry.
    for lr in &got.label_rects {
        eprintln!(
            "[test] label_rect {:8} x=[{:.1},{:.1}] y=[{:.1},{:.1}]",
            lr.kind, lr.rect.x0, lr.rect.x1, lr.rect.y0, lr.rect.y1
        );
    }
}

/// After `finalize_pdf`, every article page's content stream must contain the
/// label words INBOX, ARCHIVE, LATER, DELETE.
#[test]
fn stamps_label_text_on_article_pages() {
    let device = get_device("paper-pro-move").unwrap();
    let theme = load_theme("reader").unwrap();

    let docs = vec![
        doc("x", "<p>Article X content.</p>"),
        doc("y", "<p>Article Y content.</p>"),
    ];

    let built = assemble_document("Library", &docs, |html, _id| {
        (html.to_string(), vec![] as Vec<(String, Vec<u8>)>)
    });

    let out = std::env::temp_dir().join("rmreader_postprocess_labels_test.pdf");
    render_pdf(&device, &theme, &built.fragments, &built.assets, &out).unwrap();

    let mut embedded = stub_embedded(docs.len());
    finalize_pdf(
        &out,
        docs.len(),
        device.width_pt(),
        device.height_pt(),
        "#F3F1EA",
        "#2A2F6B",
        "#F4F1E8",
        &mut embedded,
    )
    .unwrap();

    // Collect raw content-stream bytes from all article pages and search for
    // each label word (uppercase, as stamped by the postprocessor).
    let doc = Document::load(&out).unwrap();
    let all_pages = pages(&doc);

    // Article pages start at the second page (index 1; index page is page 0).
    // We just need to confirm at least one article page contains the labels.
    let mut found_inbox = false;
    let mut found_archive = false;
    let mut found_later = false;
    let mut found_delete = false;

    for &pid in all_pages.iter().skip(1) {
        // Collect all content stream bytes for this page.
        let page_dict = doc.get_dictionary(pid).unwrap();
        let contents = match page_dict.get(b"Contents") {
            Ok(Object::Array(a)) => a.clone(),
            Ok(Object::Reference(id)) => vec![Object::Reference(*id)],
            _ => continue,
        };
        let mut page_bytes = Vec::new();
        for item in &contents {
            let sid = match item {
                Object::Reference(id) => *id,
                _ => continue,
            };
            if let Ok(stream) = doc.get_object(sid).and_then(|o| o.as_stream()) {
                if let Ok(b) = stream.decompressed_content() {
                    page_bytes.extend_from_slice(&b);
                } else {
                    page_bytes.extend_from_slice(&stream.content);
                }
            }
        }
        let text = String::from_utf8_lossy(&page_bytes);
        if text.contains("INBOX") {
            found_inbox = true;
        }
        if text.contains("ARCHIVE") {
            found_archive = true;
        }
        if text.contains("LATER") {
            found_later = true;
        }
        if text.contains("DELETE") {
            found_delete = true;
        }
    }

    assert!(
        found_inbox,
        "expected INBOX label in article page content streams"
    );
    assert!(
        found_archive,
        "expected ARCHIVE label in article page content streams"
    );
    assert!(
        found_later,
        "expected LATER label in article page content streams"
    );
    assert!(
        found_delete,
        "expected DELETE label in article page content streams"
    );
}
