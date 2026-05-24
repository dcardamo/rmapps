use inkapp_core::crypto::Key;
use inkapp_core::embed::{embed_manifest, extract_manifest};
use inkapp_core::error::Error;
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
        ..Default::default()
    };

    let key = Key::from_bytes([3u8; 32]);
    let embedded = embed_manifest(&pdf, &manifest, &key).unwrap();
    assert!(embedded.starts_with(b"%PDF"));
    // No-cleartext tier: the region name must not appear in the raw PDF bytes.
    assert!(
        !embedded.windows(4).any(|w| w == b"done"),
        "region name leaked into the PDF in cleartext"
    );
    let got = extract_manifest(&embedded, &key).unwrap();
    assert_eq!(got, manifest);
}

#[test]
fn extract_from_unembedded_pdf_errors() {
    // A freshly compiled PDF carries no InkappManifest key; extraction must
    // return an error, not panic.
    let doc = compile_to_document("#set page(width: 100pt, height: 100pt)\nhi").unwrap();
    let pdf = document_to_pdf(&doc).unwrap();
    let key = Key::from_bytes([3u8; 32]);
    assert!(
        extract_manifest(&pdf, &key).is_err(),
        "plain PDF has no manifest to extract"
    );
}

#[test]
fn extract_with_wrong_key_fails() {
    let doc = compile_to_document("#set page(width: 100pt, height: 100pt)\nhi").unwrap();
    let pdf = document_to_pdf(&doc).unwrap();
    let manifest = Manifest {
        version: 1,
        regions: vec![],
        ..Default::default()
    };
    let embedded = embed_manifest(&pdf, &manifest, &Key::from_bytes([1u8; 32])).unwrap();
    let got = extract_manifest(&embedded, &Key::from_bytes([2u8; 32]));
    assert!(
        matches!(got, Err(Error::Crypto(_))),
        "wrong key must fail to open"
    );
}
