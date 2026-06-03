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

#[test]
fn feed_index_emits_mark_all_read_region() {
    let device = get_device("paper-pro-move").unwrap();
    let theme = load_theme("reader").unwrap();
    let (rows, articles) = sample();
    let r = render_collection(&device, &theme, "Feed", &rows, &articles, &[]).unwrap();
    let m = r.mark_all_read.expect("Feed must emit a mark-all-read region");
    assert_eq!(m.page, 0, "button is on the index page");
    assert!(m.rect.x1 > m.rect.x0 && m.rect.y1 > m.rect.y0, "rect must be non-empty: {m:?}");
}

#[test]
fn library_index_has_no_mark_all_read_region() {
    let device = get_device("paper-pro-move").unwrap();
    let theme = load_theme("reader").unwrap();
    let (rows, articles) = sample();
    let r = render_collection(&device, &theme, "Library", &rows, &articles, &[]).unwrap();
    assert!(r.mark_all_read.is_none(), "Library must not render the button");
}

#[test]
fn inline_code_followed_by_dot_field_compiles() {
    // Regression: an inline <code> span immediately followed by ".data" (no space)
    // used to emit `#raw("…").data`, which Typst parsed as a field access and
    // failed to compile ("raw does not have field data"). The body below mirrors
    // that real feed content; rendering must succeed.
    let device = get_device("paper-pro-move").unwrap();
    let theme = load_theme("reader").unwrap();
    let body = rmreader::render::html2typst::convert(
        "<p>region <code>0x100089E8</code>.data init values for kernel SRAM</p>",
    );
    let articles = vec![typst_doc::Article {
        anchor: "article-x".into(),
        title: "Mem map".into(),
        byline: "A".into(),
        body,
    }];
    let rows = vec![typst_doc::Row {
        num: "01".into(),
        title: "Mem map".into(),
        author: "A".into(),
        reading_time: "1 min".into(),
        anchor: "article-x".into(),
    }];
    let r = render_collection(&device, &theme, "Feed", &rows, &articles, &[]);
    assert!(r.is_ok(), "inline code + .data must compile: {:?}", r.err());
}

/// Count Link annotations on a single 0-based page index.
fn links_on_page(pdf: &[u8], page_index: usize) -> usize {
    let doc = lopdf::Document::load_mem(pdf).unwrap();
    let pages: Vec<_> = doc.get_pages().into_values().collect();
    let pid = pages[page_index];
    let mut n = 0;
    if let Ok(annots) = doc
        .get_dictionary(pid)
        .and_then(|p| p.get(b"Annots"))
        .and_then(|a| a.as_array())
    {
        for a in annots {
            if let Ok(ad) = a.as_reference().and_then(|id| doc.get_dictionary(id)) {
                if ad.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) == Some(b"Link") {
                    n += 1;
                }
            }
        }
    }
    n
}

#[test]
fn index_page_has_nav_bar_links() {
    let device = get_device("paper-pro-move").unwrap();
    let theme = load_theme("reader").unwrap();
    let (rows, articles) = sample();
    let r = render_collection(&device, &theme, "Feed", &rows, &articles, &[]).unwrap();
    // Index is page 0. Home always links; with articles present, Next links too.
    // The two index rows also link. So the index page has multiple Link annots.
    assert!(
        links_on_page(&r.pdf, 0) >= 3,
        "index page should carry Home + Next + row links, got {}",
        links_on_page(&r.pdf, 0)
    );
}
