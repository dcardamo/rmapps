use inkapp_core::render::compile_to_document_with_sources;

#[test]
fn imported_source_resolves_and_compiles() {
    let lib = (
        "/lib/greet.typ".to_string(),
        "#let greet(who) = [Hello #who]\n".to_string(),
    );
    let main = "#set page(width: 120pt, height: 60pt)\n\
                #import \"/lib/greet.typ\": *\n\
                #greet(\"world\")\n";
    let doc = compile_to_document_with_sources(main, &[lib]).expect("compiles with import");
    assert_eq!(doc.pages.len(), 1);
}

#[test]
fn missing_import_fails() {
    let main = "#set page(width: 120pt, height: 60pt)\n\
                #import \"/lib/absent.typ\": *\n\
                #absent()\n";
    // No sources registered: the import cannot resolve, so compilation fails.
    assert!(compile_to_document_with_sources(main, &[]).is_err());
}
