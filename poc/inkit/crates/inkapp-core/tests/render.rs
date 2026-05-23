use inkapp_core::render::{compile_to_document, document_to_pdf};

const SRC: &str = r#"#set page(width: 200pt, height: 200pt, margin: 10pt)
= Hello
Some body text."#;

#[test]
fn compiles_and_is_deterministic() {
    let d1 = compile_to_document(SRC).expect("compile 1");
    let p1 = document_to_pdf(&d1).expect("pdf 1");
    assert!(p1.starts_with(b"%PDF"), "produces a PDF");

    let d2 = compile_to_document(SRC).expect("compile 2");
    let p2 = document_to_pdf(&d2).expect("pdf 2");
    assert_eq!(p1, p2, "same source -> identical PDF bytes");
}
