use inkapp_core::manifest::recover_regions;
use inkapp_core::render::compile_to_document;

// A 200x200pt page; a 50x30pt metadata-labelled region at top-left (20,20).
const SRC: &str = r#"#set page(width: 200pt, height: 200pt, margin: 0pt)
#place(top + left, dx: 20pt, dy: 20pt,
  box(width: 50pt, height: 30pt)[
    #metadata((name: "done", page: 0, x: 20.0, y: 20.0, w: 50.0, h: 30.0)) <region>
  ]
)"#;

#[test]
fn recovers_region_in_pdf_coords() {
    let doc = compile_to_document(SRC).unwrap();
    let manifest = recover_regions(&doc).unwrap();
    assert_eq!(manifest.regions.len(), 1);
    let r = &manifest.regions[0];
    assert_eq!(r.name, "done");
    assert_eq!(r.page, 0);
    // Typst top-left (20,20,50,30) on a 200pt-high page -> PDF bottom-left:
    //   x0=20, y0=200-(20+30)=150, x1=70, y1=200-20=180
    assert!((r.rect.x0 - 20.0).abs() < 1e-9, "x0");
    assert!((r.rect.y0 - 150.0).abs() < 1e-9, "y0");
    assert!((r.rect.x1 - 70.0).abs() < 1e-9, "x1");
    assert!((r.rect.y1 - 180.0).abs() < 1e-9, "y1");
}
