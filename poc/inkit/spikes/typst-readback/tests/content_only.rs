use lopdf::Document;

/// Extract MediaBox arrays (as Vec<f64>) for every page in page-number order.
/// lopdf 0.36: `get_pages()` → BTreeMap<u32, ObjectId> (already sorted by page#),
/// `Dictionary::get` → Result<&Object>, `as_float` → Result<f32>,
/// `as_i64` → Result<i64>.
fn mediaboxes(pdf: &[u8]) -> Vec<Vec<f64>> {
    let doc = Document::load_mem(pdf).unwrap();
    doc.get_pages()
        .values()
        .map(|&id| {
            let page = doc.get_dictionary(id).unwrap();
            page.get(b"MediaBox")
                .and_then(|m| m.as_array())
                .unwrap()
                .iter()
                .map(|o| {
                    // as_float returns f32; try integer fallback for whole-number values.
                    o.as_float()
                        .map(|f| f as f64)
                        .or_else(|_| o.as_i64().map(|i| i as f64))
                        .expect("MediaBox element is neither float nor integer")
                })
                .collect()
        })
        .collect()
}

#[test]
fn appending_trailing_pages_preserves_leading_pages() {
    let v1 = typst_readback::compile_pdf(&typst_readback::test_docs::v1()).unwrap();
    let v2 = typst_readback::compile_pdf(&typst_readback::test_docs::v2()).unwrap();

    let mb1 = mediaboxes(&v1);
    let mb2 = mediaboxes(&v2);

    assert!(
        mb2.len() > mb1.len(),
        "v2 should have more pages than v1: v1={} v2={}",
        mb1.len(),
        mb2.len()
    );
    assert_eq!(
        &mb2[..mb1.len()],
        &mb1[..],
        "leading MediaBoxes changed between v1 and v2"
    );
}

#[test]
fn leading_region_rect_is_stable() {
    let (_p1, r1, h1) =
        typst_readback::regions::compile_with_regions(&typst_readback::test_docs::v1()).unwrap();
    let (_p2, r2, h2) =
        typst_readback::regions::compile_with_regions(&typst_readback::test_docs::v2()).unwrap();

    let a1 = r1
        .iter()
        .find(|r| r.name == "lead")
        .expect("\"lead\" region not found in v1");
    let a2 = r2
        .iter()
        .find(|r| r.name == "lead")
        .expect("\"lead\" region not found in v2");

    let rect1 = typst_readback::regions::typst_to_pdf_rect(a1, h1);
    let rect2 = typst_readback::regions::typst_to_pdf_rect(a2, h2);

    let close = |x: f64, y: f64| (x - y).abs() < 1.0;
    assert!(
        close(rect1.x0, rect2.x0)
            && close(rect1.y0, rect2.y0)
            && close(rect1.x1, rect2.x1)
            && close(rect1.y1, rect2.y1),
        "leading \"lead\" region moved between v1 and v2: v1={rect1:?}  v2={rect2:?}"
    );
}
