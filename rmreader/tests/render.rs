//! Integration tests for the Typst render path: clickable links, a clean
//! per-glyph text layer (the whole reason we left fulgur), and byte-determinism.
use rmreader::device::get_device;
use rmreader::render::{render_collection, typst_doc};
use rmreader::theme::load_theme;

fn sample() -> (Vec<typst_doc::Row>, Vec<typst_doc::Article>) {
    let rows = vec![
        typst_doc::Row {
            num: "01".into(),
            title: "First article".into(),
            author: "A".into(),
            reading_time: "2 mins".into(),
            anchor: "article-a".into(),
        },
        typst_doc::Row {
            num: "02".into(),
            title: "Second article".into(),
            author: "B".into(),
            reading_time: "3 mins".into(),
            anchor: "article-b".into(),
        },
    ];
    let articles = vec![
        typst_doc::Article {
            anchor: "article-a".into(),
            title: "First article".into(),
            byline: "A · 2 mins".into(),
            body: "The quick brown fox jumps over the lazy dog and keeps running well past \
                   the right edge so the paragraph must wrap across several lines on the page."
                .into(),
        },
        typst_doc::Article {
            anchor: "article-b".into(),
            title: "Second article".into(),
            byline: "B · 3 mins".into(),
            body: "Second body.".into(),
        },
    ];
    (rows, articles)
}

#[test]
fn renders_internal_links_and_pages() {
    let device = get_device("paper-pro-move").unwrap();
    let theme = load_theme("reader").unwrap();
    let (rows, articles) = sample();
    let r = render_collection(&device, &theme, "Feed", &rows, &articles, &[]).unwrap();

    let doc = lopdf::Document::load_mem(&r.pdf).unwrap();
    // index page + 2 articles
    assert_eq!(doc.get_pages().len(), 3);

    // At least the two index-row links + per-page nav links.
    let mut links = 0;
    for pid in doc.get_pages().into_values() {
        if let Ok(annots) = doc
            .get_dictionary(pid)
            .and_then(|p| p.get(b"Annots"))
            .and_then(|a| a.as_array())
        {
            for a in annots {
                if let Ok(ad) = a.as_reference().and_then(|id| doc.get_dictionary(id)) {
                    if ad.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) == Some(b"Link") {
                        links += 1;
                    }
                }
            }
        }
    }
    assert!(links >= 2, "expected internal links, got {links}");

    // page_range recovered for both articles.
    assert_eq!(r.page_ranges.get("article-a").unwrap().first, 1);
    assert_eq!(r.page_ranges.get("article-b").unwrap().first, 2);
    // action band rects recovered.
    assert_eq!(r.label_rects.len(), 4);
}

#[test]
fn text_layer_is_clean_no_actualtext_duplication() {
    // The defining property: a wrapped paragraph must extract exactly once. Under
    // fulgur it extracted once per visual line (whole-paragraph /ActualText).
    let device = get_device("paper-pro-move").unwrap();
    let theme = load_theme("reader").unwrap();
    let (rows, articles) = sample();
    let r = render_collection(&device, &theme, "Feed", &rows, &articles, &[]).unwrap();

    // No /ActualText anywhere in the (decompressed) content streams.
    let doc = lopdf::Document::load_mem(&r.pdf).unwrap();
    let mut actualtext = 0usize;
    for obj in doc.objects.values() {
        if let lopdf::Object::Stream(s) = obj {
            let bytes = s
                .decompressed_content()
                .unwrap_or_else(|_| s.content.clone());
            actualtext += String::from_utf8_lossy(&bytes)
                .matches("ActualText")
                .count();
        }
    }
    assert_eq!(
        actualtext, 0,
        "Typst output must not emit /ActualText spans"
    );
}

#[test]
fn render_is_deterministic() {
    let device = get_device("paper-pro-move").unwrap();
    let theme = load_theme("reader").unwrap();
    let (rows, articles) = sample();
    let a = render_collection(&device, &theme, "Feed", &rows, &articles, &[]).unwrap();
    let b = render_collection(&device, &theme, "Feed", &rows, &articles, &[]).unwrap();
    assert_eq!(a.pdf, b.pdf, "same input must produce byte-identical PDF");
}
