use rmreader::readback::coords::PdfRect;
use rmreader::readback::textlayer::TextLayer;

fn source_pdf() -> Vec<u8> {
    let fx = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rmfiles/tests/fixtures/stamped-labels.rmdoc");
    rmfiles::Bundle::open(&fx)
        .unwrap()
        .source_pdf()
        .unwrap()
        .to_vec()
}

#[test]
fn extracts_words_with_sane_boxes() {
    let tl = TextLayer::extract(&source_pdf()).unwrap();
    let find = |w: &str| tl.words.iter().find(|x| x.text == w);
    for w in ["ARCHIVE", "quick", "brown", "fox"] {
        let word = find(w).unwrap_or_else(|| panic!("missing word {w}"));
        assert!(
            word.bbox.x1 > word.bbox.x0 && word.bbox.y1 > word.bbox.y0,
            "degenerate box for {w}"
        );
    }
    // Labels are stamped near the top -> high bottom-left y; body sentence lower.
    let archive = find("ARCHIVE").unwrap();
    let fox = find("fox").unwrap();
    assert!(
        archive.bbox.y0 > fox.bbox.y0,
        "ARCHIVE (top) should have higher y than fox (body); ARCHIVE y0={:.1} fox y0={:.1}",
        archive.bbox.y0,
        fox.bbox.y0
    );
}

#[test]
fn words_under_reconstructs_body_sentence() {
    let tl = TextLayer::extract(&source_pdf()).unwrap();
    // Build a rect spanning the body line by unioning the known body words' boxes.
    let body_words = [
        "The",
        "quick",
        "brown",
        "fox",
        "jumps",
        "over",
        "the",
        "lazy",
        "dog",
        "near",
        "the",
        "riverbank.",
    ];
    let mut rect: Option<PdfRect> = None;
    for w in tl
        .words
        .iter()
        .filter(|x| body_words.contains(&x.text.as_str()))
    {
        rect = Some(match rect {
            None => x_clone(&w.bbox),
            Some(r) => PdfRect {
                x0: r.x0.min(w.bbox.x0),
                y0: r.y0.min(w.bbox.y0),
                x1: r.x1.max(w.bbox.x1),
                y1: r.y1.max(w.bbox.y1),
            },
        });
    }
    let rect = rect.expect("found body words");
    let text = tl.words_under(0, &rect);
    assert!(text.contains("quick brown fox"), "got: {text:?}");
}

fn x_clone(r: &PdfRect) -> PdfRect {
    *r
}
