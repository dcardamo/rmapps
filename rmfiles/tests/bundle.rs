//! Integration tests for [`rmfiles::Bundle`].

use std::path::Path;

fn strokes_count(p: &Path) -> usize {
    let b = rmfiles::Bundle::open(p).unwrap();
    b.pages()
        .iter()
        .filter_map(|pg| pg.scene().unwrap())
        .map(|s| s.strokes().len())
        .sum()
}

#[test]
fn opens_zip_and_dir_identically() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let rmdoc = dir.join("stamped-labels.rmdoc");

    let from_zip = strokes_count(&rmdoc);
    assert!(
        from_zip >= 2,
        "expected >=2 strokes from zip, got {from_zip}"
    );

    let tmp = tempfile::tempdir().unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&rmdoc).unwrap()).unwrap();
    zip.extract(tmp.path()).unwrap();

    assert_eq!(strokes_count(tmp.path()), from_zip);
}

#[test]
fn exposes_source_pdf_metadata_and_canvas() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let b = rmfiles::Bundle::open(&dir.join("stamped-labels.rmdoc")).unwrap();

    assert!(
        b.source_pdf().is_some(),
        "bundle should contain a source PDF"
    );
    assert!(
        !b.pages().is_empty(),
        "bundle should have at least one page"
    );
    assert_eq!(b.canvas_size(), (1404.0, 1872.0));
}
