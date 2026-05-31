use rmbujo::render::compile_pdf;

#[test]
fn typst_renders_pdf() {
    // The Typst pipeline compiles a trivial document to real PDF bytes.
    let pdf = compile_pdf(
        "#set page(width: 260pt, height: 462pt, margin: 0pt)\n= rmbujo",
        &[],
    )
    .expect("render");
    assert!(pdf.starts_with(b"%PDF"), "expected a PDF header");
    assert!(
        pdf.len() > 100,
        "expected a non-trivial PDF, got {} bytes",
        pdf.len()
    );
}
