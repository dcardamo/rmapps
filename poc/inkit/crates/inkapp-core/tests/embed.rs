use inkapp_core::embed::{embed_manifest, extract_manifest};
use inkapp_core::geometry::PdfRect;
use inkapp_core::manifest::{Manifest, Region};
use inkapp_core::render::{compile_to_document, document_to_pdf};

#[test]
fn manifest_round_trips_through_pdf() {
    let doc = compile_to_document("#set page(width: 100pt, height: 100pt)\nhi").unwrap();
    let pdf = document_to_pdf(&doc).unwrap();

    let manifest = Manifest {
        version: 7,
        regions: vec![Region {
            name: "done".into(),
            page: 0,
            rect: PdfRect {
                x0: 1.0,
                y0: 2.0,
                x1: 3.0,
                y1: 4.0,
            },
        }],
    };

    let embedded = embed_manifest(&pdf, &manifest).unwrap();
    assert!(embedded.starts_with(b"%PDF"));
    let got = extract_manifest(&embedded).unwrap();
    assert_eq!(got, manifest);
}
