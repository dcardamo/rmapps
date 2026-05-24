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

#[test]
fn state_round_trips_and_stays_sealed() {
    use inkapp_core::manifest::DocState;
    use serde_json::json;

    let doc = compile_to_document("#set page(width: 100pt, height: 100pt)\nhi").unwrap();
    let pdf = document_to_pdf(&doc).unwrap();

    let mut state = DocState::default();
    // A distinctive marker we can search for in cleartext.
    state.doc = Some(json!({"marker": "SEKRIT_CURSOR_7"}));
    state
        .components
        .insert("stepper:c".into(), json!(424242u64));

    let manifest = Manifest {
        version: 3,
        regions: vec![],
        state,
    };

    let key = Key::from_bytes([5u8; 32]);
    let embedded = embed_manifest(&pdf, &manifest, &key).unwrap();

    // No-cleartext tier: neither the doc marker nor the component value leaks.
    assert!(
        !embedded.windows(15).any(|w| w == b"SEKRIT_CURSOR_7"),
        "doc-level state leaked into the PDF in cleartext"
    );
    assert!(
        !embedded.windows(6).any(|w| w == b"424242"),
        "component state value leaked into the PDF in cleartext"
    );

    let got = extract_manifest(&embedded, &key).unwrap();
    assert_eq!(got, manifest);
    assert_eq!(got.state.doc, manifest.state.doc);
    assert_eq!(
        got.state.components.get("stepper:c"),
        Some(&json!(424242u64))
    );
}
