fn pdf_text(pdf: &[u8]) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("d.pdf"), pdf).unwrap();
    let out = std::process::Command::new("pdftotext")
        .args(["d.pdf", "-"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn html_article_renders_with_expected_text() {
    let html = include_str!("fixtures/article.html");
    let typ = typst_readback::html::html_to_typst(html);
    eprintln!("=== Generated Typst ===\n{typ}\n=== End Typst ===");
    let pdf = typst_readback::compile_pdf(&typ).unwrap();
    let text = pdf_text(&pdf);
    eprintln!("=== Extracted PDF text ===\n{text}\n=== End PDF text ===");
    for needle in ["Article Title", "first list item", "second paragraph"] {
        assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
    }
}
