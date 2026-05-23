use lopdf::Document;

#[test]
fn compiles_hello_to_single_page_pdf() {
    let pdf = typst_readback::compile_pdf("= Hello").expect("compile");
    assert!(pdf.starts_with(b"%PDF"), "missing PDF header");
    let doc = Document::load_mem(&pdf).expect("parse pdf");
    assert_eq!(doc.get_pages().len(), 1, "expected one page");
}
