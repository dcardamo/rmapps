use fulgur::config::{Margin, PageSize};
use fulgur::engine::Engine;

#[test]
fn fulgur_renders_pdf() {
    let engine = Engine::builder()
        .page_size(PageSize { width: 260.18, height: 462.55 })
        .margin(Margin::uniform(0.0))
        .build();
    let pdf = engine.render_html("<h1>rmbujo</h1>").expect("render");
    assert!(pdf.len() > 100, "expected a non-trivial PDF, got {} bytes", pdf.len());
}
