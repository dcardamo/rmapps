use inkapp_core::manifest::recover_regions;
use inkapp_core::render::{compile_to_document, compile_to_document_with_sources};
use inkapp_core::runtime::REGION_PRELUDE;
use inkapp_core::widget::region_metadata;

const PAGE: &str = "#set page(width: 200pt, height: 120pt, margin: 12pt)\n";

#[test]
fn region_prelude_matches_inline_pattern() {
    // Via the prelude: import #region, wrap a fixed-size box.
    let prelude = (REGION_PRELUDE.0.to_string(), REGION_PRELUDE.1.to_string());
    let via_prelude = format!(
        "{PAGE}#import \"{}\": *\n#region(\"r\", box(width: 14pt, height: 14pt, stroke: 0.5pt))\n",
        REGION_PRELUDE.0
    );
    let d1 = compile_to_document_with_sources(&via_prelude, &[prelude]).unwrap();
    let m1 = recover_regions(&d1).unwrap();
    let r1 = m1
        .regions
        .iter()
        .find(|r| r.name == "r")
        .expect("prelude region");

    // Via the legacy inline helper at the same laid-out point (top-left of the
    // content area, page 0): the box lands at the margin (12,12).
    let inline = format!("{PAGE}{}", region_metadata("r", 0, 12.0, 12.0, 14.0, 14.0));
    let d2 = compile_to_document(&inline).unwrap();
    let m2 = recover_regions(&d2).unwrap();
    let r2 = m2
        .regions
        .iter()
        .find(|r| r.name == "r")
        .expect("inline region");

    for (a, b, edge) in [
        (r1.rect.x0, r2.rect.x0, "x0"),
        (r1.rect.y0, r2.rect.y0, "y0"),
        (r1.rect.x1, r2.rect.x1, "x1"),
        (r1.rect.y1, r2.rect.y1, "y1"),
    ] {
        assert!(
            (a - b).abs() < 0.01,
            "edge {edge}: prelude {a} vs inline {b}"
        );
    }
}
