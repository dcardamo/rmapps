use inkapp_core::geometry::PdfPoint;
use inkapp_core::ink::{RegionInk, Stroke};
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::compile_to_document;
use inkapp_core::widget::{RenderCx, Widget};
use inkapp_core::widgets::highlight_text::HighlightableText;

const TOKENS: &[&str] = &["the", "quick", "brown", "fox", "lazy", "dog"];

fn rendered_manifest(w: &HighlightableText) -> inkapp_core::manifest::Manifest {
    let mut cx = RenderCx::new(0);
    let body = w.render(&mut cx);
    let src = format!("#set page(width: 300pt, height: 120pt, margin: 10pt)\n{body}");
    let doc = compile_to_document(&src).unwrap();
    recover_regions(&doc).unwrap()
}

#[test]
fn renders_one_region_per_token() {
    let w = HighlightableText::new(TOKENS);
    let m = rendered_manifest(&w);
    let toks: Vec<&inkapp_core::manifest::Region> = m
        .regions
        .iter()
        .filter(|r| r.name.starts_with("tok-"))
        .collect();
    assert_eq!(toks.len(), TOKENS.len(), "one region per token");
    // Reading order: tok-0 left of tok-1 left of tok-2 ... on the same line.
    let mut ordered: Vec<&inkapp_core::manifest::Region> = toks.clone();
    ordered.sort_by_key(|r| r.name.clone());
    for pair in ordered.windows(2) {
        assert!(
            pair[0].rect.x0 <= pair[1].rect.x0,
            "tokens ordered left-to-right: {} then {}",
            pair[0].name,
            pair[1].name
        );
    }
}

#[test]
fn read_returns_highlighted_tokens() {
    let w = HighlightableText::new(TOKENS);
    let m = rendered_manifest(&w);

    // Build a highlighter swipe spanning the rects of "lazy" (idx 4) and "dog" (idx 5).
    let lazy = m.regions.iter().find(|r| r.name == "tok-4").unwrap().rect;
    let dog = m.regions.iter().find(|r| r.name == "tok-5").unwrap().rect;
    let y = (lazy.y0 + lazy.y1) / 2.0;
    let swipe = Stroke {
        points: vec![PdfPoint { x: lazy.x0, y }, PdfPoint { x: dog.x1, y }],
        highlighter: true,
    };

    // Feed both regions' ink directly (the simulator does attribution in the full
    // pipeline; here we hand the widget the ink for the regions it owns).
    let ink = vec![
        RegionInk {
            region: "tok-4".into(),
            strokes: vec![swipe.clone()],
        },
        RegionInk {
            region: "tok-5".into(),
            strokes: vec![swipe],
        },
    ];

    let mut got = w.read(&ink, &m);
    got.sort();
    assert_eq!(got, vec!["dog".to_string(), "lazy".to_string()]);
}

#[test]
fn non_highlighter_strokes_are_ignored() {
    let w = HighlightableText::new(TOKENS);
    let m = rendered_manifest(&w);

    // The same swipe across "lazy"/"dog", but drawn with a pen (not a highlighter):
    // it must NOT mark any token as highlighted.
    let lazy = m.regions.iter().find(|r| r.name == "tok-4").unwrap().rect;
    let dog = m.regions.iter().find(|r| r.name == "tok-5").unwrap().rect;
    let y = (lazy.y0 + lazy.y1) / 2.0;
    let pen = Stroke {
        points: vec![PdfPoint { x: lazy.x0, y }, PdfPoint { x: dog.x1, y }],
        highlighter: false,
    };
    let ink = vec![
        RegionInk {
            region: "tok-4".into(),
            strokes: vec![pen.clone()],
        },
        RegionInk {
            region: "tok-5".into(),
            strokes: vec![pen],
        },
    ];

    assert!(w.read(&ink, &m).is_empty(), "pen strokes do not highlight");
}
