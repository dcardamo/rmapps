use rmreader::device::get_device;
use rmreader::render::render_pdf;
use rmreader::theme::load_theme;

#[test]
fn renders_pdf_with_resolving_internal_link() {
    let device = get_device("paper-pro-move").unwrap();
    let theme = load_theme("reader").unwrap();
    let frags = vec![
        r##"<section class="page" id="index"><a href="#article-a">go</a></section>"##.to_string(),
        r##"<section class="page article" id="article-a"><h2 class="headline">A</h2><div class="body"><p>hi</p></div></section>"##.to_string(),
    ];
    let out = std::env::temp_dir().join("rmreader_render.pdf");
    render_pdf(&device, &theme, &frags, &[], &out).unwrap();

    let doc = lopdf::Document::load(&out).unwrap();
    // find at least one /Link annotation
    let mut links = 0;
    for (_n, pid) in doc.get_pages() {
        if let Ok(annots) = doc
            .get_dictionary(pid)
            .and_then(|p| p.get(b"Annots"))
            .and_then(|a| a.as_array())
        {
            for a in annots {
                if let Ok(id) = a.as_reference() {
                    if let Ok(ad) = doc.get_dictionary(id) {
                        if ad.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) == Some(b"Link") {
                            links += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(
        links >= 1,
        "expected at least one Link annotation, got {links}"
    );
}
