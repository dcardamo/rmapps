//! Integration tests for [`rm_files::Bundle`].

use std::path::Path;

fn strokes_count(p: &Path) -> usize {
    let b = rm_files::Bundle::open(p).unwrap();
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

/// `scene_bytes()` returns the raw `.rm` bytes for an inked page (in `.content`
/// order) and `None` for an un-inked page — the accessor the device-pull needs to
/// assemble per-page ink without re-parsing `.content`.
#[test]
fn scene_bytes_returns_raw_rm_or_none_in_content_order() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // An unpacked bundle: `<uuid>.content` listing two pages in order; only the
    // first page has an `.rm` file (the second is un-inked).
    let uuid = "doc-uuid";
    std::fs::write(
        root.join(format!("{uuid}.content")),
        br#"{"cPages":{"pages":[{"id":"page-a"},{"id":"page-b"}]}}"#,
    )
    .unwrap();
    std::fs::create_dir(root.join(uuid)).unwrap();
    let rm_a = b"\x00raw-rm-bytes-for-a";
    std::fs::write(root.join(uuid).join("page-a.rm"), rm_a).unwrap();

    let bundle = rm_files::Bundle::open(root).unwrap();
    let pages = bundle.pages();
    assert_eq!(pages.len(), 2, "both pages listed in .content order");
    assert_eq!(
        pages[0].scene_bytes(),
        Some(&rm_a[..]),
        "page A (inked) returns its raw .rm bytes"
    );
    assert_eq!(pages[1].scene_bytes(), None, "page B (no .rm) returns None");
}

#[test]
fn exposes_source_pdf_metadata_and_canvas() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let b = rm_files::Bundle::open(&dir.join("stamped-labels.rmdoc")).unwrap();

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
