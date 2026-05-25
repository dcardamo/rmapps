use inkapp_content::Article;
use inkapp_core::component::{Component, RenderCx};
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::compile_to_document;
use inkapp_harness::simulator::{simulate, Gesture, Scenario};
use rm_device::Remarkable;

// Keep this HTML identical to the one used to build the Article so token indices line up.
const HTML: &str = "<h2>Heading</h2><p>the quick brown <strong>fox</strong> jumps</p>\
                    <ul><li>alpha</li><li>beta</li></ul>";

#[test]
fn article_decodes_swipe_to_coalesced_span() {
    let article: Article<String> = Article::new(HTML, &[], |s| s.to_string());

    let body = article.render(&mut RenderCx::new(0));
    let src = format!("#set page(width: 400pt, height: 600pt, margin: 16pt)\n{body}");
    let doc = compile_to_document(&src).unwrap();
    let manifest = recover_regions(&doc).unwrap().with_version(1);
    let device = Remarkable::new();

    // Recover token indices by converting the same HTML (Article's token list is private).
    let tokens: Vec<String> = inkapp_content::convert(HTML, &[])
        .tokens
        .into_iter()
        .map(|t| t.text)
        .collect();
    let idx = |w: &str| tokens.iter().position(|t| t == w).unwrap();
    let brown = idx("brown");
    let fox = idx("fox");

    let scenario = Scenario::new()
        .mark(&format!("tok-{brown}"), Gesture::Swipe)
        .mark(&format!("tok-{fox}"), Gesture::Swipe);
    let trace = simulate(&src, &manifest, &device, &scenario).unwrap();

    let got = article.read(&trace.readback, &manifest);
    assert_eq!(
        got,
        vec!["brown fox".to_string()],
        "contiguous swipe coalesces"
    );
}
