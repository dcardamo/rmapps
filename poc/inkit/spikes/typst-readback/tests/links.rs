use lopdf::Document;

fn link_annotations(doc: &Document) -> Vec<lopdf::Dictionary> {
    let mut out = Vec::new();
    for (_id, obj) in doc.objects.iter() {
        if let Ok(d) = obj.as_dict() {
            if d.get(b"Subtype").and_then(|o| o.as_name()).ok() == Some(b"Link") {
                out.push(d.clone());
            }
        }
    }
    out
}

#[test]
fn pdf_has_internal_and_external_links() {
    let pdf = typst_readback::compile_pdf(include_str!("fixtures/links.typ")).unwrap();
    let doc = Document::load_mem(&pdf).unwrap();
    let anns = link_annotations(&doc);
    assert!(
        anns.len() >= 2,
        "expected >=2 link annotations, got {}",
        anns.len()
    );

    let has_uri = anns.iter().any(|d| {
        d.get(b"A")
            .and_then(|a| a.as_dict())
            .ok()
            .and_then(|a| a.get(b"URI").ok())
            .and_then(|u| u.as_str().ok())
            .map(|s| s == b"https://example.com")
            .unwrap_or(false)
    });
    assert!(has_uri, "missing external URI link");

    let has_internal = anns.iter().any(|d| {
        d.has(b"Dest")
            || d.get(b"A")
                .and_then(|a| a.as_dict())
                .ok()
                .and_then(|a| a.get(b"S").ok())
                .and_then(|s| s.as_name().ok())
                == Some(b"GoTo")
    });
    assert!(has_internal, "missing internal destination link");
}
