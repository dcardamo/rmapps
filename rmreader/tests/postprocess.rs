//! End-to-end check: assemble a 2-article doc where one article spans 2+ pages,
//! render to PDF, run the per-page nav post-processor, and assert that real
//! clickable nav links land on every article page — including the *second* page
//! of a flowing article, which fulgur leaves link-free.
use lopdf::{Document, Object, ObjectId};

use rmreader::assemble::assemble_document;
use rmreader::device::get_device;
use rmreader::postprocess::add_per_page_nav;
use rmreader::readwise::Document as RwDoc;
use rmreader::render::render_pdf;
use rmreader::theme::load_theme;

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
    add_per_page_nav(&out, docs.len(), device.width_pt(), device.height_pt()).unwrap();

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
