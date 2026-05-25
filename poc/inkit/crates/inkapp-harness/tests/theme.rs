mod common;
use common::assert_golden;

use inkapp_core::geometry::PageGeom;
use inkapp_core::manifest::recover_regions;
use inkapp_core::render::compile_to_document;
use inkapp_core::Theme;
use inkapp_harness::inspector::inspect;

/// Render the reader theme over a heading + justified body + block quote + raw span,
/// and lock the styled output to a committed golden. Exercises every prelude rule.
#[test]
fn theme_reader() {
    let geom = PageGeom::default();
    let page = format!(
        "#set page(width: {}pt, height: {}pt, margin: {}pt)\n",
        geom.w, geom.h, geom.margin
    );
    let content = r#"= The Reading Room

Typography is the craft of arranging type to make written language legible, readable, and appealing. The quick brown fox jumps over the lazy dog, and then does it again for good measure.

#quote(block: true)[Good design is as little design as possible.]

Inline `code` sits in the body, and emphasis reads as _italic_.
"#;
    let src = format!("{page}{}{content}", Theme::reader().prelude());

    let doc = compile_to_document(&src).unwrap();
    let manifest = recover_regions(&doc).unwrap().with_version(1);
    let png = inspect(&doc, &manifest, &[]).unwrap();
    assert_golden("theme_reader", &png);
}
